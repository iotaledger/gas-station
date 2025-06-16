// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use axum::Json;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::RequestError;
use crate::endpoint_types::ErrorResponse;
use crate::endpoint_types::ExecuteTxHookRequest;
use crate::endpoint_types::ExecuteTxOkResponse;
use crate::endpoint_types::SkippableDecision;

pub const TEST_ERROR_HEADER: &str = "test-error";
pub const TEST_RESPONSE_HEADER: &str = "test-response";

fn header_map_to_hash_map(headers: &HeaderMap) -> HashMap<String, Vec<String>> {
    let mut header_hashmap: HashMap<String, Vec<String>> = HashMap::new();
    for (k, v) in headers.clone() {
        let k = k.map(|v| v.to_string()).unwrap_or_default();
        let v = String::from_utf8_lossy(v.as_bytes()).into_owned();
        header_hashmap.entry(k).or_insert_with(Vec::new).push(v);
    }

    header_hashmap
}

fn get_test_response(
    headers: &HashMap<String, Vec<String>>,
) -> Result<Option<ExecuteTxOkResponse>, RequestError> {
    if let Some(test_error) = headers.get(TEST_ERROR_HEADER) {
        let test_error_message = test_error.first().ok_or_else(|| {
            RequestError::new(anyhow::anyhow!(
                "no value given for {TEST_ERROR_HEADER} header"
            ))
            .with_status(StatusCode::BAD_REQUEST)
            .with_user_message(&format!("no value given for {TEST_ERROR_HEADER} header"))
        })?;

        return Err(
            RequestError::new(anyhow::anyhow!("test error: {test_error_message}"))
                .with_status(StatusCode::BAD_REQUEST)
                .with_user_message(test_error_message),
        );
    }

    if let Some(test_response) = headers.get(TEST_RESPONSE_HEADER) {
        let test_response_raw = test_response.first().ok_or_else(|| {
            RequestError::new(anyhow::anyhow!(
                "no value given for {TEST_RESPONSE_HEADER} header"
            ))
            .with_status(StatusCode::BAD_REQUEST)
            .with_user_message(&format!("no value given for {TEST_RESPONSE_HEADER} header"))
        })?;
        let test_response: ExecuteTxOkResponse =
            serde_json::from_str(test_response_raw).map_err(|err| {
                RequestError::new(err.into())
                    .with_status(StatusCode::BAD_REQUEST)
                    .with_user_message("invalid request header")
            })?;

        return Ok(Some(test_response));
    }

    Ok(None)
}

/// Get router for access controller endpoint
pub fn router() -> OpenApiRouter {
    OpenApiRouter::new().routes(routes!(execute_tx))
}

/// Check if a transaction should be executed.
///
/// This is done when gas was already reserved and a caller now wants to initiate
/// the actual transaction execution.
///
/// Implementation here always returns `deny` and has to be adjusted depending on requirements.
#[utoipa::path(
    post,
    path = "/",
    responses(
        (status = OK, body = ExecuteTxOkResponse),
        (status = "4XX", body = ErrorResponse, description = "issues related to request arguments"), 
        (status = "5XX", body = ErrorResponse, description = "issues during request processing")
    )
)]
async fn execute_tx(
    headers: HeaderMap,
    Json(tx_data): Json<ExecuteTxHookRequest>,
) -> Result<Json<ExecuteTxOkResponse>, RequestError> {
    // Parsed transaction data can be used to decide if transaction should be executed or not.
    let transaction_data = tx_data.parse_transaction_data()?;
    dbg!(&transaction_data);

    // As this is an example server, this server supports test headers
    // that contains the response or errors we will return from here.
    // Don't support these headers and behaviors on your production system. ;)

    // First check headers from request payload,
    if let Some(response) = get_test_response(&tx_data.execute_tx_request.headers)? {
        return Ok(Json(response));
    }
    // then check headers from request.
    if let Some(response) = get_test_response(&header_map_to_hash_map(&headers))? {
        return Ok(Json(response));
    }

    Ok(Json(
        ExecuteTxOkResponse::new(SkippableDecision::Deny)
            .with_message("denied transaction by default"),
    ))
}
