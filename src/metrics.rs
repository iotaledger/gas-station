// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{http::StatusCode, routing::get, Extension, Router};
use prometheus::{
    register_histogram_with_registry, register_int_counter_vec_with_registry,
    register_int_counter_with_registry, register_int_gauge_vec_with_registry, Histogram,
    IntCounter, IntCounterVec, IntGaugeVec, Registry, TextEncoder,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::error;

/// HTTP path metrics are served on.
pub const METRICS_ROUTE: &str = "/metrics";

/// Millisecond-scale buckets for RPC/transaction latency histograms.
/// Covers sub-millisecond blips up to a 30s worst case (network retries,
/// slow full nodes), which comfortably spans reserve/sign/execute latencies.
fn latency_ms_buckets() -> Vec<f64> {
    vec![
        1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1_000.0, 2_500.0, 5_000.0, 10_000.0,
        30_000.0,
    ]
}

/// Buckets (in seconds) for the client-requested gas reservation duration.
/// `reserve_duration_secs` is hard-capped at `RESERVE_GAS_MAX_DURATION_S`
/// (600s / 10 minutes), so the buckets are chosen to resolve that whole range.
fn reserve_duration_secs_buckets() -> Vec<f64> {
    vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0]
}

/// Buckets for NANOS-denominated gas amounts (gas budgets, gas usage, and
/// deltas thereof). 1 IOTA == 1_000_000_000 NANOS; this spans from a
/// negligible 1_000 NANOS up to 10 IOTA to cover both small transactions and
/// large, operator-configured gas budgets.
fn nanos_buckets() -> Vec<f64> {
    vec![
        1_000.0,
        10_000.0,
        100_000.0,
        1_000_000.0,
        10_000_000.0,
        100_000_000.0,
        1_000_000_000.0,
        10_000_000_000.0,
    ]
}

/// Buckets for small non-negative integer counts, such as the number of gas
/// coins involved in a single request.
fn small_count_buckets() -> Vec<f64> {
    vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0]
}

/// Builds an axum [`Router`] that serves `registry`'s metrics as Prometheus
/// text format at [`METRICS_ROUTE`], replacing the router that used to be set
/// up internally by `iota_metrics::start_prometheus_server`.
pub fn metrics_router(registry: Registry) -> Router {
    Router::new()
        .route(METRICS_ROUTE, get(metrics_handler))
        .layer(Extension(registry))
}

async fn metrics_handler(Extension(registry): Extension<Registry>) -> (StatusCode, String) {
    let metric_families = registry.gather();
    match TextEncoder::new().encode_to_string(&metric_families) {
        Ok(metrics) => (StatusCode::OK, metrics),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unable to encode metrics: {err}"),
        ),
    }
}

/// Creates a fresh [`Registry`] and spawns an axum HTTP server bound to
/// `addr` that serves it at [`METRICS_ROUTE`], mirroring the behavior of the
/// now-removed `iota_metrics::start_prometheus_server`. Returns the
/// [`Registry`] so callers can register metrics against it, exactly as they
/// previously did with `RegistryService::default_registry()`.
pub fn start_prometheus_server(addr: SocketAddr) -> Registry {
    let registry = Registry::new();
    let app = metrics_router(registry.clone());

    tokio::spawn(async move {
        if let Err(err) = axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
        {
            error!("metrics server error: {err}");
        }
    });

    registry
}

pub struct GasStationRpcMetrics {
    // === RPC Server Metrics ===
    // RPC metrics for the reserve_gas endpoint
    pub num_reserve_gas_requests: IntCounter,
    pub num_authorized_reserve_gas_requests: IntCounter,
    pub num_successful_reserve_gas_requests: IntCounter,
    pub num_failed_reserve_gas_requests: IntCounter,

    // Statistics about the gas reservation request
    pub target_gas_budget_per_request: Histogram,
    pub reserve_duration_per_request: Histogram,

    // RPC metrics for the execute_tx endpoint
    pub num_execute_tx_requests: IntCounter,
    pub num_authorized_execute_tx_requests: IntCounter,
    pub num_successful_execute_tx_requests: IntCounter,
    pub num_failed_execute_tx_requests: IntCounter,

    /// Access controller metrics
    pub num_allowed_execute_tx_requests: IntCounter,
    pub num_blocked_execute_tx_requests: IntCounter,
}

impl GasStationRpcMetrics {
    pub fn new(registry: &Registry) -> Arc<Self> {
        Arc::new(Self {
            num_reserve_gas_requests: register_int_counter_with_registry!(
                "num_reserve_gas_requests",
                "Total number of reserve_gas RPC requests received",
                registry,
            )
            .unwrap(),
            num_authorized_reserve_gas_requests: register_int_counter_with_registry!(
                "num_authorized_reserve_gas_requests",
                "Total number of reserve_gas RPC requests that provided the correct auth token",
                registry,
            )
            .unwrap(),
            num_successful_reserve_gas_requests: register_int_counter_with_registry!(
                "num_successful_reserve_gas_requests",
                "Total number of reserve_gas RPC requests that were successful",
                registry,
            )
            .unwrap(),
            num_failed_reserve_gas_requests: register_int_counter_with_registry!(
                "num_failed_reserve_gas_requests",
                "Total number of reserve_gas RPC requests that failed",
                registry,
            )
            .unwrap(),
            // NANOS-denominated gas budget requested by the caller.
            target_gas_budget_per_request: register_histogram_with_registry!(
                "target_gas_budget_per_request",
                "Target gas budget value in the reserve_gas RPC request",
                nanos_buckets(),
                registry,
            )
            .unwrap(),
            // Caller-requested reservation duration, in seconds (capped at 600s).
            reserve_duration_per_request: register_histogram_with_registry!(
                "reserve_duration_per_request",
                "Reserve duration value in the reserve_gas RPC request",
                reserve_duration_secs_buckets(),
                registry,
            )
            .unwrap(),
            num_execute_tx_requests: register_int_counter_with_registry!(
                "num_execute_tx_requests",
                "Total number of execute_tx RPC requests received",
                registry,
            )
            .unwrap(),
            num_authorized_execute_tx_requests: register_int_counter_with_registry!(
                "num_authorized_execute_tx_requests",
                "Total number of execute_tx RPC requests that provided the correct auth token",
                registry,
            )
            .unwrap(),
            num_successful_execute_tx_requests: register_int_counter_with_registry!(
                "num_successful_execute_tx_requests",
                "Total number of execute_tx RPC requests that were successful",
                registry,
            )
            .unwrap(),
            num_failed_execute_tx_requests: register_int_counter_with_registry!(
                "num_failed_execute_tx_requests",
                "Total number of execute_tx RPC requests that failed",
                registry,
            )
            .unwrap(),
            num_allowed_execute_tx_requests: register_int_counter_with_registry!(
                "num_allowed_execute_tx_requests",
                "Total number execute_tx RPC requests allowed by the Access Controller",
                registry,
            )
            .unwrap(),
            num_blocked_execute_tx_requests: register_int_counter_with_registry!(
                "num_blocked_execute_tx_requests",
                "Total number execute_tx RPC requests blocked by the Access Controller",
                registry,
            )
            .unwrap(),
        })
    }

    pub fn new_for_testing() -> Arc<Self> {
        Self::new(&Registry::new())
    }
}

pub struct GasStationCoreMetrics {
    pub num_expired_gas_coins: IntCounterVec,
    pub num_smashed_gas_coins: IntCounterVec,
    pub reserved_gas_coin_count_per_request: Histogram,
    pub reserve_gas_latency_ms: Histogram,
    pub transaction_signing_latency_ms: Histogram,
    pub transaction_execution_latency_ms: Histogram,
    pub num_gas_station_invariant_violations: IntCounter,
    pub daily_gas_usage: IntGaugeVec,
    pub oversized_gas_coins_count: IntCounterVec,
    pub gas_usage_per_transaction: Histogram,
    pub reserved_gas_real_gas_usage_delta: Histogram,
}

impl GasStationCoreMetrics {
    pub fn new(registry: &Registry) -> Arc<Self> {
        Arc::new(Self {
            // Count of gas coins reserved per request, not a NANOS/latency domain.
            reserved_gas_coin_count_per_request: register_histogram_with_registry!(
                "reserved_gas_coin_count_per_request",
                "Number of gas coins reserved in each reserve_gas RPC request",
                small_count_buckets(),
                registry,
            )
            .unwrap(),
            num_expired_gas_coins: register_int_counter_vec_with_registry!(
                "num_expired_gas_coins",
                "Total number of gas coins that are put back due to reservation expiration",
                &["sponsor"],
                registry,
            )
                .unwrap(),
            num_smashed_gas_coins: register_int_counter_vec_with_registry!(
                "num_smashed_gas_coins",
                "Total number of gas coins that are smashed (i.e. deleted) during transaction execution",
                &["sponsor"],
                registry,
            )
                .unwrap(),
            reserve_gas_latency_ms: register_histogram_with_registry!(
                "reserve_gas_latency",
                "Latency of gas reservation, in milliseconds",
                latency_ms_buckets(),
                registry,
            )
            .unwrap(),
            transaction_signing_latency_ms: register_histogram_with_registry!(
                "transaction_signing_latency",
                "Latency of transaction signing, in milliseconds",
                latency_ms_buckets(),
                registry,
            )
            .unwrap(),
            transaction_execution_latency_ms: register_histogram_with_registry!(
                "transaction_execution_latency",
                "Latency of transaction execution, in milliseconds",
                latency_ms_buckets(),
                registry,
            )
            .unwrap(),
            num_gas_station_invariant_violations: register_int_counter_with_registry!(
                "num_gas_station_invariant_violations",
                "Total number of invariant violations in the Gas Station core",
                registry,
            )
                .unwrap(),
            daily_gas_usage: register_int_gauge_vec_with_registry!(
                "daily_gas_usage",
                "Current daily gas usage",
                &["sponsor"],
                registry,
            )
                .unwrap(),
            oversized_gas_coins_count: register_int_counter_vec_with_registry!(
                "oversized_gas_coins_count",
                "Total number of oversized gas coins",
                &["sponsor"],
                registry,
            )
                .unwrap(),
            // Actual (post-execution) net gas usage, NANOS-denominated.
            gas_usage_per_transaction: register_histogram_with_registry!(
                "gas_usage_per_transaction",
                "Gas usage per transaction",
                nanos_buckets(),
                registry,
            )
            .unwrap(),
            // Delta between the coin balance reserved and the real gas usage, also NANOS.
            reserved_gas_real_gas_usage_delta: register_histogram_with_registry!(
                "reserved_gas_real_gas_usage_delta",
                "Reserved gas vs real gas usage delta",
                nanos_buckets(),
                registry,
            )
            .unwrap(),
        })
    }

    pub fn new_for_testing() -> Arc<Self> {
        Self::new(&Registry::new())
    }

    pub fn invariant_violation<T: Into<String>>(&self, msg: T) {
        if cfg!(debug_assertions) {
            panic!("Invariant violation: {}", msg.into());
        } else {
            error!("Invariant violation: {}", msg.into());
        }
        self.num_gas_station_invariant_violations.inc();
    }
}

pub struct StorageMetrics {
    pub gas_station_available_gas_coin_count: IntGaugeVec,
    pub gas_station_available_gas_total_balance: IntGaugeVec,

    pub num_reserve_gas_coins_requests: IntCounter,
    pub num_successful_reserve_gas_coins_requests: IntCounter,
    pub num_ready_for_execution_requests: IntCounter,
    pub num_successful_ready_for_execution_requests: IntCounter,
    pub num_add_new_coins_requests: IntCounter,
    pub num_successful_add_new_coins_requests: IntCounter,
    pub num_expire_coins_requests: IntCounter,
    pub num_successful_expire_coins_requests: IntCounter,
}

impl StorageMetrics {
    pub fn new(registry: &Registry) -> Arc<Self> {
        Arc::new(Self {
            gas_station_available_gas_coin_count: register_int_gauge_vec_with_registry!(
                "gas_station_available_gas_coin_count",
                "Current number of available gas coins for reservation",
                &["sponsor"],
                registry,
            )
            .unwrap(),
            gas_station_available_gas_total_balance: register_int_gauge_vec_with_registry!(
                "gas_station_available_gas_total_balance",
                "Current total balance of available gas coins for reservation",
                &["sponsor"],
                registry,
            )
            .unwrap(),
            num_reserve_gas_coins_requests: register_int_counter_with_registry!(
                "num_reserve_gas_coins_requests",
                "Total number of reserve_gas_coins requests received",
                registry,
            )
            .unwrap(),
            num_successful_reserve_gas_coins_requests: register_int_counter_with_registry!(
                "num_successful_reserve_gas_coins_requests",
                "Total number of reserve_gas_coins requests that were successful",
                registry,
            )
            .unwrap(),
            num_ready_for_execution_requests: register_int_counter_with_registry!(
                "num_ready_for_execution_requests",
                "Total number of ready_for_execution requests received",
                registry,
            )
            .unwrap(),
            num_successful_ready_for_execution_requests: register_int_counter_with_registry!(
                "num_successful_ready_for_execution_requests",
                "Total number of ready_for_execution requests that were successful",
                registry,
            )
            .unwrap(),
            num_add_new_coins_requests: register_int_counter_with_registry!(
                "num_add_new_coins_requests",
                "Total number of add_new_coins requests received",
                registry,
            )
            .unwrap(),
            num_successful_add_new_coins_requests: register_int_counter_with_registry!(
                "num_successful_add_new_coins_requests",
                "Total number of add_new_coins requests that were successful",
                registry,
            )
            .unwrap(),
            num_expire_coins_requests: register_int_counter_with_registry!(
                "num_expire_coins_requests",
                "Total number of expire_coins requests received",
                registry,
            )
            .unwrap(),
            num_successful_expire_coins_requests: register_int_counter_with_registry!(
                "num_successful_expire_coins_requests",
                "Total number of expire_coins requests that were successful",
                registry,
            )
            .unwrap(),
        })
    }

    pub fn new_for_testing() -> Arc<Self> {
        Self::new(&Registry::new())
    }
}
