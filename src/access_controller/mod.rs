// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! This module implements the access controller for the gas station.
//! It provides a way to control the constraints for executing transactions, ensuring that only authorized addresses can perform specific actions.

pub mod decision;
pub mod policy;
pub mod predicates;
pub mod rule;

use anyhow::{anyhow, Result};
use decision::Decision;
use policy::AccessPolicy;
use rule::{AccessRule, TransactionDescription};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct AccessController {
    access_policy: AccessPolicy,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    rules: Vec<AccessRule>,
}

impl AccessController {
    /// Creates a new instance of the access controller.
    pub fn new(access_policy: AccessPolicy, rules: impl IntoIterator<Item = AccessRule>) -> Self {
        Self {
            access_policy,
            rules: rules.into_iter().collect(),
        }
    }

    /// Checks if the transaction can be executed based on the access controller's rules.
    // If a rule matches, the corresponding action is applied. If no rule matches, the next rule is checked.
    // If none match, the default policy is applied.
    pub fn check_access(&self, transaction_description: &TransactionDescription) -> Result<()> {
        if self.is_disabled() {
            return Ok(());
        }

        for (i, rule) in self.rules.iter().enumerate() {
            if rule.matches(&transaction_description) {
                return match rule.check_access(self.access_policy, &transaction_description) {
                    Decision::Allow => Ok(()),
                    Decision::Deny => Err(anyhow!("Access denied by rule #{}", i + 1)),
                };
            }
        }

        if self.access_policy == AccessPolicy::AllowAll {
            Ok(())
        } else {
            Err(anyhow!("Access denied by policy"))
        }
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
}

#[cfg(test)]
mod test {
    use iota_types::base_types::IotaAddress;

    use crate::access_controller::{
        predicates::{Action, ValueIotaAddress},
        AccessController,
    };

    use super::{
        policy::AccessPolicy,
        predicates::ValueNumber,
        rule::{AccessRuleBuilder, TransactionDescription},
    };

    #[test]
    fn test_deny_policy_rules_should_allow() {
        let sender_address = IotaAddress::new([1; 32]);
        let blocked_address = IotaAddress::new([2; 32]);
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .allow()
            .build();
        let allowed_tx = TransactionDescription {
            sender_address: sender_address.clone(),
            ..Default::default()
        };
        let blocked_tx = TransactionDescription {
            sender_address: blocked_address.clone(),
            ..Default::default()
        };

        let mut ac = AccessController::new(AccessPolicy::DenyAll, []);

        assert!(ac.check_access(&allowed_tx).is_err());
        assert!(ac.check_access(&blocked_tx).is_err());

        ac.add_rule(allow_rule);

        assert!(ac.check_access(&allowed_tx).is_ok());
        assert!(ac.check_access(&blocked_tx).is_err());
    }

    #[test]
    fn test_allow_policy_rules_should_block() {
        let blocked_address = IotaAddress::new([1; 32]);
        let sender_address = IotaAddress::new([2; 32]);

        let deny_rule = AccessRuleBuilder::new()
            .sender_address(blocked_address)
            .deny()
            .build();

        let blocked_transaction_description = TransactionDescription {
            sender_address: blocked_address.clone(),
            ..Default::default()
        };
        let allowed_transaction_description = TransactionDescription {
            sender_address: sender_address.clone(),
            ..Default::default()
        };
        let mut ac = AccessController::new(AccessPolicy::AllowAll, []);

        assert!(ac.check_access(&allowed_transaction_description).is_ok());
        assert!(ac.check_access(&blocked_transaction_description).is_ok());

        ac.add_rule(deny_rule);

        assert!(ac.check_access(&allowed_transaction_description).is_ok());
        assert!(ac.check_access(&blocked_transaction_description).is_err());
    }

    #[test]
    fn test_deny_policy_rules_gas_budget() {
        let sender_address = IotaAddress::new([1; 32]);
        let gas_budget = 100;
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .gas_budget(ValueNumber::LessThan(gas_budget))
            .allow()
            .build();
        let allowed_tx = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_budget - 1);
        let blocked_tx = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_budget);

        let ac = AccessController::new(AccessPolicy::DenyAll, [allow_rule]);

        assert!(ac.check_access(&allowed_tx).is_ok());
        assert!(ac.check_access(&blocked_tx).is_err());
    }

    #[test]
    fn test_allow_policy_rules_gas_budget() {
        let sender_address = IotaAddress::new([1; 32]);
        let gas_budget = 100;
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .gas_budget(ValueNumber::GreaterThanOrEqual(gas_budget))
            .deny()
            .build();
        let allowed_tx = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_budget - 1);
        let blocked_tx = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_budget);

        let ac = AccessController::new(AccessPolicy::AllowAll, [deny_rule]);
        assert!(ac.check_access(&allowed_tx).is_ok());
        assert!(ac.check_access(&blocked_tx).is_err());
    }

    #[test]
    fn test_allow_policy_rules_move_call_package_address() {
        let sender_address = IotaAddress::new([1; 32]);
        let package_address = IotaAddress::new([2; 32]);
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .move_call_package_address(package_address)
            .deny()
            .build();
        let denied_tx = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![package_address]);
        let allowed_tx = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![IotaAddress::new([3; 32])]);

        let ac = AccessController::new(AccessPolicy::AllowAll, [deny_rule]);
        assert!(ac.check_access(&allowed_tx).is_ok());
        assert!(ac.check_access(&denied_tx).is_err());
    }

    #[test]
    fn test_deny_policy_rules_move_call_package_address() {
        let sender_address = IotaAddress::new([1; 32]);
        let package_address = IotaAddress::new([2; 32]);
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .move_call_package_address(package_address)
            .allow()
            .build();
        let allowed_tx = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![package_address]);
        let blocked_tx = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![IotaAddress::new([3; 32])]);

        let ac = AccessController::new(AccessPolicy::DenyAll, [allow_rule]);
        assert!(ac.check_access(&allowed_tx).is_ok());
        assert!(ac.check_access(&blocked_tx).is_err());
    }

    #[test]
    fn deserialize_access_controller() {
        let yaml = r#"
access-policy: "deny-all"
rules:
      - sender-address: ['0x0101010101010101010101010101010101010101010101010101010101010101']
        transaction-gas-budget: <=10000
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
        assert_eq!(ac.rules[0].action, Action::Allow);
    }

    #[test]
    fn serialize_access_controller() {
        let ac = AccessController::new(
            AccessPolicy::DenyAll,
            [AccessRuleBuilder::new()
                .sender_address(IotaAddress::new([1; 32]))
                .gas_budget(ValueNumber::LessThanOrEqual(10000))
                .allow()
                .build()],
        );
        let yaml = serde_yaml::to_string(&ac).unwrap();
        println!("{}", yaml);

        assert_eq!(
            yaml,
            r#"access-policy: deny-all
rules:
- sender-address: 0x0101010101010101010101010101010101010101010101010101010101010101
  transaction-gas-budget: <=10000
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
            r#"access-policy: deny-all
rules:
- sender-address: 0x0101010101010101010101010101010101010101010101010101010101010101
  move-call-package-address: 0x0202020202020202020202020202020202020202020202020202020202020202
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
    fn test_evaluation_order_multiple_rules_policy_deny() {
        let sender_address = IotaAddress::new([1; 32]);
        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .deny()
            .build();

        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .allow()
            .build();

        let tx = TransactionDescription::default().with_sender_address(sender_address);
        let ac = AccessController::new(AccessPolicy::DenyAll, [deny_rule, allow_rule]);

        // Even the second rule allows the transaction, the first rule should deny it.
        let result = ac.check_access(&tx);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Access denied by rule #1");
    }

    #[test]
    fn test_evaluation_order_multiple_rules_policy_allow() {
        let sender_address = IotaAddress::new([1; 32]);

        let deny_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .deny()
            .build();
        let allow_rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .allow()
            .build();

        let tx = TransactionDescription::default().with_sender_address(sender_address);
        let ac = AccessController::new(AccessPolicy::AllowAll, [allow_rule, deny_rule]);

        // Even the second rule denied the transaction, the first rule should allow it.
        assert!(ac.check_access(&tx).is_ok());
    }

    #[test]
    fn test_evaluation_logic_matching() {
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

        let tx_sender_1_accepted = TransactionDescription::default()
            .with_sender_address(sender_1)
            .with_move_call_package_addresses(vec![package_id]);
        let tx_sender_1_rejected = TransactionDescription::default().with_sender_address(sender_1);
        let tx_sender_2_accepted = TransactionDescription::default().with_sender_address(sender_2);

        let ac = AccessController::new(
            AccessPolicy::AllowAll,
            [allow_sender_1_and_package, deny_sender_1],
        );

        // accepted because of rule 1
        assert!(ac.check_access(&tx_sender_1_accepted).is_ok());
        // rejected because of rule 2
        assert!(ac.check_access(&tx_sender_1_rejected).is_err());
        // accepted because of default policy
        assert!(ac.check_access(&tx_sender_2_accepted).is_ok());
    }
}
