// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::config::GasStationConfig;
use crate::gas_station::gas_station_core::GasStationContainer;
use crate::gas_station_initializer::GasStationInitializer;
use crate::iota_client::IotaClient;
use crate::metrics::{GasStationCoreMetrics, GasStationRpcMetrics, StorageMetrics};
use crate::rpc::GasStationServer;
use crate::storage::connect_storage;
use crate::{TRANSACTION_LOGGING_ENV_NAME, TRANSACTION_LOGGING_TARGET_NAME, VERSION};
use clap::*;
use iota_config::Config;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

#[derive(Parser)]
#[command(
    name = "iota-gas-station",
    about = "Iota Gas Station ",
    version = VERSION,
    rename_all = "kebab-case"
)]
pub struct Command {
    #[arg(env, long, help = "Path to config file")]
    config_path: PathBuf,
}

impl Command {
    pub async fn execute(self) {
        let config: GasStationConfig = GasStationConfig::load(self.config_path).unwrap();
        let GasStationConfig {
            signer_config,
            storage_config: gas_station_config,
            fullnode_url,
            fullnode_basic_auth,
            rpc_host_ip,
            rpc_port,
            metrics_port,
            coin_init_config,
            daily_gas_usage_cap,
            access_controller,
        } = config;

        let metric_address = SocketAddr::new(IpAddr::V4(rpc_host_ip), metrics_port);
        let registry_service = iota_metrics::start_prometheus_server(metric_address);
        let prometheus_registry = registry_service.default_registry();
        let mut telemetry_config = telemetry_subscribers::TelemetryConfig::new()
            .with_log_level("off,iota_gas_station=debug")
            .with_env()
            .with_prom_registry(&prometheus_registry);

        if std::env::var(TRANSACTION_LOGGING_ENV_NAME) == Ok("true".to_string()) {
            telemetry_config = telemetry_config.with_trace_target(TRANSACTION_LOGGING_TARGET_NAME);
        }
        let _guard = telemetry_config.init();
        info!("Metrics server started at {:?}", metric_address);

        let signer = signer_config.new_signer().await;
        let storage_metrics = StorageMetrics::new(&prometheus_registry);
        let sponsor_address = signer.get_address();
        info!("Sponsor address: {:?}", sponsor_address);

        let storage = connect_storage(&gas_station_config, sponsor_address, storage_metrics).await;
        let iota_client = IotaClient::new(&fullnode_url, fullnode_basic_auth).await;
        let _coin_init_task = if let Some(coin_init_config) = coin_init_config {
            let task = GasStationInitializer::start(
                iota_client.clone(),
                storage.clone(),
                coin_init_config,
                signer.clone(),
            )
            .await;
            Some(task)
        } else {
            None
        };

        let core_metrics = GasStationCoreMetrics::new(&prometheus_registry);
        let container = GasStationContainer::new(
            signer,
            storage,
            iota_client,
            daily_gas_usage_cap,
            core_metrics,
        )
        .await;

        let rpc_metrics = GasStationRpcMetrics::new(&prometheus_registry);

        let access_controller = Arc::new(access_controller);

        let server = GasStationServer::new(
            container.get_gas_station_arc(),
            rpc_host_ip,
            rpc_port,
            rpc_metrics,
            access_controller,
        )
        .await;
        server.handle.await.unwrap();
    }
}
