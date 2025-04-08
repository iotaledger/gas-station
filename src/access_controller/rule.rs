// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::any;

use anyhow::{bail, Context};
use iota_types::{
    base_types::IotaAddress,
    signature::GenericSignature,
    transaction::{TransactionData, TransactionDataAPI, TransactionDataV1, TransactionKind},
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::tracker::{
    tracker_storage::{redis::RedisTrackerStorage, Aggregate, AggregateType},
    StatsTracker,
};

use super::{
    decision::Decision,
    predicates::{Action, ValueAggregate, ValueIotaAddress, ValueNumber},
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

    pub fn gas_budget(mut self, gas_size: ValueNumber<u64>) -> Self {
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

    pub fn ptb_command_count(mut self, ptb_command_count: ValueNumber<usize>) -> Self {
        self.rule.ptb_command_count = Some(ptb_command_count);
        self
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct AccessRule {
    pub sender_address: ValueIotaAddress,
    pub transaction_gas_budget: Option<ValueNumber<u64>>,
    pub move_call_package_address: Option<ValueIotaAddress>,
    pub ptb_command_count: Option<ValueNumber<usize>>,
    pub gas_limit: Option<ValueAggregate>,

    pub action: Action,
}

impl AccessRule {
    // TODO This should be removed
    /// Checks if the transaction can be executed based on the access rule and the access policy.
    // pub async fn matches(
    //     &self,
    //     access_policy: AccessPolicy,
    //     data: &TransactionDescription,
    // ) -> Decision {
    //     if self.matches(data).await {
    //         return self.evaluate_access_action();
    //     }

    //     return access_policy.into();
    // }

    // The problem right now is that matching could return the error

    /// Checks if the rule matches the transaction data.
    pub async fn matches(&self, data: &TransactionDescription) -> Result<bool, anyhow::Error> {
        Ok(self.sender_address.includes(&data.sender_address)
            // Gas Budget
            && self
                .transaction_gas_budget
                .map(|size| size.matches(data.transaction_budget))
                // If the gas size is not defined then the rule matches
                .unwrap_or(true)
            // Move Call Package Address
            && self
                .move_call_package_address.as_ref().map(|address| address.includes_any(&data.move_call_package_addresses)).unwrap_or(true)
            && self.ptb_command_count_matches_or_not_applicable(data)
            // Match gas limit
            && self.match_gas_limit(data).await?)
    }

    pub async fn match_gas_limit(
        &self,
        data: &TransactionDescription,
    ) -> Result<bool, anyhow::Error> {
        if let Some(gas_limit) = self.gas_limit.as_ref() {
            if let Some(stats_tracker) = &data.stats_tracker {
                let json_rule = serde_json::to_value(self.clone())
                    .context("Failed to serialize rule to JSON")?;
                let rule_to_hash = json_rule.as_object().context("The rule isn't a map")?;

                let aggr_request = Aggregate::with_name("gas_limit")
                    .with_value(data.transaction_budget as f64)
                    .with_aggr_type(AggregateType::Sum)
                    .with_window(gas_limit.window);

                let total_gas_claim = stats_tracker
                    .update_aggr(rule_to_hash, &aggr_request)
                    .await
                    .context("Updating aggregate failed")?;

                return Ok(gas_limit.limit.matches(total_gas_claim as u64));
            } else {
                bail!("Stats tracker is not defined. But it should be");
            }
        } else {
            return Ok(false);
        }
    }
}

impl AccessRule {
    fn ptb_command_count_matches_or_not_applicable(&self, data: &TransactionDescription) -> bool {
        match (self.ptb_command_count, data.ptb_command_count) {
            (Some(criteria), Some(value)) => criteria.matches(value),
            _ => true,
        }
    }
}

// This input is used to check the access policy.
#[derive(Clone, Default)]
pub struct TransactionDescription {
    pub sender_address: IotaAddress,
    pub transaction_budget: u64,
    pub move_call_package_addresses: Vec<IotaAddress>,
    pub ptb_command_count: Option<usize>,

    pub stats_tracker: Option<StatsTracker<RedisTrackerStorage>>,
}

impl TransactionDescription {
    pub fn new(_signature: &GenericSignature, transaction_data: &TransactionData) -> Self {
        let ptb_command_count = match transaction_data {
            TransactionData::V1(TransactionDataV1 {
                kind: TransactionKind::ProgrammableTransaction(pt),
                ..
            }) => Some(pt.commands.len()),
            TransactionData::V1(TransactionDataV1 { kind: _, .. }) => None,
        };
        Self {
            sender_address: transaction_data.sender().clone(),
            transaction_budget: transaction_data.gas_budget(),
            move_call_package_addresses: get_move_call_package_addresses(transaction_data),
            ptb_command_count,
            stats_tracker: None,
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

    pub fn with_ptb_command_count(mut self, ptb_count: usize) -> Self {
        self.ptb_command_count = Some(ptb_count);
    }

    pub fn with_stats_tracker(mut self, stats_tracker: StatsTracker<RedisTrackerStorage>) -> Self {
        self.stats_tracker = Some(stats_tracker);
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
        predicates::{ValueIotaAddress, ValueNumber},
        rule::{AccessRule, AccessRuleBuilder, Action, Decision, TransactionDescription},
    };

    #[tokio::test]
    async fn test_constraint_src_address_defined_and_allowed() {
        let sender_address = IotaAddress::new([1; 32]);
        let rule = super::AccessRule {
            sender_address: [sender_address].into(),
            action: Action::Allow,
            ..Default::default()
        };
        let data_with_allowed_sender =
            TransactionDescription::default().with_sender_address(sender_address);
        let data_with_denied_sender = TransactionDescription::default();

        assert!(rule.matches(&data_with_allowed_sender).await.unwrap());
        assert!(rule.matches(&data_with_denied_sender).await.unwrap());
    }

    #[tokio::test]
    async fn test_constraint_src_address_defined_and_denied() {
        let sender_address = IotaAddress::new([1; 32]);
        let rule = super::AccessRule {
            sender_address: [sender_address].into(),
            ..Default::default()
        };
        let data_with_allowed_sender =
            TransactionDescription::default().with_sender_address(sender_address);
        let data_with_denied_sender = TransactionDescription::default();

        assert!(rule.matches(&data_with_allowed_sender).await.unwrap());
        assert!(!rule.matches(&data_with_denied_sender).await.unwrap());
    }

    #[tokio::test]
    async fn test_constraint_gas_budget() {
        let gas_limit = 100;
        let rule = AccessRuleBuilder::new()
            .gas_budget(ValueNumber::LessThanOrEqual(gas_limit))
            .build();

        let low_transaction_budget = TransactionDescription::default().with_gas_budget(50);
        let high_transaction_budget = TransactionDescription::default().with_gas_budget(200);

        assert!(rule.matches(&low_transaction_budget).await.unwrap());
        assert!(!rule.matches(&high_transaction_budget).await.unwrap());
    }

    #[tokio::test]
    async fn test_constraint_move_call_package_addr() {
        let move_call_package_address = IotaAddress::new([1; 32]);
        let rule = AccessRuleBuilder::new()
            .move_call_package_address(move_call_package_address)
            .build();

        let transaction_description = TransactionDescription::default()
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert!(rule.matches(&transaction_description).await.unwrap());
        assert!(!rule.matches(&transaction_description).await.unwrap());
    }

    #[tokio::test]
    async fn test_constraint_mix_ups_sender_package_address() {
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

        assert!(rule.matches(&transaction_description).await.unwrap());

        let transaction_description_with_not_matched_package_address =
            TransactionDescription::default()
                .with_sender_address(sender_address)
                .with_move_call_package_addresses(vec![IotaAddress::new([3; 32])]);

        assert!(!rule
            .matches(&transaction_description_with_not_matched_package_address)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_constraint_mix_ups_sender_budget_package_address() {
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

        assert!(rule.matches(&transaction_description).await.unwrap());

        let transaction_description_with_not_matched_package_address =
            TransactionDescription::default()
                .with_sender_address(sender_address)
                .with_gas_budget(gas_limit)
                .with_move_call_package_addresses(vec![IotaAddress::new([3; 32])]);

        assert!(!rule
            .matches(&transaction_description_with_not_matched_package_address)
            .await
            .unwrap());

        let transaction_description_with_not_matched_gas_limit = TransactionDescription::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_limit + 1)
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert!(!rule
            .matches(&transaction_description_with_not_matched_gas_limit)
            .await
            .unwrap());
    }

    #[test]
    fn test_constraint_ptb_count_matches() {
        let rule = super::AccessRule {
            sender_address: ValueIotaAddress::All,
            action: Action::Allow,
            ptb_command_count: Some(ValueNumber::LessThanOrEqual(1)),
            ..Default::default()
        };
        let data_with_matching_ptb_count =
            TransactionDescription::default().with_ptb_command_count(1);
        let data_with_not_matching_ptb_count =
            TransactionDescription::default().with_ptb_command_count(5);

        assert!(rule.matches(&data_with_matching_ptb_count));
        assert!(!rule.matches(&data_with_not_matching_ptb_count));
    }

    #[tokio::test]
    async fn test_allow_when_deny_all() {
        let sender_address = IotaAddress::new([0; 32]);
        let input = TransactionDescription::default().with_sender_address(sender_address);
        let access_rule = AccessRule {
            sender_address: [sender_address].into(),
            action: Action::Allow,
            ..Default::default()
        };

        assert!(access_rule.matches(&input).await.unwrap());
    }
}
