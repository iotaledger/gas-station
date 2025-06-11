// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use anyhow::Context as _;
use axum::http::StatusCode;
use base64::prelude::*;
use iota_types::transaction::TransactionData;
use serde::Deserialize;
use serde::Serialize;
use utoipa::ToSchema;

use crate::RequestError;

/// Input for hook to check if transaction should be executed.
/// Contains original request for Gas Stations `execute_tx` endpoint.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[schema(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTxHookRequest {
    pub execute_tx_request: ExecuteTxGasStationRequest,
}

/// Original request data and headers sent to Gas Stations `execute_tx` endpoint.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ExecuteTxGasStationRequest {
    pub payload: ExecuteTxRequestPayload,
    pub headers: HashMap<String, Vec<String>>,
}

/// Data originally sent to IOTA Gas Station.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[schema(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTxRequestPayload {
    /// ID used to reference a gas reservation.
    #[schema(format = "uint64")]
    pub reservation_id: u64,
    /// Transaction as base64 encoded BCS serialized `TransactionData`.
    #[schema(content_encoding = "base64")]
    pub tx_bytes: String,
    /// Base64 encoded user signature.
    #[schema(content_encoding = "base64")]
    pub user_sig: String,
}

impl ExecuteTxHookRequest {
    /// Helper function to allow accessing transaction data easily.
    pub fn parse_transaction_data(&self) -> Result<TransactionData, RequestError> {
        BASE64_STANDARD
            .decode(&self.execute_tx_request.payload.tx_bytes)
            .context("failed to decode base64 string with transaction data")
            .and_then(|bytes| {
                bcs::from_bytes(&bytes).context("failed to parse BCS bytes to `TransactionData`")
            })
            .map_err(|err| RequestError::new(err).with_status(StatusCode::BAD_REQUEST))
    }
}

/// Action that should be performed by Gas Station.
///
/// "allow"/"deny" transaction or take "noDecision" and proceed with other rules.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[schema(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub enum SkippableDecision {
    Allow,
    Deny,
    NoDecision,
}

/// Result of checking if transaction should be executed.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[schema(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTxOkResponse {
    /// Hooks decision about transaction execution.
    decision: SkippableDecision,
    /// Message intended to be forwarded to caller.
    #[serde(skip_serializing_if = "Option::is_none")]
    user_message: Option<String>,
}

impl ExecuteTxOkResponse {
    pub fn new(decision: SkippableDecision) -> Self {
        Self {
            decision,
            user_message: None,
        }
    }

    pub fn with_message(mut self, message: &str) -> Self {
        self.user_message = Some(message.to_string());
        self
    }
}

/// Response for failed requests.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[schema(rename_all = "camelCase")]
pub struct ErrorResponse {
    /// Technical error description.
    error: String,
    /// Message intended to be forwarded to caller.
    #[serde(skip_serializing_if = "Option::is_none")]
    user_message: Option<String>,
}

impl ErrorResponse {
    pub fn new(error: String, user_message: Option<String>) -> Self {
        Self {
            error,
            user_message,
        }
    }
}
