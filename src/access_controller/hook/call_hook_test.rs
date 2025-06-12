// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::http::HeaderMap;
use reqwest::StatusCode;

use crate::access_controller::hook::HookAction;
use crate::access_controller::hook::{ExecuteTxOkResponse, SkippableDecision};
use crate::access_controller::rule::TransactionContext;

pub const TEST_ERROR_HEADER: &str = "test-error";
pub const TEST_RESPONSE_HEADER: &str = "test-response";

fn get_test_response(headers: &HeaderMap) -> Result<Option<ExecuteTxOkResponse>, anyhow::Error> {
    if let Some(header_value) = headers.get(TEST_ERROR_HEADER) {
        let error_message = String::from_utf8_lossy(header_value.as_bytes()).into_owned();

        anyhow::bail!(
            "hook call failed with status {}; {}",
            StatusCode::BAD_REQUEST,
            error_message
        );
    }

    if let Some(header_value) = headers.get(TEST_RESPONSE_HEADER) {
        let test_response_raw = String::from_utf8_lossy(header_value.as_bytes()).into_owned();
        let test_response: ExecuteTxOkResponse = serde_json::from_str(&test_response_raw)?;

        return Ok(Some(test_response));
    }

    Ok(None)
}

impl HookAction {
    /// Mock hook call by using serialized value in "test-response" header as hook "call" outcome.
    pub async fn call_hook(
        &self,
        ctx: &TransactionContext,
    ) -> Result<ExecuteTxOkResponse, anyhow::Error> {
        if let Some(response) = get_test_response(&ctx.headers)? {
            return Ok(response);
        }

        Ok(ExecuteTxOkResponse {
            decision: SkippableDecision::Deny,
            user_message: Some("denied transaction by default".to_string()),
        })
    }
}
