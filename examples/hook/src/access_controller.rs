use axum::Json;
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::RequestError;
use crate::endpoint_types::Action;
use crate::endpoint_types::ErrorResponse;
use crate::endpoint_types::ExecuteTxCheckRequest;
use crate::endpoint_types::ExecuteTxOkResponse;

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
    Json(tx_data): Json<ExecuteTxCheckRequest>,
) -> Result<Json<ExecuteTxOkResponse>, RequestError> {
    let test = tx_data.parse_transaction_data()?;
    dbg!(&test);

    match tx_data.execute_tx_request.reservation_id {
        10 => {
            return Ok(Json(ExecuteTxOkResponse::new(Action::Allow)));
        }
        20 => {
            return Ok(Json(
                ExecuteTxOkResponse::new(Action::Deny).with_message("rate limit exceeded"),
            ));
        }
        30 => {
            return Ok(Json(ExecuteTxOkResponse::new(Action::NoAction)));
        }
        40 => {
            return Err(RequestError::new(anyhow::anyhow!("test error"))
                .with_status(StatusCode::IM_A_TEAPOT));
        }
        41 => {
            return Err(RequestError::new(anyhow::anyhow!("endpoint was a teapot"))
                .with_status(StatusCode::IM_A_TEAPOT)
                .with_user_message("Please stop talking to teapots. They usually don't answer."));
        }
        _ => (),
    }

    Ok(Json(ExecuteTxOkResponse::new(Action::Deny)))
}
