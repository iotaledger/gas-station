// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use anyhow::Context;
use axum::http::HeaderMap;
use iota_sdk_types::{Address, Transaction, TransactionDigest, UserSignature};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use serde_with::skip_serializing_none;
use tracing::trace;
use url::Url;

use crate::{
    access_controller::{
        hook::{HookAction, HookActionHeaders},
        predicates::{
            Action, CountBy, RegoExpression, ValueAggregate, ValueIotaAddress, ValueNumber,
        },
        rego_input,
        reports::{PredicateReport, RuleReport},
        utils::header_map_to_btree_map,
    },
    base64::Base64,
    rpc::rpc_types::ExecuteTransactionRequestType,
    tracker::{
        stats_tracker_storage::{Aggregate, AggregateType},
        StatsTracker,
    },
};

pub(crate) mod predicate_names {
    pub const SENDER_ADDRESS: &str = "sender_address";
    pub const GAS_BUDGET: &str = "gas_budget";
    pub const MOVE_CALL_PACKAGE_ADDRESS: &str = "move_call_package_address";
    pub const PTB_COMMAND_COUNT: &str = "ptb_command_count";
    pub const GAS_USAGE: &str = "gas_usage";
    pub const REG_O_EXPRESSION: &str = "rego_expression";
}

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

    pub fn sender_address(mut self, sender_address: impl Into<Address>) -> Self {
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

    /// Sets the action of the AccessRule to deny.
    pub fn deny(mut self) -> Self {
        self.rule.action = Action::Deny;
        self
    }

    /// Sets the action of the AccessRule to call hook.
    pub fn hook(
        mut self,
        url: Url,
        headers: Option<HookActionHeaders>,
    ) -> Result<Self, anyhow::Error> {
        let hook_action = if let Some(headers_value) = headers {
            HookAction::new_detailed(url, Some(headers_value))?
        } else {
            HookAction::new_url(url)?
        };
        self.rule.action = Action::HookAction(hook_action);

        Ok(self)
    }

    pub fn gas_budget(mut self, gas_size: ValueNumber<u64>) -> Self {
        self.rule.transaction_gas_budget = Some(gas_size);
        self
    }

    pub fn move_call_package_address(mut self, address: impl Into<Address>) -> Self {
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

    pub fn rego_expression(mut self, rego_expression: RegoExpression) -> Self {
        self.rule.rego_expression = Some(rego_expression);
        self
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct AccessRule {
    #[serde(default)]
    pub sender_address: ValueIotaAddress,
    pub transaction_gas_budget: Option<ValueNumber<u64>>,
    pub move_call_package_address: Option<ValueIotaAddress>,
    pub ptb_command_count: Option<ValueNumber<usize>>,
    pub gas_usage: Option<ValueAggregate>,
    pub rego_expression: Option<RegoExpression>,

    pub action: Action,
}

#[derive(Clone, Default)]
pub struct GasUsageConfirmationRequest {
    pub rule_meta: Map<String, Value>,
    pub aggregate: Aggregate,
    pub gas_usage: u64,
}

impl AccessRule {
    pub async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        trace!("Initializing rule: {:?}", self);
        if let Some(rego_expression) = self.rego_expression.as_mut() {
            rego_expression.reload_source().await?;
        }
        self.action.initialize()?;
        Ok(())
    }

    /// Returns the action of the rule.
    ///
    /// Checks if the rule matches the transaction data.
    pub async fn matches(&self, data: &TransactionContext) -> Result<RuleReport, anyhow::Error> {
        let mut rule_report = RuleReport::new();

        let sender_address_result = self.match_sender_address(data);
        rule_report.add_predicate_report(sender_address_result);

        // When first predicate is not matched, we don't need to check the rest of the predicates.
        if !rule_report.is_matched {
            return Ok(rule_report);
        }

        let gas_budget_result = self.match_gas_budget(data);
        rule_report.add_predicate_report(gas_budget_result);

        if !rule_report.is_matched {
            return Ok(rule_report);
        }

        let move_call_package_address_result = self.match_move_call_package_address(data);
        rule_report.add_predicate_report(move_call_package_address_result);

        if !rule_report.is_matched {
            return Ok(rule_report);
        }

        let ptb_command_count_result = self.match_ptb_command_count(data);
        rule_report.add_predicate_report(ptb_command_count_result);

        if !rule_report.is_matched {
            return Ok(rule_report);
        }

        let rego_expression_result = self.match_rego_expression(data)?;
        rule_report.add_predicate_report(rego_expression_result);

        Ok(rule_report)
    }

    pub fn match_ptb_command_count(&self, data: &TransactionContext) -> PredicateReport {
        match (self.ptb_command_count, data.ptb_command_count) {
            (Some(criteria), Some(value)) => {
                let is_matched = criteria.matches(value);
                let result_reason = if is_matched {
                    format!("ptb command count {} matches: {}", value, criteria)
                } else {
                    format!("ptb command count {} does not match: {}", value, criteria)
                };
                PredicateReport::new(
                    predicate_names::PTB_COMMAND_COUNT,
                    is_matched,
                    result_reason,
                )
            }
            _ => PredicateReport::new(
                predicate_names::PTB_COMMAND_COUNT,
                true,
                "ptb command count is not defined".to_string(),
            ),
        }
    }

    pub fn match_move_call_package_address(&self, data: &TransactionContext) -> PredicateReport {
        let is_matched = self
            .move_call_package_address
            .as_ref()
            .map(|address| address.includes_any(&data.move_call_package_addresses))
            .unwrap_or(true);
        let result_reason = if self.move_call_package_address.is_some() {
            if is_matched {
                format!(
                    "move call package address {:?} is in the list: '{:?}'",
                    data.move_call_package_addresses,
                    self.move_call_package_address.as_ref().unwrap()
                )
            } else {
                format!(
                    "move call package address {:?} is not in the list: '{:?}'",
                    data.move_call_package_addresses,
                    self.move_call_package_address.as_ref().unwrap()
                )
            }
        } else {
            "move call package address is not defined".to_string()
        };
        PredicateReport::new(
            predicate_names::MOVE_CALL_PACKAGE_ADDRESS,
            is_matched,
            result_reason,
        )
    }

    pub fn match_gas_budget(&self, data: &TransactionContext) -> PredicateReport {
        let is_matched = self
            .transaction_gas_budget
            .map(|size| size.matches(data.transaction_budget))
            .unwrap_or(true);
        let result_reason = if self.transaction_gas_budget.is_some() {
            if is_matched {
                format!(
                    "gas budget {} matches: {}",
                    data.transaction_budget,
                    self.transaction_gas_budget.as_ref().unwrap()
                )
            } else {
                format!(
                    "gas budget {} does not match: {}",
                    data.transaction_budget,
                    self.transaction_gas_budget.as_ref().unwrap()
                )
            }
        } else {
            "gas budget is not defined".to_string()
        };
        PredicateReport::new(predicate_names::GAS_BUDGET, is_matched, result_reason)
    }

    pub fn match_sender_address(&self, data: &TransactionContext) -> PredicateReport {
        let is_matched = self.sender_address.includes(&data.sender_address);
        let result_reason = if is_matched {
            format!(
                "sender address {} is in the list: '{:?}'",
                data.sender_address, self.sender_address
            )
        } else {
            format!(
                "sender address {} is not in the list: '{:?}'",
                data.sender_address, self.sender_address
            )
        };
        PredicateReport::new(predicate_names::SENDER_ADDRESS, is_matched, result_reason)
    }

    /// Match checking for global limits. Global limits use a persistent storage to track their values
    pub async fn match_global_limits(
        &self,
        ctx: &TransactionContext,
    ) -> Result<(RuleReport, Vec<GasUsageConfirmationRequest>), anyhow::Error> {
        let mut confirmation_requests = vec![];
        let mut rule_report = RuleReport::new();
        let gas_limit_result = self
            .match_gas_limit(ctx)
            .await
            .context("failed to match gas limit")?;
        if let Some(confirmation_request) = gas_limit_result.1 {
            confirmation_requests.push(confirmation_request);
        }
        rule_report.add_predicate_report(gas_limit_result.0);
        Ok((rule_report, confirmation_requests))
    }

    /// Returns the rule meta data as a JSON object. The rule meta is used to calculate the hash of the rule.
    pub fn get_rule_meta(
        &self,
        ctx: &TransactionContext,
    ) -> Result<Map<String, Value>, anyhow::Error> {
        let json_rule =
            serde_json::to_value(self.clone()).context("Failed to serialize rule to JSON")?;
        let mut rule_to_hash = json_rule
            .as_object()
            .context("The rule isn't a map")?
            .to_owned();

        if let Some(gas_limit) = self.gas_usage.as_ref() {
            for count_by in gas_limit.count_by.iter() {
                let count_by_value = match count_by {
                    CountBy::SenderAddress => ctx.sender_address.to_string(),
                    CountBy::HttpHeader(header) => {
                        // in case when there are multiple header values, we join them by the comma
                        // we also sort the values to make sure the same header values are hashed the same way
                        let values = ctx.headers.get_all(header.header_name.as_str());
                        values
                            .iter()
                            .map(|value| value.to_str().unwrap_or("").to_string())
                            .sorted()
                            .collect::<Vec<String>>()
                            .join(",")
                    }
                };
                (&mut rule_to_hash).insert(count_by.to_string(), Value::String(count_by_value));
            }
        }
        Ok(rule_to_hash)
    }

    async fn match_gas_limit(
        &self,
        ctx: &TransactionContext,
    ) -> Result<(PredicateReport, Option<GasUsageConfirmationRequest>), anyhow::Error> {
        if let Some(gas_limit) = self.gas_usage.as_ref() {
            let rule_meta = self
                .get_rule_meta(ctx)
                .context("Failed to calculate rule meta for gas limit")?;

            let aggr = Aggregate::with_name(predicate_names::GAS_USAGE)
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
            let is_matched = gas_limit.value.matches(total_gas_claim as u64);
            let result_reason = format!(
                "total gas usage {} {}: {}",
                total_gas_claim,
                if is_matched {
                    "matches"
                } else {
                    "does not match"
                },
                gas_limit.value
            );
            return Ok((
                PredicateReport::new(predicate_names::GAS_USAGE, is_matched, result_reason),
                Some(confirmation_request),
            ));
        } else {
            // If the gas limit is not defined then the rule matches
            return Ok((
                PredicateReport::new(predicate_names::GAS_USAGE, true, "gas usage is not defined"),
                None,
            ));
        }
    }

    fn match_rego_expression(
        &self,
        ctx: &TransactionContext,
    ) -> Result<PredicateReport, anyhow::Error> {
        if let Some(rego_expression) = self.rego_expression.as_ref() {
            let input_payload = RegoInputPayload::from_context(ctx);
            let input_string = serde_json::to_string_pretty(&input_payload)
                .context("Failed to serialize input payload to JSON")?;
            let result = rego_expression
                .matches(&input_string)
                .context("Failed to match rego expression")?;

            let result_reason = if result {
                format!(
                    "rego expression matches: {}",
                    rego_expression.source.location.to_string()
                )
            } else {
                format!(
                    "rego expression does not match: {}",
                    rego_expression.source.location.to_string()
                )
            };
            Ok(PredicateReport::new(
                predicate_names::REG_O_EXPRESSION,
                result,
                result_reason,
            ))
        } else {
            // If the rego expression is not defined then the rule matches. Every payload is allowed
            Ok(PredicateReport::new(
                predicate_names::REG_O_EXPRESSION,
                true,
                "rego expression is not defined",
            ))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegoInputPayload {
    pub transaction_data: Value,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub http_headers: BTreeMap<String, Vec<String>>,
}

impl RegoInputPayload {
    pub fn from_context(ctx: &TransactionContext) -> Self {
        Self {
            transaction_data: ctx.transaction_data.clone(),
            http_headers: header_map_to_btree_map(&ctx.headers),
        }
    }
}

// This input is used to check the access policy.
#[derive(Clone)]
pub struct TransactionContext {
    pub transaction_digest: TransactionDigest,
    pub sender_address: Address,
    pub transaction_budget: u64,
    pub move_call_package_addresses: Vec<Address>,
    pub ptb_command_count: Option<usize>,
    pub transaction_data: Value,

    pub stats_tracker: StatsTracker,
    pub reservation_id: u64,
    pub tx_bytes: Base64,
    pub user_sig: Base64,
    pub request_type: Option<ExecuteTransactionRequestType>,
    pub headers: HeaderMap,
}

#[cfg(test)]
impl Default for TransactionContext {
    fn default() -> Self {
        Self {
            sender_address: Address::ZERO,
            transaction_budget: 0,
            move_call_package_addresses: vec![],
            ptb_command_count: None,
            stats_tracker: crate::test_env::mocked_stats_tracker(),
            transaction_digest: TransactionDigest::default(),
            transaction_data: Value::Null,
            reservation_id: 0,
            tx_bytes: Base64::try_from(String::default())
                .expect("empty string should be valid base64"),
            user_sig: Base64::try_from(String::default())
                .expect("empty string should be valid base64"),
            request_type: None,
            headers: HeaderMap::default(),
        }
    }
}

impl TransactionContext {
    pub fn new(
        _signature: &UserSignature,
        transaction: &Transaction,
        stats_tracker: StatsTracker,
        reservation_id: u64,
        tx_bytes: Base64,
        user_sig: Base64,
        request_type: Option<ExecuteTransactionRequestType>,
        headers: HeaderMap,
    ) -> Self {
        let Transaction::V1(v1) = transaction else {
            // The fields extracted below feed access-control predicates, so
            // there is no safe fallback for an unknown future variant.
            unreachable!("no non-`V1` `Transaction` variant exists in the pinned SDK rev");
        };
        let ptb_command_count = match &v1.kind {
            iota_sdk_types::TransactionKind::Programmable(pt) => Some(pt.commands.len()),
            _ => None,
        };
        let transaction_value = rego_input::to_legacy_json(transaction);
        Self {
            transaction_digest: transaction.digest(),
            sender_address: v1.sender,
            transaction_budget: v1.gas_payment.budget,
            move_call_package_addresses: get_move_call_package_addresses(transaction),
            ptb_command_count,
            stats_tracker,
            transaction_data: transaction_value,
            reservation_id,
            tx_bytes,
            user_sig,
            request_type,
            headers,
        }
    }

    pub fn with_sender_address(mut self, sender_address: Address) -> Self {
        self.sender_address = sender_address;
        self
    }

    pub fn with_gas_budget(mut self, transaction_budget: u64) -> Self {
        self.transaction_budget = transaction_budget;
        self
    }

    pub fn with_move_call_package_addresses(
        mut self,
        move_call_package_addresses: Vec<Address>,
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

    pub fn with_transaction_data(mut self, transaction_data: Value) -> Self {
        self.transaction_data = transaction_data;
        self
    }

    pub fn with_reservation_id(mut self, reservation_id: u64) -> Self {
        self.reservation_id = reservation_id;
        self
    }

    pub fn with_tx_bytes(mut self, tx_bytes: Base64) -> Self {
        self.tx_bytes = tx_bytes;
        self
    }

    pub fn with_user_sig(mut self, user_sig: Base64) -> Self {
        self.user_sig = user_sig;
        self
    }

    pub fn with_request_type(mut self, request_type: ExecuteTransactionRequestType) -> Self {
        self.request_type = Some(request_type);
        self
    }

    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }
}

/// Old-shape equivalent of `TransactionDataAPI::move_calls`'s package list.
/// See `rego_input::move_calls` for the port itself; this just adapts its
/// return type (`iota_sdk_types::ObjectId`) into the `Address` type
/// `ValueIotaAddress`/`move-call-package-address` expect.
fn get_move_call_package_addresses(transaction: &Transaction) -> Vec<Address> {
    rego_input::move_calls(transaction)
        .into_iter()
        .map(|(package, _module, _function)| Address::from(package))
        .collect()
}

#[cfg(test)]
mod test {

    use std::{str::FromStr, vec};

    use axum::http::{HeaderMap, HeaderName, HeaderValue};
    use iota_sdk_types::{
        Address, Argument, Command, GasPayment, Identifier, MoveCall, ObjectId,
        ProgrammableTransaction, Transaction, TransactionExpiration, TransactionKind,
        TransactionV1,
    };

    use crate::{
        access_controller::{
            predicates::{
                Action, CountBy, Location, RegoExpression, SourceWithData, ValueAggregate,
                ValueIotaAddress, ValueNumber,
            },
            rego_input,
            rule::{AccessRule, AccessRuleBuilder, TransactionContext},
        },
        test_env::{new_stats_tracker_for_testing, random_address},
    };

    /// Builds a minimal single-`MoveCall` programmable transaction, useful
    /// for exercising the Rego-input compat layer end-to-end.
    fn move_call_transaction(
        sender: Address,
        package: Address,
        module: &str,
        function: &str,
        pure_inputs: Vec<Vec<u8>>,
    ) -> Transaction {
        let arguments = (0..pure_inputs.len() as u16).map(Argument::Input).collect();
        Transaction::V1(TransactionV1 {
            kind: TransactionKind::Programmable(ProgrammableTransaction {
                inputs: pure_inputs
                    .into_iter()
                    .map(iota_sdk_types::Input::Pure)
                    .collect(),
                commands: vec![Command::MoveCall(MoveCall {
                    package: ObjectId::from(package),
                    module: Identifier::new_unchecked(module),
                    function: Identifier::new_unchecked(function),
                    type_arguments: vec![],
                    arguments,
                })],
            }),
            sender,
            gas_payment: GasPayment {
                objects: vec![],
                owner: sender,
                price: 0,
                budget: 0,
            },
            expiration: TransactionExpiration::None,
        })
    }

    #[tokio::test]
    async fn test_constraint_sender_address() {
        let matched_sender = Address::from_bytes([0; 32]).unwrap();
        let unmatched_sender = Address::from_bytes([1; 32]).unwrap();

        let matched_data = TransactionContext::default().with_sender_address(matched_sender);
        let unmatched_data = TransactionContext::default().with_sender_address(unmatched_sender);

        let rule = AccessRule {
            sender_address: [matched_sender].into(),
            ..Default::default()
        };

        assert!(rule.matches(&matched_data).await.unwrap().is_matched);
        assert!(!rule.matches(&unmatched_data).await.unwrap().is_matched);
    }

    #[tokio::test]
    async fn test_constraint_gas_budget() {
        let gas_limit = 100;
        let rule = AccessRuleBuilder::new()
            .gas_budget(ValueNumber::LessThanOrEqual(gas_limit))
            .build();

        let matched_data = TransactionContext::default().with_gas_budget(50);
        let unmatched_data = TransactionContext::default().with_gas_budget(200);

        assert!(rule.matches(&matched_data).await.unwrap().is_matched);
        assert!(!rule.matches(&unmatched_data).await.unwrap().is_matched);
    }

    #[tokio::test]
    async fn test_constraint_move_call_package_addr() {
        let matched_package_id = Address::from_bytes([1; 32]).unwrap();
        let unmatch_package_id = Address::from_bytes([2; 32]).unwrap();

        let rule = AccessRuleBuilder::new()
            .move_call_package_address(matched_package_id)
            .build();

        let matched_data = TransactionContext::default()
            .with_move_call_package_addresses(vec![matched_package_id]);
        let unmatched_data = TransactionContext::default()
            .with_move_call_package_addresses(vec![unmatch_package_id]);

        assert!(rule.matches(&matched_data).await.unwrap().is_matched);
        assert!(!rule.matches(&unmatched_data).await.unwrap().is_matched);
    }

    #[tokio::test]
    async fn test_constraint_mix_ups_sender_budget_package_address() {
        let sender_address = Address::from_bytes([1; 32]).unwrap();
        let move_call_package_address = Address::from_bytes([2; 32]).unwrap();
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

        assert!(rule.matches(&data).await.unwrap().is_matched);

        let unmatched_data_package_address = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_limit)
            .with_move_call_package_addresses(vec![Address::from_bytes([3; 32]).unwrap()]);

        assert!(
            !rule
                .matches(&unmatched_data_package_address)
                .await
                .unwrap()
                .is_matched
        );

        let unmatched_data_gas_limit = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_gas_budget(gas_limit + 1)
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert!(
            !rule
                .matches(&unmatched_data_gas_limit)
                .await
                .unwrap()
                .is_matched
        );
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

        assert!(
            rule.matches(&data_with_matching_ptb_count)
                .await
                .unwrap()
                .is_matched
        );
        assert!(
            !rule
                .matches(&data_with_not_matching_ptb_count)
                .await
                .unwrap()
                .is_matched
        );
    }

    #[tokio::test]
    async fn test_constraint_mix_ups_sender_package_address() {
        let sender_address = Address::from_bytes([1; 32]).unwrap();
        let move_call_package_address = Address::from_bytes([2; 32]).unwrap();

        let rule = AccessRuleBuilder::new()
            .sender_address(sender_address)
            .move_call_package_address(move_call_package_address)
            .allow()
            .build();

        let matched_data = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![move_call_package_address]);

        assert!(rule.matches(&matched_data).await.unwrap().is_matched);

        let unmatched_data = TransactionContext::default()
            .with_sender_address(sender_address)
            .with_move_call_package_addresses(vec![Address::from_bytes([3; 32]).unwrap()]);

        assert!(!rule.matches(&unmatched_data).await.unwrap().is_matched);
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
                .with_count_by(vec![CountBy::SenderAddress]),
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

        assert!(
            !rule
                .match_global_limits(&matched_data)
                .await
                .unwrap()
                .0
                .is_matched
        );
        assert!(
            rule.match_global_limits(&matched_data)
                .await
                .unwrap()
                .0
                .is_matched
        );
        assert!(
            !rule
                .match_global_limits(&unmatched_data)
                .await
                .unwrap()
                .0
                .is_matched
        );
    }

    #[tokio::test]
    async fn test_constraint_rego_expression() {
        let rego_content = r#"
            package test

            default allow_sender = false
            allow_sender if {
                input.transaction_data.V1.sender == "0x1212121212121212121212121212121212121212121212121212121212121212"
            }
        "#;
        let build_transaction = |sender_byte: u8| {
            Transaction::V1(TransactionV1 {
                kind: TransactionKind::Programmable(ProgrammableTransaction {
                    commands: vec![],
                    inputs: vec![],
                }),
                expiration: TransactionExpiration::None,
                gas_payment: GasPayment {
                    objects: vec![],
                    owner: Address::ZERO,
                    budget: 0,
                    price: 0,
                },
                sender: Address::from_bytes([sender_byte; 32]).unwrap(),
            })
        };
        let location = Location::new_memory(rego_content, "data.test.allow_sender");
        let mut source = SourceWithData::new(location.clone());
        source.fetch().await.unwrap();
        let rego_expression =
            RegoExpression::from_source(source).expect("Failed to create Rego expression");

        let rule = AccessRuleBuilder::new()
            .rego_expression(rego_expression)
            .allow()
            .build();
        let matched_data = TransactionContext::default()
            .with_transaction_data(rego_input::to_legacy_json(&build_transaction(0x12)));
        assert!(rule.matches(&matched_data).await.unwrap().is_matched);

        // Test with unmatched sender address
        let unmatched_data = TransactionContext::default()
            .with_transaction_data(rego_input::to_legacy_json(&build_transaction(0x13)));
        assert!(
            !rule
                .match_rego_expression(&unmatched_data)
                .unwrap()
                .is_matched
        );
    }

    /// End-to-end test running the "Rego Filtering Code Example" from
    /// `docs/access-controller.md` **verbatim** against a transaction built
    /// through the SDK-migration compat layer (`rego_input::to_legacy_json`),
    /// asserting it evaluates the way the docs describe: matches only a
    /// single move call to the documented package/module/function with a
    /// first argument that BCS-decodes to the string `"hello"`.
    #[tokio::test]
    async fn test_documented_rego_move_call_matches_example() {
        // Verbatim from docs/access-controller.md, "Rego Filtering Code Example".
        let rego_content = r#"
package matchers

default move_call_matches = false

move_call_matches if {
    cmds := input.transaction_data.V1.kind.ProgrammableTransaction.commands
    count(cmds) == 1


    mc := cmds[0].MoveCall
    mc["package"]  == "0xb674e2ed79db3c25fa4c00d5c7d62a9c18089e1fc4c2de5b5ee8b2836a85ae26"
    mc.module   == "allowed_module_name"
    mc.function == "allowed_function_name"

    argv_1 := input.transaction_data.V1.kind.ProgrammableTransaction.inputs[0].Pure
    bcs.decode_typed(argv_1, "string") == "hello"
}
"#;
        let location = Location::new_memory(rego_content, "data.matchers.move_call_matches");
        let mut source = SourceWithData::new(location);
        source.fetch().await.unwrap();
        let rego_expression =
            RegoExpression::from_source(source).expect("valid, documented rego policy");
        let rule = AccessRuleBuilder::new()
            .rego_expression(rego_expression)
            .allow()
            .build();

        let package = Address::from_str(
            "0xb674e2ed79db3c25fa4c00d5c7d62a9c18089e1fc4c2de5b5ee8b2836a85ae26",
        )
        .unwrap();
        let sender = Address::from_bytes([0xa2; 32]).unwrap();
        let hello_bcs = bcs::to_bytes(&"hello".to_string()).unwrap();

        // Matches: exactly the documented package/module/function, first
        // argument BCS-decodes to "hello".
        let matching = TransactionContext::default().with_transaction_data(
            rego_input::to_legacy_json(&move_call_transaction(
                sender,
                package,
                "allowed_module_name",
                "allowed_function_name",
                vec![hello_bcs.clone()],
            )),
        );
        assert!(rule.matches(&matching).await.unwrap().is_matched);

        // Doesn't match: right package/module, wrong function.
        let wrong_function = TransactionContext::default().with_transaction_data(
            rego_input::to_legacy_json(&move_call_transaction(
                sender,
                package,
                "allowed_module_name",
                "some_other_function",
                vec![hello_bcs.clone()],
            )),
        );
        assert!(
            !rule
                .match_rego_expression(&wrong_function)
                .unwrap()
                .is_matched
        );

        // Doesn't match: right call, but wrong decoded argument value.
        let wrong_argument = TransactionContext::default().with_transaction_data(
            rego_input::to_legacy_json(&move_call_transaction(
                sender,
                package,
                "allowed_module_name",
                "allowed_function_name",
                vec![bcs::to_bytes(&"goodbye".to_string()).unwrap()],
            )),
        );
        assert!(
            !rule
                .match_rego_expression(&wrong_argument)
                .unwrap()
                .is_matched
        );
    }

    #[tokio::test]
    async fn test_constraint_rego_expression_with_http_header() {
        let rego_content = r#"
            package test

            default allow_account = false
            allow_account if {
                input.http_headers["x-account-id"][0] == "123"
            }
        "#;
        let location = Location::new_memory(rego_content, "data.test.allow_account");
        let mut source = SourceWithData::new(location);
        source.fetch().await.unwrap();

        let rego_expression =
            RegoExpression::from_source(source).expect("Failed to create Rego expression");
        let rule = AccessRuleBuilder::new()
            .rego_expression(rego_expression)
            .allow()
            .build();

        let matched_data = TransactionContext::default().with_headers(HeaderMap::from_iter([(
            HeaderName::from_str("X-Account-Id").unwrap(),
            HeaderValue::from_str("123").unwrap(),
        )]));
        assert!(rule.matches(&matched_data).await.unwrap().is_matched);

        let unmatched_data = TransactionContext::default().with_headers(HeaderMap::from_iter([(
            HeaderName::from_str("X-Account-Id").unwrap(),
            HeaderValue::from_str("456").unwrap(),
        )]));
        assert!(!rule.matches(&unmatched_data).await.unwrap().is_matched);
    }

    #[tokio::test]
    async fn test_constraint_rego_expression_with_multiple_http_header_values() {
        let rego_content = r#"
            package test

            default allow_account = false
            allow_account if {
                input.http_headers["x-account-id"][0] == "123"
                input.http_headers["x-account-id"][1] == "456"
            }
        "#;
        let location = Location::new_memory(rego_content, "data.test.allow_account");
        let mut source = SourceWithData::new(location);
        source.fetch().await.unwrap();

        let rego_expression =
            RegoExpression::from_source(source).expect("Failed to create Rego expression");
        let rule = AccessRuleBuilder::new()
            .rego_expression(rego_expression)
            .allow()
            .build();

        let matched_data = TransactionContext::default().with_headers(HeaderMap::from_iter([
            (
                HeaderName::from_str("X-Account-Id").unwrap(),
                HeaderValue::from_str("123").unwrap(),
            ),
            (
                HeaderName::from_str("X-Account-Id").unwrap(),
                HeaderValue::from_str("456").unwrap(),
            ),
        ]));
        assert!(rule.matches(&matched_data).await.unwrap().is_matched);
    }

    #[tokio::test]
    async fn test_constraint_gas_usage_with_http_header() {
        let sender_address = random_address();
        let sponsor_address = random_address();
        let stats_tracker = new_stats_tracker_for_testing(sponsor_address).await;
        let rule = AccessRuleBuilder::new()
            .gas_limit(
                ValueAggregate::new(
                    std::time::Duration::from_secs(10),
                    ValueNumber::LessThanOrEqual(300),
                )
                .with_count_by(vec![CountBy::new_http_header("X-Account-Id")]),
            )
            .allow()
            .build();

        let account_1_ctx = TransactionContext::default()
            .with_headers(HeaderMap::from_iter([(
                HeaderName::from_str("X-Account-Id").unwrap(),
                HeaderValue::from_str("123").unwrap(),
            )]))
            .with_gas_budget(300)
            .with_stats_tracker(stats_tracker.clone())
            .with_sender_address(sender_address);
        let account_2_ctx = TransactionContext::default()
            .with_headers(HeaderMap::from_iter([(
                HeaderName::from_str("X-Account-Id").unwrap(),
                HeaderValue::from_str("456").unwrap(),
            )]))
            .with_gas_budget(300)
            .with_stats_tracker(stats_tracker)
            .with_sender_address(sender_address);

        // Even though the transactions come from the same sender, they should
        // be distinguished by the account ID. Each account ID should have a separate
        // gas usage limit and should be separately blocked after its limit is used.
        assert!(
            rule.match_global_limits(&account_1_ctx)
                .await
                .unwrap()
                .0
                .is_matched
        );
        assert!(
            !rule
                .match_global_limits(&account_1_ctx)
                .await
                .unwrap()
                .0
                .is_matched
        );
        assert!(
            rule.match_global_limits(&account_2_ctx)
                .await
                .unwrap()
                .0
                .is_matched
        );
        assert!(
            !rule
                .match_global_limits(&account_2_ctx)
                .await
                .unwrap()
                .0
                .is_matched
        );
    }
}
