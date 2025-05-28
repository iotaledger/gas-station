// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::access_controller::hook::{ExecuteTxOkResponse, SkippableDecision};
use crate::access_controller::rule::TransactionContext;

pub const TEST_ERROR_HEADER: &str = "test-error";
pub const TEST_RESPONSE_HEADER: &str = "test-response";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookAction(pub(crate) Url);

impl HookAction {
    /// Mock hook call by using serialized value in "test-response" header as hook "call" outcome.
    pub async fn call_hook(
        &self,
        ctx: &TransactionContext,
    ) -> Result<ExecuteTxOkResponse, anyhow::Error> {
        if let Some(header_value) = ctx.headers.get(TEST_ERROR_HEADER) {
            let error_message = String::from_utf8_lossy(header_value.as_bytes()).into_owned();

            anyhow::bail!(
                "hook call failed with status {}; {}",
                StatusCode::BAD_REQUEST,
                error_message
            );
        }

        if let Some(header_value) = ctx.headers.get(TEST_RESPONSE_HEADER) {
            let test_response_raw = String::from_utf8_lossy(header_value.as_bytes()).into_owned();
            let test_response: ExecuteTxOkResponse = serde_json::from_str(&test_response_raw)?;

            return Ok(test_response);
        }

        Ok(ExecuteTxOkResponse {
            decision: SkippableDecision::Deny,
            user_message: Some("denied transaction by default".to_string()),
        })
    }
}
