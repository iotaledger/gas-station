// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context;
use iota_types::{
    base_types::IotaAddress,
    digests::TransactionDigest,
    signature::GenericSignature,
    transaction::{TransactionData, TransactionDataAPI, TransactionDataV1, TransactionKind},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use serde_with::skip_serializing_none;

use crate::tracker::{
    stats_tracker_storage::{Aggregate, AggregateType},
    StatsTracker,
};

use super::predicates::{Action, LimitBy, ValueAggregate, ValueIotaAddress, ValueNumber};

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

    pub fn gas_limit(mut self, gas_limit: ValueAggregate) -> Self {
        self.rule.gas_usage = Some(gas_limit);
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
    pub gas_usage: Option<ValueAggregate>,

    pub action: Action,
}

#[derive(Clone, Default)]
pub struct GasUsageConfirmationRequest {
    pub rule_meta: Map<String, Value>,
    pub aggregate: Aggregate,
    pub gas_usage: u64,
}

impl AccessRule {
    /// Checks if the rule matches the transaction data.
    pub async fn matches(&self, data: &TransactionContext) -> Result<bool, anyhow::Error> {
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
            && self.ptb_command_count_matches_or_not_applicable(data))
    }

    /// Match checking for global limits. Global limits use a persistent storage to track their values
    pub async fn match_global_limits(
        &self,
        ctx: &TransactionContext,
    ) -> Result<(bool, Vec<GasUsageConfirmationRequest>), anyhow::Error> {
        let mut confirmation_requests = vec![];
        let gas_limit_result = self
            .match_gas_limit(ctx)
            .await
            .context("failed to match gas limit")?;
        if let Some(confirmation_request) = gas_limit_result.1 {
            confirmation_requests.push(confirmation_request);
        }
        let result = (gas_limit_result.0, confirmation_requests);
        Ok(result)
    }

    /// Returns the rule meta data as a JSON object. The rule meta is used to calculate the hash of the rule.
    fn get_rule_meta(&self, ctx: &TransactionContext) -> Result<Map<String, Value>, anyhow::Error> {
        let json_rule =
            serde_json::to_value(self.clone()).context("Failed to serialize rule to JSON")?;
        let mut rule_to_hash = json_rule
            .as_object()
            .context("The rule isn't a map")?
            .to_owned();

        if let Some(gas_limit) = self.gas_usage.as_ref() {
            for count_by in gas_limit.count_by.iter() {
                let count_by_value = match count_by {
                    LimitBy::SenderAddress => ctx.sender_address.to_string(),
                };
                (&mut rule_to_hash).insert(count_by.to_string(), Value::String(count_by_value));
            }
        }
        Ok(rule_to_hash)
    }

    async fn match_gas_limit(
        &self,
        ctx: &TransactionContext,
    ) -> Result<(bool, Option<GasUsageConfirmationRequest>), anyhow::Error> {
        if let Some(gas_limit) = self.gas_usage.as_ref() {
            let rule_meta = self
                .get_rule_meta(ctx)
                .context("Failed to calculate rule meta")?;

            let aggr = Aggregate::with_name("gas_usage")
                .with_aggr_type(AggregateType::Sum)
                .with_window(gas_limit.window);

            let total_gas_claim = ctx
                .stats_tracker
                .update_aggr(rule_meta.clone(), &aggr, ctx.transaction_budget as i64)
                .await
                .context("Updating aggregate failed")?;

            let confirmation_request = GasUsageConfirmationRequest {
                rule_meta,
                aggregate: aggr,
                gas_usage: ctx.transaction_budget,
            };

            return Ok((
                gas_limit.value.matches(total_gas_claim as u64),
                Some(confirmation_request),
            ));
        } else {
            // If the gas limit is not defined then the rule matches
            return Ok((true, None));
        }
    }
}

impl AccessRule {
    fn ptb_command_count_matches_or_not_applicable(&self, data: &TransactionContext) -> bool {
        match (self.ptb_command_count, data.ptb_command_count) {
            (Some(criteria), Some(value)) => criteria.matches(value),
            _ => true,
        }
    }
}

// This input is used to check the access policy.
#[derive(Clone)]
pub struct TransactionContext {
    pub transaction_digest: TransactionDigest,
    pub sender_address: IotaAddress,
    pub transaction_budget: u64,
    pub move_call_package_addresses: Vec<IotaAddress>,
    pub ptb_command_count: Option<usize>,

    pub stats_tracker: StatsTracker,
}

#[cfg(test)]
impl Default for TransactionContext {
    fn default() -> Self {
        Self {
            sender_address: IotaAddress::default(),
            transaction_budget: 0,
            move_call_package_addresses: vec![],
            ptb_command_count: None,
            stats_tracker: crate::test_env::mocked_stats_tracker(),
            transaction_digest: TransactionDigest::default(),
        }
    }
}

impl TransactionContext {
    pub fn new(
        _signature: &GenericSignature,
        transaction_data: &TransactionData,
        stats_tracker: StatsTracker,
    ) -> Self {
        let ptb_command_count = match transaction_data {
            TransactionData::V1(TransactionDataV1 {
                kind: TransactionKind::ProgrammableTransaction(pt),
                ..
            }) => Some(pt.commands.len()),
            TransactionData::V1(TransactionDataV1 { kind: _, .. }) => None,
        };
        Self {
            transaction_digest: transaction_data.digest(),
            sender_address: transaction_data.sender().clone(),
            transaction_budget: transaction_data.gas_budget(),
            move_call_package_addresses: get_move_call_package_addresses(transaction_data),
            ptb_command_count,
            stats_tracker,
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
        self
    }

    pub fn with_stats_tracker(mut self, stats_tracker: StatsTracker) -> Self {
        self.stats_tracker = stats_tracker;
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
    use itertools::GroupBy;

    use crate::{
        access_controller::{
            predicates::{Action, LimitBy, ValueAggregate, ValueIotaAddress, ValueNumber},
            rule::{AccessRule, AccessRuleBuilder, TransactionContext},
        },
        test_env::{new_stats_tracker_for_testing, random_address},
    };

    #[tokio::test]
    async fn test_constraint_sender_address() {
        let matched_sender = IotaAddress::new([0; 32]);
        let unmatched_sender = IotaAddress::new([1; 32]);

        let matched_data = TransactionContext::default().with_sender_address(matched_sender);
        let unmatched_data = TransactionContext::default().with_sender_address(unmatched_sender);

        let rule = AccessRule {
            sender_address: [matched_sender].into(),
            ..Default::default()
        };

        assert!(rule.matches(&matched_data).await.unwrap());
        assert!(!rule.matches(&unmatched_data).await.unwrap());
    }

    #[tokio::test]
    async fn test_constraint_gas_budget() {
        let gas_limit = 100;
        let rule = AccessRuleBuilder::new()
            .gas_budget(ValueNumber::LessThanOrEqual(gas_limit))
            .build();

        let matched_data = TransactionContext::default().with_gas_budget(50);
        let unmatched_data = TransactionContext::default().with_gas_budget(200);

        assert!(rule.matches(&matched_data).await.unwrap());
        assert!(!rule.matches(&unmatched_data).await.unwrap());
    }

    #[tokio::test]
    async fn test_constraint_move_call_package_addr() {
        let matched_package_id = IotaAddress::new([1; 32]);
        let unmatch_package_id = IotaAddress::new([2; 32]);

        let rule = AccessRuleBuilder::new()
            .move_call_package_address(matched_package_id)
            .build();

        let matched_data = TransactionContext::default()
            .with_move_call_package_addresses(vec![matched_package_id]);
        let unmatched_data = TransactionContext::default()
            .with_move_call_package_addresses(vec![unmatch_package_id]);

        assert!(rule.matches(&matched_data).await.unwrap());
        assert!(!rule.matches(&unmatched_data).await.unwrap());
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

        let data = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_limit)
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert!(rule.matches(&data).await.unwrap());

        let unmatched_data_package_address = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_limit)
            .with_move_call_package_addresses(vec![IotaAddress::new([3; 32])]);

        assert!(!rule.matches(&unmatched_data_package_address).await.unwrap());

        let unmatched_data_gas_limit = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_limit + 1)
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert!(!rule.matches(&unmatched_data_gas_limit).await.unwrap());
    }

    #[tokio::test]
    async fn test_constraint_ptb_count_matches() {
        let rule = super::AccessRule {
            sender_address: ValueIotaAddress::All,
            action: Action::Allow,
            ptb_command_count: Some(ValueNumber::LessThanOrEqual(1)),
            ..Default::default()
        };
        let data_with_matching_ptb_count = TransactionContext::default().with_ptb_command_count(1);
        let data_with_not_matching_ptb_count =
            TransactionContext::default().with_ptb_command_count(5);

        assert!(rule.matches(&data_with_matching_ptb_count).await.unwrap());
        assert!(!rule
            .matches(&data_with_not_matching_ptb_count)
            .await
            .unwrap());
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

        let matched_data = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert!(rule.matches(&matched_data).await.unwrap());

        let unmatched_data = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![IotaAddress::new([3; 32])]);

        assert!(!rule.matches(&unmatched_data).await.unwrap());
    }

    #[tokio::test]
    async fn test_constraint_gas_usage_matches() {
        let sponsor_address = random_address();
        let sender_address_limited = random_address();
        let sender_address_unlimited = random_address();
        let stats_tracker = new_stats_tracker_for_testing(sponsor_address).await;

        let rule = AccessRuleBuilder::new()
            .sender_address(sender_address_limited)
            .gas_limit(
                ValueAggregate::new(
                    std::time::Duration::from_secs(10),
                    ValueNumber::GreaterThanOrEqual(300),
                )
                .with_count_by(vec![LimitBy::SenderAddress]),
            )
            .deny()
            .build();

        // The context will be matched second time, because the gas limit increments
        // and crosses 300 threshold
        let matched_data = TransactionContext::default()
            .with_sender_address(sender_address_limited)
            .with_gas_budget(200)
            .with_stats_tracker(stats_tracker.clone());

        // The wont be matched, because the sender address is different
        let unmatched_data = TransactionContext::default()
            .with_sender_address(sender_address_unlimited)
            .with_gas_budget(200)
            .with_stats_tracker(stats_tracker.clone());

        assert!(!rule.match_global_limits(&matched_data).await.unwrap().0);
        assert!(rule.match_global_limits(&matched_data).await.unwrap().0);
        assert!(!rule.match_global_limits(&unmatched_data).await.unwrap().0);
    }
}
