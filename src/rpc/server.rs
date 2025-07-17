// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::access_controller::decision::Decision;
use crate::access_controller::rule::TransactionContext;
use crate::access_controller::{AccessController, TransactionExecutionResult};
use crate::config::GasStationConfig;
use crate::errors::generate_event_id;
use crate::gas_station::gas_station_core::GasStation;
use crate::logging::TxLogMessage;
use crate::metrics::GasStationRpcMetrics;
use crate::rpc::client::GasStationRpcClient;
use crate::rpc::rpc_types::{
    ExecuteTxRequest, ExecuteTxResponse, GasStationResponse, ReserveGasRequest, ReserveGasResponse,
};
use crate::tracker::StatsTracker;
use crate::{read_auth_env, VERSION};
use arc_swap::ArcSwap;
use axum::headers::authorization::Bearer;
use axum::headers::Authorization;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Extension, Json, Router, TypedHeader};
use fastcrypto::encoding::Base64;
use iota_config::Config;
use iota_json_rpc_types::IotaTransactionBlockEffectsAPI;
use iota_types::crypto::ToFromBytes;
use iota_types::signature::GenericSignature;
use iota_types::transaction::TransactionData;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, trace, warn};

pub struct GasStationServer {
    pub handle: JoinHandle<()>,
    pub rpc_port: u16,
}

impl GasStationServer {
    pub async fn new(
        station: Arc<GasStation>,
        host_ip: Ipv4Addr,
        rpc_port: u16,
        metrics: Arc<GasStationRpcMetrics>,
        access_controller: Arc<ArcSwap<AccessController>>,
        stats_tracker: StatsTracker,
        config_path: PathBuf,
    ) -> Self {
        let state = ServerState::new(
            station,
            metrics,
            access_controller,
            stats_tracker,
            config_path,
        );
        if state.secret.is_none() {
            warn!(
                "⚠️  {} environment variable is not set. Authorization is disabled! ⚠️",
                crate::AUTH_ENV_NAME
            );
        }
        let app = Router::new()
            .route("/", get(health))
            .route("/version", get(version))
            .route("/debug_health_check", post(debug_health_check))
            .route("/v1/reserve_gas", post(reserve_gas))
            .route("/v1/execute_tx", post(execute_tx))
            .route(
                "/v1/reload_access_controller",
                get(reload_access_controller),
            )
            .layer(Extension(state));

        let address = SocketAddr::new(IpAddr::V4(host_ip), rpc_port);

        let handle = tokio::spawn(async move {
            info!("listening on {}", address);
            axum::Server::bind(&address)
                .serve(app.into_make_service())
                .await
                .unwrap();
        });
        Self { handle, rpc_port }
    }

    pub fn get_local_client(&self) -> GasStationRpcClient {
        GasStationRpcClient::new(format!("http://localhost:{}", self.rpc_port))
    }
}

#[derive(Clone)]
struct ServerState {
    gas_station: Arc<GasStation>,
    secret: Arc<Option<String>>,
    metrics: Arc<GasStationRpcMetrics>,
    access_controller: Arc<ArcSwap<AccessController>>,
    stats_tracker: StatsTracker,
    config_path: PathBuf,
}

impl ServerState {
    fn new(
        gas_station: Arc<GasStation>,
        metrics: Arc<GasStationRpcMetrics>,
        access_controller: Arc<ArcSwap<AccessController>>,
        stats_tracker: StatsTracker,
        config_path: PathBuf,
    ) -> Self {
        let secret = Arc::new(read_auth_env());
        Self {
            gas_station,
            secret,
            metrics,
            access_controller,
            stats_tracker,
            config_path,
        }
    }
}

async fn health() -> &'static str {
    info!("Received health request");
    "OK"
}

async fn version() -> &'static str {
    info!("Received version request");
    VERSION
}

async fn debug_health_check(
    authorization: Option<TypedHeader<Authorization<Bearer>>>,
    Extension(server): Extension<ServerState>,
) -> String {
    info!("Received debug_health_check request");
    if let Some(secret) = server.secret.as_ref() {
        let token = authorization.as_ref().map(|auth| auth.token());
        if token != Some(secret.as_str()) {
            return "Unauthorized".to_string();
        }
    }
    if let Err(err) = server.gas_station.debug_check_health().await {
        return format!("Failed to check health: {:?}", err);
    }
    "OK".to_string()
}

async fn reserve_gas(
    authorization: Option<TypedHeader<Authorization<Bearer>>>,
    Extension(server): Extension<ServerState>,
    Json(payload): Json<ReserveGasRequest>,
) -> impl IntoResponse {
    if let Some(secret) = server.secret.as_ref() {
        let token = authorization.as_ref().map(|auth| auth.token());
        if token != Some(secret.as_str()) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ReserveGasResponse::new_err(anyhow::anyhow!(
                    "Authorization token is required or invalid"
                ))),
            );
        }
    }
    server.metrics.num_authorized_reserve_gas_requests.inc();
    debug!("Received v1 reserve_gas request: {:?}", payload);
    if let Err(err) = payload.check_validity() {
        debug!("Invalid reserve_gas request: {:?}", err);
        return (
            StatusCode::BAD_REQUEST,
            Json(ReserveGasResponse::new_err(err)),
        );
    }
    let ReserveGasRequest {
        gas_budget,
        reserve_duration_secs,
    } = payload;
    server
        .metrics
        .target_gas_budget_per_request
        .observe(gas_budget);
    server
        .metrics
        .reserve_duration_per_request
        .observe(reserve_duration_secs);
    // Spawn a thread to process the request so that it will finish even when client drops the connection.
    tokio::task::spawn(reserve_gas_impl(
        server.gas_station.clone(),
        server.metrics.clone(),
        gas_budget,
        reserve_duration_secs,
    ))
    .await
    .unwrap_or_else(|err| {
        error!("Failed to spawn reserve_gas task: {:?}", err);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ReserveGasResponse::new_err(anyhow::anyhow!(
                "Failed to spawn reserve_gas task"
            ))),
        )
    })
}

async fn reserve_gas_impl(
    gas_station: Arc<GasStation>,
    metrics: Arc<GasStationRpcMetrics>,
    gas_budget: u64,
    reserve_duration_secs: u64,
) -> (StatusCode, Json<ReserveGasResponse>) {
    match gas_station
        .reserve_gas(gas_budget, Duration::from_secs(reserve_duration_secs))
        .await
    {
        Ok((sponsor, reservation_id, gas_coins)) => {
            info!(
                ?reservation_id,
                "Reserved gas coins with sponsor={:?}, budget={:?} and duration={:?}: {:?}",
                sponsor,
                gas_budget,
                reserve_duration_secs,
                gas_coins
            );
            metrics.num_successful_reserve_gas_requests.inc();
            let response = ReserveGasResponse::new_ok(sponsor, reservation_id, gas_coins);
            (StatusCode::OK, Json(response))
        }
        Err(err) => {
            error!("Failed to reserve gas: {:?}", err);
            metrics.num_failed_reserve_gas_requests.inc();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ReserveGasResponse::new_err(err)),
            )
        }
    }
}

async fn execute_tx(
    headers: HeaderMap,
    authorization: Option<TypedHeader<Authorization<Bearer>>>,
    Extension(server): Extension<ServerState>,
    Json(payload): Json<ExecuteTxRequest>,
) -> impl IntoResponse {
    server.metrics.num_execute_tx_requests.inc();
    if let Some(secret) = server.secret.as_ref() {
        let token = authorization.as_ref().map(|auth| auth.token());
        if token != Some(secret.as_str()) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ExecuteTxResponse::new_err(anyhow::anyhow!(
                    "Invalid authorization token"
                ))),
            );
        }
    }

    server.metrics.num_authorized_execute_tx_requests.inc();

    debug!("Received v1 execute_tx request: {:?}", payload);
    let ExecuteTxRequest {
        reservation_id,
        tx_bytes,
        user_sig: user_sig_raw,
        request_type,
    } = payload;
    let Ok((tx_data, user_sig)) = convert_tx_and_sig(tx_bytes.clone(), user_sig_raw.clone()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ExecuteTxResponse::new_err(anyhow::anyhow!(
                "Invalid bcs bytes for TransactionData"
            ))),
        );
    };

    // collect information about request and transaction
    let ctx = TransactionContext::new(
        &user_sig,
        &tx_data,
        server.stats_tracker.clone(),
        reservation_id,
        tx_bytes,
        user_sig_raw,
        request_type,
        headers,
    );

    // Spawn a thread to process the request so that it will finish even when client drops the connection.
    tokio::task::spawn(execute_tx_impl(
        server.gas_station.clone(),
        server.metrics.clone(),
        tx_data,
        user_sig,
        server.access_controller.clone(),
        ctx,
    ))
    .await
    .unwrap_or_else(|err| {
        error!("Failed to spawn execute_tx task: {:?}", err);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ExecuteTxResponse::new_err(anyhow::anyhow!(
                "Failed to spawn execute_tx task"
            ))),
        )
    })
}

async fn execute_tx_impl(
    gas_station: Arc<GasStation>,
    metrics: Arc<GasStationRpcMetrics>,
    tx_data: TransactionData,
    user_sig: GenericSignature,
    access_controller: Arc<ArcSwap<AccessController>>,
    ctx: TransactionContext,
) -> (StatusCode, Json<ExecuteTxResponse>) {
    match access_controller.load().check_access(&ctx).await {
        Ok(Decision::Allow) => {
            metrics.num_allowed_execute_tx_requests.inc();
        }
        Ok(Decision::Deny) => {
            metrics.num_failed_execute_tx_requests.inc();
            return (
                StatusCode::FORBIDDEN,
                Json(ExecuteTxResponse::new_err(anyhow::anyhow!(
                    "Access denied by access controller"
                ))),
            );
        }
        Err(err) => {
            let event_id = generate_event_id();
            warn!(
                "EventId={} Error while checking access: {:?}",
                event_id, err
            );
            return (
                StatusCode::BAD_REQUEST,
                Json(ExecuteTxResponse::new_err(anyhow::anyhow!(
                    "Error while checking access. EventId={}",
                    event_id
                ))),
            );
        }
    }

    let transaction_digest = tx_data.digest();
    match gas_station
        .execute_transaction(ctx.reservation_id, tx_data, user_sig, ctx.request_type)
        .await
    {
        Ok(effects) => {
            info!(
                ?ctx.reservation_id,
                "Successfully executed transaction {:?} with status: {:?}",
                effects.transaction_digest(),
                effects.status()
            );
            trace!(target: "transactions", "{}", TxLogMessage::new(&effects));

            metrics.num_successful_execute_tx_requests.inc();
            let confirmation_result = access_controller
                .load()
                .confirm_transaction(
                    TransactionExecutionResult::new(transaction_digest)
                        .with_gas_usage(effects.gas_cost_summary().gas_used()),
                    &ctx.stats_tracker.clone(),
                )
                .await;
            // When then confirmation fails, the error shouldn't prevent the user from
            // receiving the successful response.
            if let Err(err) = confirmation_result {
                error!("Error while confirming transaction in AC: {:?}", err);
            }

            (StatusCode::OK, Json(ExecuteTxResponse::new_ok(effects)))
        }
        Err(err) => {
            error!("Failed to execute transaction: {:?}", err);

            let confirmation_result = access_controller
                .load()
                .confirm_transaction(
                    TransactionExecutionResult::new(transaction_digest),
                    &ctx.stats_tracker,
                )
                .await;
            if let Err(err) = confirmation_result {
                error!("Error while canceling transaction in AC: {:?}", err);
            }

            metrics.num_failed_execute_tx_requests.inc();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ExecuteTxResponse::new_err(err)),
            )
        }
    }
}

async fn reload_access_controller(
    authorization: Option<TypedHeader<Authorization<Bearer>>>,
    Extension(server): Extension<ServerState>,
) -> impl IntoResponse {
    if let Some(secret) = server.secret.as_ref() {
        let token = authorization.as_ref().map(|auth| auth.token());
        if token != Some(secret.as_str()) {
            return (
                StatusCode::FORBIDDEN,
                Json(GasStationResponse::new_err_from_str(
                    "Invalid authorization token",
                )),
            );
        }
    }
    let mut access_controller = match GasStationConfig::load(&server.config_path) {
        Ok(new_config) => new_config.access_controller,
        Err(err) => {
            error!("Failed to load config file: {:?}", err);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GasStationResponse::new_err_from_str(
                    "Failed to load config file",
                )),
            );
        }
    };
    let result = access_controller.initialize().await;
    if let Err(err) = result {
        error!("Failed to initialize access controller: {:?}", err);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(GasStationResponse::new_err(err)),
        );
    }
    server.access_controller.store(Arc::new(access_controller));
    info!(
        "Access controller reloaded successfully with {} rules",
        server.access_controller.load().rules.len()
    );
    return (StatusCode::OK, Json(GasStationResponse::new_ok("success")));
}

fn convert_tx_and_sig(
    tx_bytes: Base64,
    user_sig: Base64,
) -> anyhow::Result<(TransactionData, GenericSignature)> {
    let tx = bcs::from_bytes(
        &tx_bytes
            .to_vec()
            .map_err(|_| anyhow::anyhow!("Failed to convert tx_bytes to vector"))?,
    )?;
    let user_sig = GenericSignature::from_bytes(
        &user_sig
            .to_vec()
            .map_err(|_| anyhow::anyhow!("Failed to convert user_sig to vector"))?,
    )?;
    Ok((tx, user_sig))
}
