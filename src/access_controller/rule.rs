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

    pub fn denied(mut self) -> Self {
        self.rule.action = Action::Deny;
        self
    }

    pub fn gas_budget(mut self, gas_size: ValueNumber) -> Self {
        self.rule.transaction_gas_budget = Some(gas_size);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
#[skip_serializing_none]
pub struct AccessRule {
    pub sender_address: ValueIotaAddress,
    pub transaction_gas_budget: Option<ValueNumber>,

    pub action: Action,
}

impl AccessRule {
    /// Checks if the transaction can be executed based on the access rule and the access policy.
    pub fn check_access(
        &self,
        access_policy: AccessPolicy,
        data: &TransactionDescription,
    ) -> Decision {
        if self.rule_matches(data) {
            return self.evaluate_access_action(access_policy);
        }

        return access_policy.into();
    }

    /// Checks if the rule matches the transaction data.
    pub fn rule_matches(&self, data: &TransactionDescription) -> bool {
        self.sender_address.includes(&data.sender_address)
            && self
                .transaction_gas_budget
                .map(|size| size.matches(data.transaction_budget))
                // If the gas size is not defined then the rule matches
                .unwrap_or(true)
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
}

impl TransactionDescription {
    pub fn new(_signature: &GenericSignature, transaction_data: &TransactionData) -> Self {
        Self {
            sender_address: transaction_data.sender().clone(),
            transaction_budget: transaction_data.gas_budget(),
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
        let data_with_valid_sender =
            TransactionDescription::default().with_sender_address(sender_address);
        let data_with_invalid_sender = TransactionDescription::default();

        assert!(
            rule.check_access(AccessPolicy::AllowAll, &data_with_valid_sender) == Decision::Deny
        );
        assert!(
            rule.check_access(AccessPolicy::DenyAll, &data_with_valid_sender) == Decision::Deny
        );
        assert!(
            rule.check_access(AccessPolicy::DenyAll, &data_with_invalid_sender) == Decision::Deny
        );
        assert!(
            rule.check_access(AccessPolicy::DenyAll, &data_with_invalid_sender) == Decision::Deny
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
            .denied()
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
