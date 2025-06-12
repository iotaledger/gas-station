// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! This module implements the access controller for the gas station.
//! It provides a way to control the constraints for executing transactions, ensuring that only authorized addresses can perform specific actions.

pub mod decision;
pub mod hook;
pub mod policy;
pub mod predicates;
pub mod rule;

use std::{collections::HashMap, fmt::Formatter, sync::Arc};

use anyhow::{anyhow, Context, Result};
use decision::Decision;
use hook::SkippableDecision;
use iota_types::digests::TransactionDigest;
use policy::AccessPolicy;
use predicates::Action;
use rule::{AccessRule, GasUsageConfirmationRequest, TransactionContext};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::debug;

use crate::tracker::StatsTracker;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct AccessController {
    pub access_policy: AccessPolicy,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub rules: Vec<AccessRule>,

    #[serde(skip)]
    confirmation_requests: Arc<Mutex<HashMap<TransactionDigest, Vec<GasUsageConfirmationRequest>>>>,
}

impl std::fmt::Debug for AccessController {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessController")
            .field("access_policy", &self.access_policy)
            .field("rules", &self.rules)
            .finish()
    }
}

impl AccessController {
    /// Creates a new instance of the access controller.
    pub fn new(access_policy: AccessPolicy, rules: impl IntoIterator<Item = AccessRule>) -> Self {
        Self {
            access_policy,
            rules: rules.into_iter().collect(),
            confirmation_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Initializes the access controller by loading the rules from the external sources
    pub async fn initialize(&mut self) -> Result<()> {
        for (i, rule) in &mut self.rules.iter_mut().enumerate() {
            debug!("Initializing access control rule {}", i + 1);
            rule.initialize().await?;
        }
        Ok(())
    }

    /// Checks if the transaction can be executed based on the access controller's rules.
    // If a rule matches, the corresponding action is applied. If no rule matches, the next rule is checked.
    // If none match, the default policy is applied.
    pub async fn check_access(&self, ctx: &TransactionContext) -> Result<Decision> {
        if self.is_disabled() {
            return Ok(Decision::Allow);
        }

        for (i, rule) in self.rules.iter().enumerate() {
            if rule
                .matches(&ctx)
                .await
                .with_context(|| anyhow!("Error evaluating rule #{}", i + 1))?
            {
                // Validate the counters if the rule partially matches
                let matching_result = rule.match_global_limits(ctx).await?;
                if !matching_result.1.is_empty() {
                    self.confirmation_requests
                        .lock()
                        .await
                        .insert(ctx.transaction_digest, matching_result.1);
                }
                // if the rule matches and also matches the global limits, invoke the action
                if matching_result.0 {
                    match &rule.action {
                        Action::Allow => return Ok(Decision::Allow),
                        Action::Deny => return Ok(Decision::Deny),
                        Action::HookAction(hook_action) => {
                            // call hook and take defined result or continue with next rule
                            let response = hook_action.call_hook(ctx).await?;
                            debug!("Called hook: {}, for transaction with digest: {}. Got decision: {:?}, with user message: {:?}",
                                    hook_action.url(),
                                    ctx.transaction_digest,
                                    response.decision,
                                    response.user_message,
                                );
                            match response.decision {
                                SkippableDecision::Allow => return Ok(Decision::Allow),
                                SkippableDecision::Deny => return Ok(Decision::Deny),
                                _ => (),
                            };
                        }
                    };
                }
            }
        }

        match self.access_policy {
            AccessPolicy::AllowAll => Ok(Decision::Allow),
            AccessPolicy::DenyAll => Ok(Decision::Deny),
            AccessPolicy::Disabled => Ok(Decision::Allow),
        }
    }

    pub async fn confirm_transaction(
        &self,
        result: TransactionExecutionResult,
        stats_tracker: &StatsTracker,
    ) -> Result<()> {
        let mut confirmation_requests = self.confirmation_requests.lock().await;
        let transaction_digest = result.transaction_digest;
        let maybe_requests = confirmation_requests.remove(&transaction_digest);
        if let Some(requests) = maybe_requests {
            for req in requests {
                let diff = if let Some(real_gas_usage) = result.gas_usage {
                    let reserved_gas_usage = req.gas_usage;
                    let diff = reserved_gas_usage - real_gas_usage;
                    debug!("Transaction with id: {transaction_digest} confirmed, reserved gas usage: {reserved_gas_usage}, real gas usage: {real_gas_usage}, diff: {diff}");
                    diff
                } else {
                    debug!("Transaction with id: {transaction_digest} confirmed, but no gas usage was provided");
                    req.gas_usage
                } as i64;
                stats_tracker
                    .update_aggr(req.rule_meta, &req.aggregate, diff * -1)
                    .await
                    .context("Failed to update aggregate while when confirming transactions")?;
            }
        }
        Ok(())
    }

    /// Adds a new rule to the access controller.
    pub fn add_rule(&mut self, rule: AccessRule) {
        self.rules.push(rule);
    }

    /// Adds multiple rules to the access controller.
    pub fn add_rules(&mut self, rules: impl IntoIterator<Item = AccessRule>) {
        self.rules.extend(rules);
    }

    /// Returns true if the access controller is disabled.
    pub fn is_disabled(&self) -> bool {
        self.access_policy == AccessPolicy::Disabled
    }

    pub async fn reload(&mut self, rules: Vec<AccessRule>, policy: AccessPolicy) -> Result<()> {
        self.rules = rules;
        self.access_policy = policy;
        self.initialize().await
    }
}

pub struct TransactionExecutionResult {
    pub transaction_digest: TransactionDigest,
    pub gas_usage: Option<u64>,
}
impl TransactionExecutionResult {
    pub fn new(transaction_digest: TransactionDigest) -> Self {
        Self {
            transaction_digest,
            gas_usage: None,
        }
    }
    pub fn with_gas_usage(mut self, gas_used: u64) -> Self {
        self.gas_usage = Some(gas_used);
        self
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use indoc::indoc;
    use iota_types::base_types::IotaAddress;
    use url::Url;

    use crate::access_controller::{
        decision::Decision,
        hook::{HookAction, HookActionDetailed, HookActionHeaders},
        predicates::{Action, ValueIotaAddress},
        AccessController,
    };

    use super::{
        policy::AccessPolicy,
        predicates::ValueNumber,
        rule::{AccessRuleBuilder, TransactionContext},
    };

    #[tokio::test]
    async fn test_deny_policy_rules_should_allow() {
        let sender_address = IotaAddress::new([1; 32]);
        let blocked_address = IotaAddress::new([2; 32]);
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .allow()
            .build();
        let to_allow_tx = TransactionContext {
            sender_address: sender_address.clone(),
            ..Default::default()
        };
        let denied_tx = TransactionContext {
            sender_address: blocked_address.clone(),
            ..Default::default()
        };

        let mut ac = AccessController::new(AccessPolicy::DenyAll, []);

        assert!(matches!(
            ac.check_access(&to_allow_tx).await,
            Ok(Decision::Deny)
        ));
        assert!(matches!(
            ac.check_access(&denied_tx).await,
            Ok(Decision::Deny)
        ));

        ac.add_rule(allow_rule);

        assert!(matches!(
            ac.check_access(&to_allow_tx).await,
            Ok(Decision::Allow)
        ));
        assert!(matches!(
            ac.check_access(&denied_tx).await,
            Ok(Decision::Deny)
        ));
    }

    #[tokio::test]
    async fn test_allow_policy_rules_should_block() {
        let blocked_address = IotaAddress::new([1; 32]);
        let sender_address = IotaAddress::new([2; 32]);

        let deny_rule = AccessRuleBuilder::new()
            .sender_address(blocked_address)
            .deny()
            .build();

        let to_deny_tx = TransactionContext {
            sender_address: blocked_address.clone(),
            ..Default::default()
        };
        let allowed_tx = TransactionContext {
            sender_address: sender_address.clone(),
            ..Default::default()
        };
        let mut ac = AccessController::new(AccessPolicy::AllowAll, []);

        assert!(matches!(
            ac.check_access(&allowed_tx).await,
            Ok(Decision::Allow)
        ));
        assert!(matches!(
            ac.check_access(&to_deny_tx).await,
            Ok(Decision::Allow)
        ));

        ac.add_rule(deny_rule);

        assert!(matches!(
            ac.check_access(&allowed_tx).await,
            Ok(Decision::Allow)
        ));
        assert!(matches!(
            ac.check_access(&to_deny_tx).await,
            Ok(Decision::Deny)
        ));
    }

    #[tokio::test]
    async fn test_deny_policy_rules_gas_budget() {
        let sender_address = IotaAddress::new([1; 32]);
        let gas_budget = 100;
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .gas_budget(ValueNumber::LessThan(gas_budget))
            .allow()
            .build();
        let allowed_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_budget - 1);
        let denied_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_budget);

        let ac = AccessController::new(AccessPolicy::DenyAll, [allow_rule]);

        assert!(matches!(
            ac.check_access(&allowed_tx).await,
            Ok(Decision::Allow)
        ));
        assert!(matches!(
            ac.check_access(&denied_tx).await,
            Ok(Decision::Deny)
        ));
    }

    #[tokio::test]
    async fn test_allow_policy_rules_gas_budget() {
        let sender_address = IotaAddress::new([1; 32]);
        let gas_budget = 100;
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .gas_budget(ValueNumber::GreaterThanOrEqual(gas_budget))
            .deny()
            .build();
        let allowed_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_budget - 1);
        let denied_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_budget);

        let ac = AccessController::new(AccessPolicy::AllowAll, [deny_rule]);
        assert!(matches!(
            ac.check_access(&allowed_tx).await,
            Ok(Decision::Allow)
        ));
        assert!(matches!(
            ac.check_access(&denied_tx).await,
            Ok(Decision::Deny)
        ));
    }

    #[tokio::test]
    async fn test_allow_policy_rules_move_call_package_address() {
        let sender_address = IotaAddress::new([1; 32]);
        let package_address = IotaAddress::new([2; 32]);
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .move_call_package_address(package_address)
            .deny()
            .build();
        let denied_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![package_address]);
        let allowed_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![IotaAddress::new([3; 32])]);

        let ac = AccessController::new(AccessPolicy::AllowAll, [deny_rule]);
        assert!(matches!(
            ac.check_access(&allowed_tx).await,
            Ok(Decision::Allow)
        ));
        assert!(matches!(
            ac.check_access(&denied_tx).await,
            Ok(Decision::Deny)
        ));
    }

    #[tokio::test]
    async fn test_deny_policy_rules_move_call_package_address() {
        let sender_address = IotaAddress::new([1; 32]);
        let package_address = IotaAddress::new([2; 32]);
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .move_call_package_address(package_address)
            .allow()
            .build();
        let allowed_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![package_address]);
        let denied_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![IotaAddress::new([3; 32])]);

        let ac = AccessController::new(AccessPolicy::DenyAll, [allow_rule]);
        assert!(matches!(
            ac.check_access(&allowed_tx).await,
            Ok(Decision::Allow)
        ));
        assert!(matches!(
            ac.check_access(&denied_tx).await,
            Ok(Decision::Deny)
        ));
    }

    #[tokio::test]
    async fn test_allow_policy_rules_ptb_command_count() {
        let sender_address = IotaAddress::new([1; 32]);
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .ptb_command_count(ValueNumber::GreaterThan(1))
            .deny()
            .build();
        let denied_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_ptb_command_count(5);
        let allowed_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_ptb_command_count(1);

        let ac = AccessController::new(AccessPolicy::AllowAll, [deny_rule]);
        assert!(matches!(
            ac.check_access(&allowed_tx).await,
            Ok(Decision::Allow)
        ));
        assert!(matches!(
            ac.check_access(&denied_tx).await,
            Ok(Decision::Deny)
        ));
    }

    #[tokio::test]
    async fn test_deny_policy_rules_ptb_command_count() {
        let sender_address = IotaAddress::new([1; 32]);
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .ptb_command_count(ValueNumber::LessThanOrEqual(1))
            .allow()
            .build();
        let allowed_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_ptb_command_count(1);
        let blocked_tx = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_ptb_command_count(5);

        let ac = AccessController::new(AccessPolicy::DenyAll, [allow_rule]);
        assert!(matches!(
            ac.check_access(&allowed_tx).await,
            Ok(Decision::Allow)
        ));
        assert!(matches!(
            ac.check_access(&blocked_tx).await,
            Ok(Decision::Deny)
        ));
    }

    #[test]
    fn deserialize_access_controller() {
        let yaml = r#"
access-policy: "deny-all"
rules:
      - sender-address: ['0x0101010101010101010101010101010101010101010101010101010101010101']
        transaction-gas-budget: <=10000
        ptb-command-count: <=5
        action: allow
"#;
        let ac: AccessController = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ac.access_policy, AccessPolicy::DenyAll);
        assert_eq!(ac.rules.len(), 1);
        assert_eq!(
            ac.rules[0].sender_address,
            ValueIotaAddress::List(vec![IotaAddress::new([1; 32])])
        );
        assert_eq!(
            ac.rules[0].transaction_gas_budget,
            Some(ValueNumber::LessThanOrEqual(10000))
        );
        assert_eq!(
            ac.rules[0].ptb_command_count,
            Some(ValueNumber::LessThanOrEqual(5))
        );
        assert_eq!(ac.rules[0].action, Action::Allow);
    }

    #[test]
    fn serialize_access_controller() {
        let ac = AccessController::new(
            AccessPolicy::DenyAll,
            [AccessRuleBuilder::new()
                .sender_address(IotaAddress::new([1; 32]))
                .gas_budget(ValueNumber::LessThanOrEqual(10000))
                .ptb_command_count(ValueNumber::LessThanOrEqual(5))
                .allow()
                .build()],
        );
        let yaml = serde_yaml::to_string(&ac).unwrap();
        println!("{}", yaml);

        assert_eq!(
            yaml,
            r#"---
access-policy: deny-all
rules:
  - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
    transaction-gas-budget: "<=10000"
    ptb-command-count: "<=5"
    action: allow
"#
        );
    }

    #[test]
    fn serialize_access_controller_with_move_call_package_address() {
        let ac = AccessController::new(
            AccessPolicy::DenyAll,
            [AccessRuleBuilder::new()
                .sender_address(IotaAddress::new([1; 32]))
                .move_call_package_address(IotaAddress::new([2; 32]))
                .allow()
                .build()],
        );
        let yaml = serde_yaml::to_string(&ac).unwrap();

        assert_eq!(
            yaml,
            r#"---
access-policy: deny-all
rules:
  - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
    move-call-package-address: "0x0202020202020202020202020202020202020202020202020202020202020202"
    action: allow
"#
        );
    }

    #[test]
    fn deserialize_access_controller_with_move_call_package_address() {
        let yaml = r#"
access-policy: "deny-all"
rules:
      - sender-address: ['0x0101010101010101010101010101010101010101010101010101010101010101']
        move-call-package-address: ['0x0202020202020202020202020202020202020202020202020202020202020202']
        action: allow
"#;
        let ac: AccessController = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ac.access_policy, AccessPolicy::DenyAll);
        assert_eq!(ac.rules.len(), 1);
        assert_eq!(
            ac.rules[0].sender_address,
            ValueIotaAddress::List(vec![IotaAddress::new([1; 32])])
        );
        assert_eq!(
            ac.rules[0].move_call_package_address,
            Some(ValueIotaAddress::List(vec![IotaAddress::new([2; 32])]))
        );
        assert_eq!(ac.rules[0].action, Action::Allow);
    }

    #[test]
    fn serialize_access_controller_with_detailed_hook_action() {
        let ac = AccessController::new(
            AccessPolicy::DenyAll,
            [AccessRuleBuilder::new()
                .hook(
                    Url::parse("http://example.org/").unwrap(),
                    Some({
                        let mut headers = BTreeMap::new();
                        headers.insert("foo".to_string(), vec!["bar".to_string()]);
                        headers
                    }),
                )
                .build()],
        );

        let yaml = serde_yaml::to_string(&ac).unwrap();

        assert_eq!(
            yaml,
            indoc! {r#"
              access-policy: deny-all
              rules:
              - sender-address: '*'
                action:
                  url: http://example.org/
                  headers:
                    foo:
                    - bar
            "#}
        );
    }

    #[test]
    fn deserialize_access_controller_with_detailed_hook_action() {
        let yaml = indoc! {r#"
          access-policy: deny-all
          rules:
          - action:
              url: http://127.0.0.1:8080
              headers:
                foo:
                - bar
        "#};

        let ac: AccessController = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(ac.access_policy, AccessPolicy::DenyAll);
        assert_eq!(ac.rules.len(), 1);
        assert_eq!(ac.rules[0].sender_address, ValueIotaAddress::All,);
        assert_eq!(
            ac.rules[0].action,
            Action::HookAction(HookAction::HookActionDetailed(
                HookActionDetailed::new(Url::parse("http://127.0.0.1:8080/").unwrap())
                    .with_headers({
                        let mut headers = HookActionHeaders::new();
                        headers.insert("foo".to_string(), vec!["bar".to_string()]);
                        headers
                    })
            ))
        );
    }

    #[tokio::test]
    async fn test_evaluation_order_multiple_rules_policy_deny() {
        let sender_address = IotaAddress::new([1; 32]);
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .deny()
            .build();

        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .allow()
            .build();

        let tx = TransactionContext::default().with_sender_address(sender_address);
        let ac = AccessController::new(AccessPolicy::DenyAll, [deny_rule, allow_rule]);

        // Even if the second rule allows the transaction, the first rule should deny it.
        let result = ac.check_access(&tx).await;
        assert!(matches!(result, Ok(Decision::Deny)));
    }

    #[tokio::test]
    async fn test_evaluation_order_multiple_rules_policy_allow() {
        let sender_address = IotaAddress::new([1; 32]);

        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .deny()
            .build();
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .allow()
            .build();

        let tx = TransactionContext::default().with_sender_address(sender_address);
        let ac = AccessController::new(AccessPolicy::AllowAll, [allow_rule, deny_rule]);

        // Even if the second rule denied the transaction, the first rule should allow it.
        assert!(matches!(ac.check_access(&tx).await, Ok(Decision::Allow)));
    }

    #[tokio::test]
    async fn test_evaluation_logic_matching() {
        let sender_1 = IotaAddress::new([1; 32]);
        let sender_2 = IotaAddress::new([2; 32]);
        let package_id = IotaAddress::new([10; 32]);

        let allow_sender_1_and_package = AccessRuleBuilder::new()
            .sender_address(sender_1)
            .move_call_package_address(package_id)
            .allow()
            .build();

        let deny_sender_1 = AccessRuleBuilder::new()
            .sender_address(sender_1)
            .deny()
            .build();

        let tx_sender_1_accepted = TransactionContext::default()
            .with_sender_address(sender_1)
            .with_move_call_package_addresses(vec![package_id]);
        let tx_sender_1_rejected = TransactionContext::default().with_sender_address(sender_1);
        let tx_sender_2_accepted = TransactionContext::default().with_sender_address(sender_2);

        let ac = AccessController::new(
            AccessPolicy::AllowAll,
            [allow_sender_1_and_package, deny_sender_1],
        );

        // accepted because of rule 1
        assert!(matches!(
            ac.check_access(&tx_sender_1_accepted).await,
            Ok(Decision::Allow)
        ));
        // rejected because of rule 2
        assert!(matches!(
            ac.check_access(&tx_sender_1_rejected).await,
            Ok(Decision::Deny)
        ));
        // accepted because of default policy
        assert!(matches!(
            ac.check_access(&tx_sender_2_accepted).await,
            Ok(Decision::Allow)
        ));
    }

    mod hook {
        use axum::http::{HeaderMap, HeaderValue};
        use url::Url;

        use crate::access_controller::hook::{
            ExecuteTxOkResponse, SkippableDecision, TEST_ERROR_HEADER, TEST_RESPONSE_HEADER,
        };

        use super::*;

        fn get_headers_with_test_response(
            decision: SkippableDecision,
            user_message: Option<String>,
        ) -> HeaderMap {
            let response = ExecuteTxOkResponse {
                decision,
                user_message,
            };
            let mut headers = HeaderMap::new();
            headers.insert(
                TEST_RESPONSE_HEADER,
                HeaderValue::from_str(&serde_json::to_string(&response).unwrap()).unwrap(),
            );

            headers
        }

        #[tokio::test]
        async fn test_hook_can_allow_tx() {
            let hook_rule = AccessRuleBuilder::new()
                .hook(Url::parse("https://example.net").unwrap(), None)
                .build();
            // "calling" the test hook action with this context will return `SkippableDecision::Allow`
            let allow_ctx = TransactionContext::default().with_headers(
                get_headers_with_test_response(SkippableDecision::Allow, None),
            );

            let ac_allow_all = AccessController::new(AccessPolicy::AllowAll, [hook_rule.clone()]);
            let ac_deny_all = AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone()]);

            assert!(matches!(
                ac_allow_all.check_access(&allow_ctx).await,
                Ok(Decision::Allow)
            ));
            assert!(matches!(
                ac_deny_all.check_access(&allow_ctx).await,
                Ok(Decision::Allow)
            ));
        }

        #[tokio::test]
        async fn test_hook_can_deny_tx() {
            let hook_rule = AccessRuleBuilder::new()
                .hook(Url::parse("https://example.net").unwrap(), None)
                .build();
            // "calling" the test hook action with this context will return `SkippableDecision::Deny`
            let deny_ctx = TransactionContext::default().with_headers(
                get_headers_with_test_response(SkippableDecision::Deny, None),
            );

            let ac_allow_all = AccessController::new(AccessPolicy::AllowAll, [hook_rule.clone()]);
            let ac_deny_all = AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone()]);

            assert!(matches!(
                ac_allow_all.check_access(&deny_ctx).await,
                Ok(Decision::Deny)
            ));
            assert!(matches!(
                ac_deny_all.check_access(&deny_ctx).await,
                Ok(Decision::Deny)
            ));
        }

        #[tokio::test]
        async fn test_hook_can_forward_decision_to_next_rule() {
            let hook_rule = AccessRuleBuilder::new()
                .hook(Url::parse("https://example.net").unwrap(), None)
                .build();
            let allow_rule = AccessRuleBuilder::new().allow().build();
            let deny_rule = AccessRuleBuilder::new().deny().build();
            // "calling" the test hook action with this context will return `SkippableDecision::NoDecision`
            let no_decision_ctx = TransactionContext::default().with_headers(
                get_headers_with_test_response(SkippableDecision::NoDecision, None),
            );

            let ac_allow_by_second_rule =
                AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone(), allow_rule]);
            let ac_deny_by_second_rule =
                AccessController::new(AccessPolicy::AllowAll, [hook_rule.clone(), deny_rule]);

            assert!(matches!(
                ac_allow_by_second_rule.check_access(&no_decision_ctx).await,
                Ok(Decision::Allow)
            ));
            assert!(matches!(
                ac_deny_by_second_rule.check_access(&no_decision_ctx).await,
                Ok(Decision::Deny)
            ));
        }

        #[tokio::test]
        async fn test_hook_can_forward_own_error_messages() {
            let hook_rule = AccessRuleBuilder::new()
                .hook(Url::parse("https://example.net").unwrap(), None)
                .build();
            // "calling" the test hook action with this context will return an error
            let error_ctx = TransactionContext::default().with_headers({
                let mut headers = HeaderMap::new();
                headers.insert(
                    TEST_ERROR_HEADER,
                    HeaderValue::from_str("error sent back from hook server").unwrap(),
                );
                headers
            });

            let ac_error = AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone()]);
            let result = ac_error.check_access(&error_ctx).await;

            assert!(matches!(result, Err(_)));
            assert_eq!(
                result.unwrap_err().to_string(),
                "hook call failed with status 400 Bad Request; error sent back from hook server"
                    .to_string()
            );
        }

        #[tokio::test]
        async fn test_hook_is_not_called_if_a_previous_rule_applies() {
            let hook_rule = AccessRuleBuilder::new()
                .hook(Url::parse("https://example.net").unwrap(), None)
                .build();
            let allow_rule = AccessRuleBuilder::new().allow().build();
            let deny_rule = AccessRuleBuilder::new().deny().build();
            // "calling" the test hook action with this context will return an error
            // this should be skipped as the first rule in the list should already apply
            let error_ctx = TransactionContext::default().with_headers({
                let mut headers = HeaderMap::new();
                headers.insert(
                    TEST_ERROR_HEADER,
                    HeaderValue::from_str("this error should not be thrown").unwrap(),
                );
                headers
            });

            let ac_allow_by_first_rule =
                AccessController::new(AccessPolicy::DenyAll, [allow_rule, hook_rule.clone()]);
            let ac_deny_by_first_rule =
                AccessController::new(AccessPolicy::AllowAll, [deny_rule, hook_rule.clone()]);

            assert!(matches!(
                ac_allow_by_first_rule.check_access(&error_ctx).await,
                Ok(Decision::Allow)
            ));
            assert!(matches!(
                ac_deny_by_first_rule.check_access(&error_ctx).await,
                Ok(Decision::Deny)
            ));
        }

        #[tokio::test]
        async fn test_hook_is_called_if_another_rule_term_applies() {
            let sender_address = IotaAddress::new([1; 32]);
            let hook_rule = AccessRuleBuilder::new()
                .sender_address(sender_address)
                .hook(Url::parse("https://example.net").unwrap(), None)
                .build();
            // "calling" the test hook action with this context will return an error
            // this should be skipped as the first term in the rule should already stop rule evaluation
            let sender_address_hook_error_ctx = TransactionContext::default()
                .with_sender_address(sender_address)
                .with_headers({
                    let mut headers = HeaderMap::new();
                    headers.insert(
                        TEST_ERROR_HEADER,
                        HeaderValue::from_str("sender matched, so we'll see this error").unwrap(),
                    );
                    headers
                });

            let ac_blocked_for_sender =
                AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone()]);
            let result = ac_blocked_for_sender
                .check_access(&sender_address_hook_error_ctx)
                .await;

            assert!(matches!(result, Err(_)));
            assert_eq!(
                result.unwrap_err().to_string(),
                "hook call failed with status 400 Bad Request; sender matched, so we'll see this error".to_string()
            );
        }

        #[tokio::test]
        async fn test_hook_is_not_called_if_another_rule_term_does_not_apply() {
            let sender_address = IotaAddress::new([1; 32]);
            let blocked_address = IotaAddress::new([2; 32]);
            let hook_rule = AccessRuleBuilder::new()
                .sender_address(sender_address)
                .hook(Url::parse("https://example.net").unwrap(), None)
                .build();
            // "calling" the test hook action with this context will return an error
            // this should be skipped as the first term in the rule should already stop rule evaluation
            let blocked_address_hook_error_ctx = TransactionContext::default()
                .with_sender_address(blocked_address)
                .with_headers({
                    let mut headers = HeaderMap::new();
                    headers.insert(
                        TEST_ERROR_HEADER,
                        HeaderValue::from_str("this error should not be thrown").unwrap(),
                    );
                    headers
                });

            let ac_blocked_for_sender =
                AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone()]);

            assert!(matches!(
                ac_blocked_for_sender
                    .check_access(&blocked_address_hook_error_ctx)
                    .await,
                Ok(Decision::Deny)
            ));
        }
    }
}
