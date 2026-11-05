// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! This module implements the access controller for the gas station.
//! It provides a way to control the constraints for executing transactions, ensuring that only authorized addresses can perform specific actions.

pub mod decision;
pub mod hook;
pub mod policy;
pub mod predicates;
pub(crate) mod rego_input;
pub mod reports;
pub mod rule;
pub mod utils;

use std::{collections::HashMap, fmt::Formatter, sync::Arc};

use anyhow::{Context, Result};
use decision::Decision;
use hook::SkippableDecision;
use iota_sdk_types::TransactionDigest;
use policy::AccessPolicy;
use predicates::Action;
use rule::{AccessRule, GasUsageConfirmationRequest, TransactionContext};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, trace};

use crate::access_controller::{reports::AccessReport, rule::RequestId};

/// Represents a single `execute_tx` request, containing the transaction under
/// evaluation and the unique identifier used to track its gas-usage charges
/// in the confirmation ledger.
pub struct RequestContext {
    pub transaction: TransactionContext,
    pub request_id: RequestId,
}

impl RequestContext {
    pub fn new(transaction: TransactionContext) -> Self {
        Self {
            transaction,
            request_id: RequestId::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct AccessController {
    pub access_policy: AccessPolicy,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub rules: Vec<AccessRule>,

    #[serde(skip)]
    /// What each request charged, keyed by `RequestContext::request_id`, so
    /// that `confirm_transaction` settles its own request with its own result.
    confirmation_requests: Arc<Mutex<HashMap<RequestId, Vec<GasUsageConfirmationRequest>>>>,
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
    pub async fn check_access(&self, request: &RequestContext) -> Result<Decision> {
        if self.is_disabled() {
            return Ok(Decision::Allow);
        }
        let ctx = &request.transaction;

        let mut access_report = AccessReport::new();
        access_report.set_transaction_data(ctx.transaction_data.clone());
        for rule in self.rules.iter() {
            let mut maybe_decision: Option<Decision> = None;
            let mut rule_report = rule.matches(ctx).await?;
            let is_partially_matched = rule_report.is_matched;

            if is_partially_matched {
                // Validate the counters if the rule partially matches
                let matching_result = rule.match_global_limits(ctx).await?;
                if !matching_result.1.is_empty() {
                    self.confirmation_requests
                        .lock()
                        .await
                        .entry(request.request_id)
                        .or_default()
                        .extend(matching_result.1);
                }
                // add the the predicate reports from global limits to get the "full" report for the rule
                rule_report.add_predicate_reports(matching_result.0.predicate_reports);

                if matching_result.0.is_matched {
                    rule_report.set_applied_action(Some(rule.action.clone()));

                    maybe_decision = match &rule.action {
                        Action::Allow => Some(Decision::Allow),
                        Action::Deny => Some(Decision::Deny),
                        Action::HookAction(hook_action) => {
                            // call hook and take defined result or continue with next rule
                            let response = hook_action.call_hook(ctx).await?;
                            debug!("Called hook: {}, for transaction with digest: {}. Got decision: {:?}, with user message: {:?}",
                                    hook_action.url(),
                                    ctx.transaction_digest,
                                    response.decision,
                                    response.user_message,
                                );
                            if let Some(user_message) = response.user_message {
                                rule_report.set_applied_action_details(user_message);
                            }
                            match response.decision {
                                SkippableDecision::Allow => Some(Decision::Allow),
                                SkippableDecision::Deny => Some(Decision::Deny),
                                SkippableDecision::NoDecision => None,
                            }
                        }
                    };
                }
            }

            // regardless of matching result, add the rule report to the access report
            access_report.add_rule(rule_report);

            // if a decision was made, set it in the access report and return it
            if let Some(decision) = maybe_decision {
                access_report.set_decision(decision.clone());
                trace!("{}", access_report);
                return Ok(decision);
            }
        }

        access_report.set_decision_with_reason(self.access_policy.into(), "Default policy applied");

        trace!("{}", access_report);
        Ok(self.access_policy.into())
    }

    pub async fn confirm_transaction(
        &self,
        request: &RequestContext,
        result: TransactionExecutionResult,
    ) -> Result<()> {
        let ctx = &request.transaction;
        let request_id = request.request_id;
        let transaction_digest = result.transaction_digest;
        let maybe_requests = self.confirmation_requests.lock().await.remove(&request_id);
        if let Some(requests) = maybe_requests {
            for req in requests {
                let diff = if let Some(real_gas_usage) = result.gas_usage {
                    let reserved_gas_usage = req.gas_usage;
                    let diff = reserved_gas_usage.saturating_sub(
                        i64::try_from(real_gas_usage)
                            .context("real gas usage does not fit in i64")?,
                    );
                    debug!("Transaction with id: {transaction_digest} (request {request_id:?}) confirmed, reserved gas usage: {reserved_gas_usage}, real gas usage: {real_gas_usage}, diff: {diff}");
                    diff
                } else {
                    debug!("Transaction with id: {transaction_digest} (request {request_id:?}) confirmed, but no gas usage was provided");
                    req.gas_usage
                };
                ctx.stats_tracker
                    .update_aggr(req.rule_meta, &req.aggregate, diff.saturating_neg())
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
    use std::{collections::BTreeMap, time::Duration};

    use indoc::indoc;
    use iota_sdk_types::{Address, TransactionDigest};
    use url::Url;

    use crate::{
        access_controller::{
            decision::Decision,
            hook::{HookAction, HookActionHeaders},
            predicates::{Action, CountBy, ValueAggregate, ValueIotaAddress},
            AccessController, TransactionExecutionResult,
        },
        test_env::{new_stats_tracker_for_testing, random_address},
        tracker::StatsTracker,
    };

    use super::{
        policy::AccessPolicy,
        predicates::ValueNumber,
        rule::{AccessRuleBuilder, TransactionContext},
        RequestContext,
    };

    #[tokio::test]
    async fn test_deny_policy_rules_should_allow() {
        let sender_address = Address::from_bytes([1; 32]).unwrap();
        let blocked_address = Address::from_bytes([2; 32]).unwrap();
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .allow()
            .build();
        let to_allow_tx = RequestContext::new(TransactionContext {
            sender_address,
            ..Default::default()
        });
        let denied_tx = RequestContext::new(TransactionContext {
            sender_address: blocked_address,
            ..Default::default()
        });

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
        let blocked_address = Address::from_bytes([1; 32]).unwrap();
        let sender_address = Address::from_bytes([2; 32]).unwrap();

        let deny_rule = AccessRuleBuilder::new()
            .sender_address(blocked_address)
            .deny()
            .build();

        let to_deny_tx = RequestContext::new(TransactionContext {
            sender_address: blocked_address,
            ..Default::default()
        });
        let allowed_tx = RequestContext::new(TransactionContext {
            sender_address,
            ..Default::default()
        });
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
        let sender_address = Address::from_bytes([1; 32]).unwrap();
        let gas_budget = 100;
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .gas_budget(ValueNumber::LessThan(gas_budget))
            .allow()
            .build();
        let allowed_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_gas_budget(gas_budget - 1),
        );
        let denied_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_gas_budget(gas_budget),
        );

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
        let sender_address = Address::from_bytes([1; 32]).unwrap();
        let gas_budget = 100;
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .gas_budget(ValueNumber::GreaterThanOrEqual(gas_budget))
            .deny()
            .build();
        let allowed_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_gas_budget(gas_budget - 1),
        );
        let denied_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_gas_budget(gas_budget),
        );

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
        let sender_address = Address::from_bytes([1; 32]).unwrap();
        let package_address = Address::from_bytes([2; 32]).unwrap();
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .move_call_package_address(package_address)
            .deny()
            .build();
        let denied_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_move_call_package_addresses(vec![package_address]),
        );
        let allowed_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_move_call_package_addresses(vec![Address::from_bytes([3; 32]).unwrap()]),
        );

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
        let sender_address = Address::from_bytes([1; 32]).unwrap();
        let package_address = Address::from_bytes([2; 32]).unwrap();
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .move_call_package_address(package_address)
            .allow()
            .build();
        let allowed_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_move_call_package_addresses(vec![package_address]),
        );
        let denied_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_move_call_package_addresses(vec![Address::from_bytes([3; 32]).unwrap()]),
        );

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
        let sender_address = Address::from_bytes([1; 32]).unwrap();
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .ptb_command_count(ValueNumber::GreaterThan(1))
            .deny()
            .build();
        let denied_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_ptb_command_count(5),
        );
        let allowed_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_ptb_command_count(1),
        );

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
        let sender_address = Address::from_bytes([1; 32]).unwrap();
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .ptb_command_count(ValueNumber::LessThanOrEqual(1))
            .allow()
            .build();
        let allowed_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_ptb_command_count(1),
        );
        let blocked_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_ptb_command_count(5),
        );

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
        let yaml = indoc! {r#"
          access-policy: "deny-all"
          rules:
            - sender-address: ["0x0101010101010101010101010101010101010101010101010101010101010101"]
              transaction-gas-budget: "<=10000"
              ptb-command-count: "<=5"
              action: allow
        "#};
        let ac: AccessController = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ac.access_policy, AccessPolicy::DenyAll);
        assert_eq!(ac.rules.len(), 1);
        assert_eq!(
            ac.rules[0].sender_address,
            ValueIotaAddress::List(vec![Address::from_bytes([1; 32]).unwrap()])
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
                .sender_address(Address::from_bytes([1; 32]).unwrap())
                .gas_budget(ValueNumber::LessThanOrEqual(10000))
                .ptb_command_count(ValueNumber::LessThanOrEqual(5))
                .allow()
                .build()],
        );
        let yaml = serde_yaml::to_string(&ac).unwrap();
        println!("{}", yaml);

        assert_eq!(
            yaml,
            indoc! {r#"
              ---
              access-policy: deny-all
              rules:
                - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
                  transaction-gas-budget: "<=10000"
                  ptb-command-count: "<=5"
                  action: allow
            "#}
        );
    }

    #[test]
    fn serialize_access_controller_with_move_call_package_address() {
        let ac = AccessController::new(
            AccessPolicy::DenyAll,
            [AccessRuleBuilder::new()
                .sender_address(Address::from_bytes([1; 32]).unwrap())
                .move_call_package_address(Address::from_bytes([2; 32]).unwrap())
                .allow()
                .build()],
        );
        let yaml = serde_yaml::to_string(&ac).unwrap();

        assert_eq!(
            yaml,
            indoc! {r#"
              ---
              access-policy: deny-all
              rules:
                - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
                  move-call-package-address: "0x0202020202020202020202020202020202020202020202020202020202020202"
                  action: allow
            "#}
        );
    }

    #[test]
    fn deserialize_access_controller_with_move_call_package_address() {
        let yaml = indoc! {r#"
          ---
          access-policy: "deny-all"
          rules:
            - sender-address: ["0x0101010101010101010101010101010101010101010101010101010101010101"]
              move-call-package-address: ["0x0202020202020202020202020202020202020202020202020202020202020202"]
              action: allow
          "#};
        let ac: AccessController = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ac.access_policy, AccessPolicy::DenyAll);
        assert_eq!(ac.rules.len(), 1);
        assert_eq!(
            ac.rules[0].sender_address,
            ValueIotaAddress::List(vec![Address::from_bytes([1; 32]).unwrap()])
        );
        assert_eq!(
            ac.rules[0].move_call_package_address,
            Some(ValueIotaAddress::List(vec![
                Address::from_bytes([2; 32]).unwrap()
            ]))
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
                .unwrap()
                .build()],
        );

        let yaml = serde_yaml::to_string(&ac).unwrap();

        assert_eq!(
            yaml,
            indoc! {r###"
              ---
              access-policy: deny-all
              rules:
                - sender-address: "*"
                  action:
                    url: "http://example.org/"
                    headers:
                      foo:
                        - bar
            "###}
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
        assert_eq!(ac.rules[0].sender_address, ValueIotaAddress::All);
        let mut headers = HookActionHeaders::new();
        headers.insert("foo".to_string(), vec!["bar".to_string()]);
        assert_eq!(
            ac.rules[0].action,
            Action::HookAction(Box::new(
                HookAction::new_detailed(
                    Url::parse("http://127.0.0.1:8080/").unwrap(),
                    Some(headers)
                )
                .unwrap()
            ))
        );
    }

    #[tokio::test]
    async fn test_evaluation_order_multiple_rules_policy_deny() {
        let sender_address = Address::from_bytes([1; 32]).unwrap();
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .deny()
            .build();

        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .allow()
            .build();

        let tx =
            RequestContext::new(TransactionContext::default().with_sender_address(sender_address));
        let ac = AccessController::new(AccessPolicy::DenyAll, [deny_rule, allow_rule]);

        // Even if the second rule allows the transaction, the first rule should deny it.
        let result = ac.check_access(&tx).await;
        assert!(matches!(result, Ok(Decision::Deny)));
    }

    #[tokio::test]
    async fn test_evaluation_order_multiple_rules_policy_allow() {
        let sender_address = Address::from_bytes([1; 32]).unwrap();

        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .deny()
            .build();
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .allow()
            .build();

        let tx =
            RequestContext::new(TransactionContext::default().with_sender_address(sender_address));
        let ac = AccessController::new(AccessPolicy::AllowAll, [allow_rule, deny_rule]);

        // Even if the second rule denied the transaction, the first rule should allow it.
        assert!(matches!(ac.check_access(&tx).await, Ok(Decision::Allow)));
    }

    #[tokio::test]
    async fn test_evaluation_logic_matching() {
        let sender_1 = Address::from_bytes([1; 32]).unwrap();
        let sender_2 = Address::from_bytes([2; 32]).unwrap();
        let package_id = Address::from_bytes([10; 32]).unwrap();

        let allow_sender_1_and_package = AccessRuleBuilder::new()
            .sender_address(sender_1)
            .move_call_package_address(package_id)
            .allow()
            .build();

        let deny_sender_1 = AccessRuleBuilder::new()
            .sender_address(sender_1)
            .deny()
            .build();

        let tx_sender_1_accepted = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_1)
                .with_move_call_package_addresses(vec![package_id]),
        );
        let tx_sender_1_rejected =
            RequestContext::new(TransactionContext::default().with_sender_address(sender_1));
        let tx_sender_2_accepted =
            RequestContext::new(TransactionContext::default().with_sender_address(sender_2));

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

    /// Checks that a rejected request with the given `budget` leaves the
    /// sender's `gas-usage` counter at 0.
    async fn assert_gas_usage_counter_unaffected_by_budget(budget: u64) {
        let sponsor_address = random_address();
        let sender_address = random_address();
        let stats_tracker = new_stats_tracker_for_testing(sponsor_address).await;
        let rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .gas_limit(
                ValueAggregate::new(Duration::from_secs(60), ValueNumber::LessThanOrEqual(1000))
                    .with_count_by(vec![CountBy::SenderAddress]),
            )
            .allow()
            .build();
        let ac = AccessController::new(AccessPolicy::DenyAll, [rule]);

        let mut huge_budget_tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_gas_budget(budget)
                .with_stats_tracker(stats_tracker.clone()),
        );
        huge_budget_tx.transaction.transaction_digest = TransactionDigest::new([1; 32]);
        // The budget is far above the limit, so the request is not allowed.
        assert!(!matches!(
            ac.check_access(&huge_budget_tx).await,
            Ok(Decision::Allow)
        ));
        // Confirming without gas usage refunds the whole charged budget.
        let _ = ac
            .confirm_transaction(
                &huge_budget_tx,
                TransactionExecutionResult::new(huge_budget_tx.transaction.transaction_digest),
            )
            .await;

        let tx_500 = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_gas_budget(500)
                .with_stats_tracker(stats_tracker.clone()),
        );
        let decision = ac.check_access(&tx_500).await;
        assert!(
            matches!(decision, Ok(Decision::Allow)),
            "a budget of 500 should fit if the counter was left at 0, got {decision:?}"
        );
        let tx_600 = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender_address)
                .with_gas_budget(600)
                .with_stats_tracker(stats_tracker),
        );
        let decision = ac.check_access(&tx_600).await;
        assert!(
            matches!(decision, Ok(Decision::Deny)),
            "a budget of 600 should be denied since 500 + 600 > 1000, got {decision:?}"
        );
    }

    // The largest budget that fits in i64 is charged to the counter.
    #[tokio::test]
    async fn test_gas_usage_counter_unaffected_by_budget_i64_max() {
        assert_gas_usage_counter_unaffected_by_budget(i64::MAX as u64).await;
    }

    // A budget that does not fit in i64 is rejected before it is charged.
    #[tokio::test]
    async fn test_gas_usage_counter_unaffected_by_budget_above_i64_max() {
        assert_gas_usage_counter_unaffected_by_budget(i64::MAX as u64 + 1).await;
    }

    fn gas_limit_rule(sender: Address, limit: u64) -> AccessRuleBuilder {
        AccessRuleBuilder::new().sender_address(sender).gas_limit(
            ValueAggregate::new(Duration::from_secs(60), ValueNumber::LessThanOrEqual(limit))
                .with_count_by(vec![CountBy::SenderAddress]),
        )
    }

    fn tx(sender: Address, budget: u64, stats_tracker: &StatsTracker) -> RequestContext {
        let mut tx = RequestContext::new(
            TransactionContext::default()
                .with_sender_address(sender)
                .with_gas_budget(budget)
                .with_stats_tracker(stats_tracker.clone()),
        );
        tx.transaction.transaction_digest = TransactionDigest::new([budget as u8; 32]);
        tx
    }

    // A copy that fails before execution, for example because the other copy
    // already consumed the reservation, must release only its own charge.
    #[tokio::test]
    async fn test_gas_usage_counter_keeps_executed_copy_when_duplicate_fails_first() {
        let sender = random_address();
        let stats_tracker = new_stats_tracker_for_testing(random_address()).await;
        let ac = AccessController::new(
            AccessPolicy::DenyAll,
            [gas_limit_rule(sender, 1000).allow().build()],
        );

        let executed = tx(sender, 400, &stats_tracker);
        let failed = tx(sender, 400, &stats_tracker);
        for copy in [&executed, &failed] {
            assert!(matches!(ac.check_access(copy).await, Ok(Decision::Allow)));
        }

        // The failed copy is confirmed first, without gas usage.
        let result = TransactionExecutionResult::new(failed.transaction.transaction_digest);
        ac.confirm_transaction(&failed, result).await.unwrap();
        // The executed copy is confirmed with its real gas usage.
        let result = TransactionExecutionResult::new(executed.transaction.transaction_digest)
            .with_gas_usage(100);
        ac.confirm_transaction(&executed, result).await.unwrap();

        // Read the counter back through the limit of 1000. A budget of 900 is
        // allowed only if the counter is at most 100. It is never confirmed, so
        // counter = 100 + 900 = 1000, and the next budget of 1 must be denied.
        // Together the two checks pin the counter to exactly the 100 really used.
        let decision = ac.check_access(&tx(sender, 900, &stats_tracker)).await;
        assert!(matches!(decision, Ok(Decision::Allow)), "got {decision:?}");
        let decision = ac.check_access(&tx(sender, 1, &stats_tracker)).await;
        assert!(matches!(decision, Ok(Decision::Deny)), "got {decision:?}");
    }

    // Two throttles on one sender, both charge every request. After execution
    // both counters must be corrected to the real usage, not only the last one.
    #[tokio::test]
    async fn test_gas_usage_counter_refunds_every_throttle() {
        let sender = random_address();
        let stats_tracker = new_stats_tracker_for_testing(random_address()).await;
        let throttle = |limit, window| {
            AccessRuleBuilder::new()
                .sender_address(sender)
                .gas_limit(
                    ValueAggregate::new(window, ValueNumber::GreaterThan(limit))
                        .with_count_by(vec![CountBy::SenderAddress]),
                )
                .deny()
                .build()
        };
        let ac = AccessController::new(
            AccessPolicy::AllowAll,
            [
                throttle(100, Duration::from_secs(60)),
                throttle(1000, Duration::from_secs(3600)),
            ],
        );

        // A budget of 50 charges both counters, trips neither, and is allowed.
        let first = tx(sender, 50, &stats_tracker);
        assert!(matches!(ac.check_access(&first).await, Ok(Decision::Allow)));
        // It really used 10, so both counters must drop from 50 to 10.
        let result = TransactionExecutionResult::new(first.transaction.transaction_digest)
            .with_gas_usage(10);
        ac.confirm_transaction(&first, result).await.unwrap();

        // Read the 60s counter back through its limit of 100. A budget of 90 is
        // allowed only if the counter is at most 10. It is never confirmed, so
        // counter = 10 + 90 = 100, and the next budget of 1 must be denied.
        let decision = ac.check_access(&tx(sender, 90, &stats_tracker)).await;
        assert!(matches!(decision, Ok(Decision::Allow)), "got {decision:?}");
        let decision = ac.check_access(&tx(sender, 1, &stats_tracker)).await;
        assert!(matches!(decision, Ok(Decision::Deny)), "got {decision:?}");
    }

    mod hook {
        use url::Url;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        use crate::access_controller::hook::{ExecuteTxOkResponse, SkippableDecision};

        use super::*;

        async fn get_mock_server_ok(decision: &SkippableDecision) -> (MockServer, Url) {
            let endpoint = "/hello";
            let mock_server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(endpoint))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(ExecuteTxOkResponse {
                        decision: decision.clone(),
                        user_message: None,
                    }),
                )
                .mount(&mock_server)
                .await;
            let url = Url::parse(&format!("{}{}", &mock_server.uri(), endpoint)).unwrap();

            (mock_server, url)
        }

        async fn get_mock_server_bad_request() -> (MockServer, Url, &'static str) {
            let endpoint = "/hello";
            let error = "error sent back from hook server";
            let mock_server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(endpoint))
                .respond_with(ResponseTemplate::new(400).set_body_string(error))
                .mount(&mock_server)
                .await;
            let url = Url::parse(&format!("{}{}", &mock_server.uri(), endpoint)).unwrap();

            (mock_server, url, error)
        }

        #[tokio::test]
        async fn test_hook_can_allow_tx() {
            let decision = SkippableDecision::Allow;
            let (mock_server, url) = get_mock_server_ok(&decision).await;
            let hook_rule = AccessRuleBuilder::new().hook(url, None).unwrap().build();
            let ctx = RequestContext::new(TransactionContext::default());

            let ac_allow_all = AccessController::new(AccessPolicy::AllowAll, [hook_rule.clone()]);
            let ac_deny_all = AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone()]);

            assert!(matches!(
                ac_allow_all.check_access(&ctx).await,
                Ok(Decision::Allow)
            ));
            assert_eq!(mock_server.received_requests().await.unwrap().len(), 1);
            assert!(matches!(
                ac_deny_all.check_access(&ctx).await,
                Ok(Decision::Allow)
            ));
            assert_eq!(mock_server.received_requests().await.unwrap().len(), 2);
        }

        #[tokio::test]
        async fn test_hook_can_deny_tx() {
            let decision = SkippableDecision::Deny;
            let (mock_server, url) = get_mock_server_ok(&decision).await;
            let hook_rule = AccessRuleBuilder::new().hook(url, None).unwrap().build();
            let ctx = RequestContext::new(TransactionContext::default());

            let ac_allow_all = AccessController::new(AccessPolicy::AllowAll, [hook_rule.clone()]);
            let ac_deny_all = AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone()]);

            assert!(matches!(
                ac_allow_all.check_access(&ctx).await,
                Ok(Decision::Deny)
            ));
            assert_eq!(mock_server.received_requests().await.unwrap().len(), 1);
            assert!(matches!(
                ac_deny_all.check_access(&ctx).await,
                Ok(Decision::Deny)
            ));
        }

        #[tokio::test]
        async fn test_hook_can_forward_decision_to_next_rule() {
            let decision = SkippableDecision::NoDecision;
            let (mock_server, url) = get_mock_server_ok(&decision).await;
            let hook_rule = AccessRuleBuilder::new().hook(url, None).unwrap().build();
            let allow_rule = AccessRuleBuilder::new().allow().build();
            let deny_rule = AccessRuleBuilder::new().deny().build();
            let ctx = RequestContext::new(TransactionContext::default());

            let ac_allow_by_second_rule =
                AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone(), allow_rule]);
            let ac_deny_by_second_rule =
                AccessController::new(AccessPolicy::AllowAll, [hook_rule.clone(), deny_rule]);

            assert!(matches!(
                ac_allow_by_second_rule.check_access(&ctx).await,
                Ok(Decision::Allow)
            ));
            assert_eq!(mock_server.received_requests().await.unwrap().len(), 1);
            assert!(matches!(
                ac_deny_by_second_rule.check_access(&ctx).await,
                Ok(Decision::Deny)
            ));
            assert_eq!(mock_server.received_requests().await.unwrap().len(), 2);
        }

        #[tokio::test]
        async fn test_hook_can_forward_own_error_messages() {
            let (mock_server, url, error) = get_mock_server_bad_request().await;
            let hook_rule = AccessRuleBuilder::new().hook(url, None).unwrap().build();
            let ctx = RequestContext::new(TransactionContext::default());

            let ac_error = AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone()]);
            let result = ac_error.check_access(&ctx).await;

            assert!(result.is_err());
            assert_eq!(
                result.unwrap_err().to_string(),
                format!("hook call failed with status 400 Bad Request; {error}")
            );
            assert_eq!(mock_server.received_requests().await.unwrap().len(), 1);
        }

        #[tokio::test]
        async fn test_hook_is_not_called_if_a_previous_rule_applies() {
            let (mock_server, url, _) = get_mock_server_bad_request().await;
            let hook_rule = AccessRuleBuilder::new().hook(url, None).unwrap().build();
            let allow_rule = AccessRuleBuilder::new().allow().build();
            let deny_rule = AccessRuleBuilder::new().deny().build();
            let ctx = RequestContext::new(TransactionContext::default());

            let ac_allow_by_first_rule =
                AccessController::new(AccessPolicy::DenyAll, [allow_rule, hook_rule.clone()]);
            let ac_deny_by_first_rule =
                AccessController::new(AccessPolicy::AllowAll, [deny_rule, hook_rule.clone()]);

            assert!(matches!(
                ac_allow_by_first_rule.check_access(&ctx).await,
                Ok(Decision::Allow)
            ));
            assert_eq!(mock_server.received_requests().await.unwrap().len(), 0);
            assert!(matches!(
                ac_deny_by_first_rule.check_access(&ctx).await,
                Ok(Decision::Deny)
            ));
            assert_eq!(mock_server.received_requests().await.unwrap().len(), 0);
        }

        #[tokio::test]
        async fn test_hook_is_called_if_another_rule_term_applies() {
            let (mock_server, url, error) = get_mock_server_bad_request().await;
            let sender_address = Address::from_bytes([1; 32]).unwrap();
            let hook_rule = AccessRuleBuilder::new()
                .sender_address(sender_address)
                .hook(url, None)
                .unwrap()
                .build();
            let ctx = RequestContext::new(
                TransactionContext::default().with_sender_address(sender_address),
            );

            let ac_blocked_for_sender =
                AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone()]);
            let result = ac_blocked_for_sender.check_access(&ctx).await;

            assert!(result.is_err());
            assert_eq!(
                result.unwrap_err().to_string(),
                format!("hook call failed with status 400 Bad Request; {error}")
            );
            assert_eq!(mock_server.received_requests().await.unwrap().len(), 1);
        }

        #[tokio::test]
        async fn test_hook_is_not_called_if_another_rule_term_does_not_apply() {
            let (mock_server, url, _) = get_mock_server_bad_request().await;
            let sender_address = Address::from_bytes([1; 32]).unwrap();
            let blocked_address = Address::from_bytes([2; 32]).unwrap();
            let hook_rule = AccessRuleBuilder::new()
                .sender_address(sender_address)
                .hook(url, None)
                .unwrap()
                .build();
            let ctx = RequestContext::new(
                TransactionContext::default().with_sender_address(blocked_address),
            );

            let ac_blocked_for_sender =
                AccessController::new(AccessPolicy::DenyAll, [hook_rule.clone()]);

            assert!(matches!(
                ac_blocked_for_sender.check_access(&ctx).await,
                Ok(Decision::Deny)
            ));
            assert_eq!(mock_server.received_requests().await.unwrap().len(), 0);
        }
    }
}
