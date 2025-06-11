// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;

use crate::endpoint_types::ErrorResponse;

// Based on [axum example](https://github.com/tokio-rs/axum/blob/main/examples/anyhow-error-response/src/main.rs).

/// Error that is converted to HTTP error response.
pub struct RequestError {
    error: anyhow::Error,
    status: Option<StatusCode>,
    user_message: Option<String>,
}

impl RequestError {
    pub fn new(error: anyhow::Error) -> Self {
        Self {
            error,
            status: None,
            user_message: None,
        }
    }

    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_user_message(mut self, user_message: &str) -> Self {
        self.user_message = Some(user_message.to_string());
        self
    }
}

/// Tell axum how to convert `AppError` into a response.
impl IntoResponse for RequestError {
    fn into_response(self) -> Response {
        (
            self.status.unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(ErrorResponse::new(
                self.error.to_string(),
                self.user_message,
            )),
        )
            .into_response()
    }
}

/// This enables using `?` on functions that return `Result<_, anyhow::Error>` to turn them into
/// `Result<_, AppError>`. That way you don't need to do that manually.
///
/// using `?` will return an `INTERNAL_SERVER_ERROR` by default`
impl<E> From<E> for RequestError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self::new(err.into())
    }
}
