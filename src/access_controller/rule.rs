// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use iota_types::{
    base_types::IotaAddress,
    signature::GenericSignature,
    transaction::{TransactionData, TransactionDataAPI},
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use super::{
    decision::Decision,
    predicates::{Action, ValueIotaAddress, ValueNumber},
    AccessPolicy,
};

/// The AccessRuleBuilder is used to build an AccessRule with fluent API.
pub struct AccessRuleBuilder {
    rule: AccessRule,
}

impl AccessRuleBuilder {
    pub fn new() -> Self {
        Self {
            rule: AccessRule::default(),
        }
    }

    pub fn build(self) -> AccessRule {
        self.rule
    }

    pub fn sender_address(mut self, sender_address: impl Into<IotaAddress>) -> Self {
        let iota_address = sender_address.into();
        match &mut self.rule.sender_address {
            ValueIotaAddress::All => {
                self.rule.sender_address = ValueIotaAddress::Single(iota_address);
            }
            ValueIotaAddress::Single(_) => {
                self.rule.sender_address = ValueIotaAddress::List(vec![iota_address]);
            }
            ValueIotaAddress::List(list) => {
                list.push(iota_address);
            }
        }
        self
    }

    /// Sets the action of the AccessRule to allow.
    pub fn allow(mut self) -> Self {
        self.rule.action = Action::Allow;
        self
    }

    pub fn deny(mut self) -> Self {
        self.rule.action = Action::Deny;
        self
    }

    pub fn gas_budget(mut self, gas_size: ValueNumber) -> Self {
        self.rule.transaction_gas_budget = Some(gas_size);
        self
    }

    pub fn move_call_package_address(mut self, address: impl Into<IotaAddress>) -> Self {
        let iota_address = address.into();
        if let Some(address) = &mut self.rule.move_call_package_address {
            match address {
                ValueIotaAddress::All => {
                    *address = ValueIotaAddress::Single(iota_address);
                }
                ValueIotaAddress::Single(_) => {
                    *address = ValueIotaAddress::List(vec![iota_address]);
                }
                ValueIotaAddress::List(list) => {
                    list.push(iota_address);
                }
            }
        } else {
            self.rule.move_call_package_address = Some(ValueIotaAddress::Single(iota_address));
        }

        self
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct AccessRule {
    pub sender_address: ValueIotaAddress,
    pub transaction_gas_budget: Option<ValueNumber>,
    pub move_call_package_address: Option<ValueIotaAddress>,

    pub action: Action,
}

impl AccessRule {
    /// Checks if the transaction can be executed based on the access rule and the access policy.
    pub fn check_access(
        &self,
        access_policy: AccessPolicy,
        data: &TransactionDescription,
    ) -> Decision {
        if self.matches(data) {
            return self.evaluate_access_action(access_policy);
        }

        return access_policy.into();
    }

    /// Checks if the rule matches the transaction data.
    pub fn matches(&self, data: &TransactionDescription) -> bool {
        self.sender_address.includes(&data.sender_address)
            // Gas Budget
            && self
                .transaction_gas_budget
                .map(|size| size.matches(data.transaction_budget))
                // If the gas size is not defined then the rule matches
                .unwrap_or(true)
            // Move Call Package Address
            && self
                .move_call_package_address.as_ref().map(|address| address.includes_any(&data.move_call_package_addresses)).unwrap_or(true)
    }

    /// Evaluates the access action based on the access policy.
    pub fn evaluate_access_action(&self, _general_policy: AccessPolicy) -> Decision {
        match self.action {
            Action::Allow => {
                return Decision::Allow;
            }
            Action::Deny => {
                return Decision::Deny;
            }
        }
    }
}

// This input is used to check the access policy.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransactionDescription {
    pub sender_address: IotaAddress,
    pub transaction_budget: u64,
    pub move_call_package_addresses: Vec<IotaAddress>,
}

impl TransactionDescription {
    pub fn new(_signature: &GenericSignature, transaction_data: &TransactionData) -> Self {
        Self {
            sender_address: transaction_data.sender().clone(),
            transaction_budget: transaction_data.gas_budget(),
            move_call_package_addresses: get_move_call_package_addresses(transaction_data),
        }
    }

    pub fn with_sender_address(mut self, sender_address: IotaAddress) -> Self {
        self.sender_address = sender_address;
        self
    }

    pub fn with_gas_budget(mut self, transaction_budget: u64) -> Self {
        self.transaction_budget = transaction_budget;
        self
    }

    pub fn with_move_call_package_addresses(
        mut self,
        move_call_package_addresses: Vec<IotaAddress>,
    ) -> Self {
        self.move_call_package_addresses = move_call_package_addresses;
        self
    }
}

fn get_move_call_package_addresses(transaction_data: &TransactionData) -> Vec<IotaAddress> {
    let TransactionData::V1(data_v1) = transaction_data;
    data_v1
        .move_calls()
        .iter()
        .map(|call| IotaAddress::new(call.0.into_bytes()))
        .collect()
}

#[cfg(test)]
mod test {

    use iota_types::base_types::IotaAddress;

    use crate::access_controller::{
        policy::AccessPolicy,
        predicates::ValueNumber,
        rule::{AccessRule, AccessRuleBuilder, Action, Decision, TransactionDescription},
    };

    #[test]
    fn test_constraint_src_address_defined_and_allowed() {
        let sender_address = IotaAddress::new([1; 32]);
        let rule = super::AccessRule {
            sender_address: [sender_address].into(),
            action: Action::Allow,
            ..Default::default()
        };
        let data_with_allowed_sender =
            TransactionDescription::default().with_sender_address(sender_address);
        let data_with_denied_sender = TransactionDescription::default();

        assert!(
            rule.check_access(AccessPolicy::DenyAll, &data_with_allowed_sender) == Decision::Allow
        );
        assert!(
            rule.check_access(AccessPolicy::DenyAll, &data_with_denied_sender) == Decision::Deny
        );
        assert!(
            rule.check_access(AccessPolicy::AllowAll, &data_with_allowed_sender) == Decision::Allow
        );
        assert!(
            rule.check_access(AccessPolicy::AllowAll, &data_with_denied_sender) == Decision::Allow
        );
    }

    #[test]
    fn test_constraint_src_address_defined_and_denied() {
        let sender_address = IotaAddress::new([1; 32]);
        let rule = super::AccessRule {
            sender_address: [sender_address].into(),
            action: Action::Deny,
            ..Default::default()
        };
        let data_with_allowed_sender =
            TransactionDescription::default().with_sender_address(sender_address);
        let data_with_denied_sender = TransactionDescription::default();

        assert!(
            rule.check_access(AccessPolicy::AllowAll, &data_with_allowed_sender) == Decision::Deny
        );
        assert!(
            rule.check_access(AccessPolicy::DenyAll, &data_with_allowed_sender) == Decision::Deny
        );
        assert!(
            rule.check_access(AccessPolicy::DenyAll, &data_with_denied_sender) == Decision::Deny
        );
        assert!(
            rule.check_access(AccessPolicy::DenyAll, &data_with_denied_sender) == Decision::Deny
        );
    }

    #[test]
    fn test_constraint_gas_budget() {
        let gas_limit = 100;
        let rule_allow = AccessRuleBuilder::new()
            .gas_budget(ValueNumber::LessThanOrEqual(gas_limit))
            .allow()
            .build();

        let rule_deny = AccessRuleBuilder::new()
            .gas_budget(ValueNumber::GreaterThan(gas_limit))
            .deny()
            .build();

        let low_transaction_budget = TransactionDescription::default().with_gas_budget(50);
        let high_transaction_budget = TransactionDescription::default().with_gas_budget(200);

        assert_eq!(
            rule_deny.check_access(AccessPolicy::AllowAll, &low_transaction_budget),
            Decision::Allow
        );
        assert_eq!(
            rule_deny.check_access(AccessPolicy::AllowAll, &high_transaction_budget),
            Decision::Deny
        );

        // now when policy is deny
        assert_eq!(
            rule_deny.check_access(AccessPolicy::DenyAll, &low_transaction_budget),
            Decision::Deny
        );
        assert_eq!(
            rule_deny.check_access(AccessPolicy::DenyAll, &high_transaction_budget),
            Decision::Deny
        );

        assert_eq!(
            rule_allow.check_access(AccessPolicy::AllowAll, &low_transaction_budget),
            Decision::Allow
        );
        assert_eq!(
            rule_allow.check_access(AccessPolicy::AllowAll, &high_transaction_budget),
            Decision::Allow
        );
        assert_eq!(
            rule_allow.check_access(AccessPolicy::DenyAll, &low_transaction_budget),
            Decision::Allow
        );
        assert_eq!(
            rule_allow.check_access(AccessPolicy::DenyAll, &high_transaction_budget),
            Decision::Deny
        );
    }

    #[test]
    fn test_constraint_move_call_package_addr() {
        let move_call_package_address = IotaAddress::new([1; 32]);
        let rule_allow = AccessRuleBuilder::new()
            .move_call_package_address(move_call_package_address)
            .allow()
            .build();

        let rule_deny = AccessRuleBuilder::new()
            .move_call_package_address(move_call_package_address)
            .deny()
            .build();
        let transaction_description = TransactionDescription::default()
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert_eq!(
            rule_allow.check_access(AccessPolicy::AllowAll, &transaction_description),
            Decision::Allow
        );
        assert_eq!(
            rule_allow.check_access(AccessPolicy::DenyAll, &transaction_description),
            Decision::Allow
        );
        assert_eq!(
            rule_deny.check_access(AccessPolicy::AllowAll, &transaction_description),
            Decision::Deny
        );
        assert_eq!(
            rule_deny.check_access(AccessPolicy::DenyAll, &transaction_description),
            Decision::Deny
        );
    }

    #[test]
    fn test_constraint_mix_ups_sender_package_address() {
        let sender_address = IotaAddress::new([1; 32]);
        let move_call_package_address = IotaAddress::new([2; 32]);

        let rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .move_call_package_address(move_call_package_address)
            .allow()
            .build();

        let transaction_description = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert_eq!(
            rule.check_access(AccessPolicy::AllowAll, &transaction_description),
            Decision::Allow
        );
        assert_eq!(
            rule.check_access(AccessPolicy::DenyAll, &transaction_description),
            Decision::Allow
        );

        let transaction_description_with_not_matched_package_address =
            TransactionDescription::default()
                .with_sender_address(sender_address)
                .with_move_call_package_addresses(vec![IotaAddress::new([3; 32])]);

        assert_eq!(
            rule.check_access(
                AccessPolicy::AllowAll,
                &transaction_description_with_not_matched_package_address
            ),
            Decision::Allow
        );
        assert_eq!(
            rule.check_access(
                AccessPolicy::DenyAll,
                &transaction_description_with_not_matched_package_address
            ),
            Decision::Deny
        );
    }

    #[test]
    fn test_constraint_mix_ups_sender_budget_package_address() {
        let sender_address = IotaAddress::new([1; 32]);
        let move_call_package_address = IotaAddress::new([2; 32]);
        let gas_limit = 100;

        let rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .move_call_package_address(move_call_package_address)
            .gas_budget(ValueNumber::LessThanOrEqual(gas_limit))
            .allow()
            .build();

        let transaction_description = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_limit)
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert_eq!(
            rule.check_access(AccessPolicy::AllowAll, &transaction_description),
            Decision::Allow
        );
        assert_eq!(
            rule.check_access(AccessPolicy::DenyAll, &transaction_description),
            Decision::Allow
        );

        let transaction_description_with_not_matched_package_address =
            TransactionDescription::default()
                .with_sender_address(sender_address)
                .with_gas_budget(gas_limit)
                .with_move_call_package_addresses(vec![IotaAddress::new([3; 32])]);

        assert_eq!(
            rule.check_access(
                AccessPolicy::AllowAll,
                &transaction_description_with_not_matched_package_address
            ),
            Decision::Allow
        );
        assert_eq!(
            rule.check_access(
                AccessPolicy::DenyAll,
                &transaction_description_with_not_matched_package_address
            ),
            Decision::Deny
        );

        let transaction_description_with_not_matched_gas_limit = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_limit + 1)
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert_eq!(
            rule.check_access(
                AccessPolicy::AllowAll,
                &transaction_description_with_not_matched_gas_limit
            ),
            Decision::Allow
        );
        assert_eq!(
            rule.check_access(
                AccessPolicy::DenyAll,
                &transaction_description_with_not_matched_gas_limit
            ),
            Decision::Deny
        );
    }

    #[test]

    fn test_allow_when_deny_all() {
        let policy = super::AccessPolicy::DenyAll;
        let sender_address = IotaAddress::new([0; 32]);
        let input = TransactionDescription::default().with_sender_address(sender_address);
        let access_rule = AccessRule {
            sender_address: [sender_address].into(),
            action: Action::Allow,
            ..Default::default()
        };

        assert_eq!(
            access_rule.check_access(policy, &input),
            super::Decision::Allow
        );
    }
}
