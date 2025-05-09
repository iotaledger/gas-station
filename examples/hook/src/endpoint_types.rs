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
pub struct ExecuteTxCheckRequest {
    #[schema(value_type = ExecuteTxRequestPayload)]
    pub execute_tx_request: ExecuteTxRequestPayload,
}

/// Data originally sent to IOTA Gas Station.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[schema(rename_all = "camelCase")]
pub struct ExecuteTxRequestPayload {
    /// ID used to reference a gas reservation.
    pub reservation_id: u64,
    /// Transaction as base64 encoded BCS serialized `TransactionData`.
    #[schema(content_encoding = "base64")]
    pub tx_bytes: String,
    /// Base64 encoded user signature.
    #[schema(content_encoding = "base64")]
    pub user_sig: String,
}

impl ExecuteTxCheckRequest {
    pub fn parse_transaction_data(&self) -> Result<TransactionData, RequestError> {
        BASE64_STANDARD
            .decode(&self.execute_tx_request.tx_bytes)
            .context("failed to decode base64 string with transaction data")
            .and_then(|bytes| {
                bcs::from_bytes(&bytes).context("failed to parse BCS bytes to `TransactionData`")
            })
            .map_err(|err| RequestError::new(err).with_status(StatusCode::BAD_REQUEST))
    }
}

/// Action that should be performed by Gas Station.
///
/// "allow"/"deny" transaction or take "noAction" and proceed with other rules.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[schema(rename_all = "camelCase")]
pub enum Action {
    Allow,
    Deny,
    NoAction,
}

/// Result of checking if transaction should be executed.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[schema(rename_all = "camelCase")]
pub struct ExecuteTxOkResponse {
    action: Action,
    /// Message intended to be forwarded to caller.
    #[serde(skip_serializing_if = "Option::is_none")]
    user_message: Option<String>,
}

impl ExecuteTxOkResponse {
    pub fn new(action: Action) -> Self {
        Self {
            action,
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
