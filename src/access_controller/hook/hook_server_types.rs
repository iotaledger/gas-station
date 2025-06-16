// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! Types related to hook server. Kept in sync with API spec.

use serde::{Deserialize, Serialize};

use super::HookActionHeaders;

/// Input for hook to check if transaction should be executed.
/// Contains original request for Gas Stations `execute_tx` endpoint.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTxHookRequest {
    pub execute_tx_request: ExecuteTxGasStationRequest,
}

/// Original request data and headers sent to Gas Stations `execute_tx` endpoint.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTxGasStationRequest {
    pub payload: ExecuteTxRequestPayload,
    pub headers: HookActionHeaders,
}

/// Data originally sent to IOTA Gas Station.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTxRequestPayload {
    /// ID used to reference a gas reservation.
    pub reservation_id: u64,
    /// Transaction as base64 encoded BCS serialized `TransactionData`.
    pub tx_bytes: String,
    /// Base64 encoded user signature.
    pub user_sig: String,
}

/// Result of checking if transaction should be executed.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTxOkResponse {
    /// Hooks decision about transaction execution.
    pub decision: SkippableDecision,
    /// Message intended to be forwarded to caller.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
}

/// "allow"/"deny" transaction or take "noDecision" and proceed with other rules.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkippableDecision {
    Allow,
    Deny,
    NoDecision,
}
