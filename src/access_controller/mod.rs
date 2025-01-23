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
    pub fn check_access(&self, transaction_description: &TransactionDescription) -> Result<()> {
        if self.is_disabled() {
            return Ok(());
        }

        // In case of allow_all, we looking for the first rule that deny the access
        if self.access_policy == AccessPolicy::AllowAll {
            for (i, rule) in self.rules.iter().enumerate() {
                if rule.check_access(self.access_policy, &transaction_description) == Decision::Deny
                {
                    return Err(anyhow!("Access denied by rule {}", i));
                }
            }
            return Ok(());
        }
        // In case of deny_all, we looking for the first rule that allow the access
        if self.access_policy == AccessPolicy::DenyAll {
            for rule in &self.rules {
                if rule.check_access(self.access_policy, &transaction_description)
                    == Decision::Allow
                {
                    return Ok(());
                }
            }
            return Err(anyhow!("Access denied by policy"));
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
            .denied()
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
            .denied()
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
}
