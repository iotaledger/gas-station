// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::config::cold_params::ColdParams;
use crate::config::GasStationConfig;
use crate::gas_station::gas_station_core::GasStationContainer;
use crate::gas_station::rescan_trigger::RescanGasObjectsTrigger;
use crate::gas_station_initializer::GasStationInitializer;
use crate::iota_client::IotaClient;
use crate::metrics::{GasStationCoreMetrics, GasStationRpcMetrics, StorageMetrics};
use crate::rpc::GasStationServer;
use crate::storage::{connect_storage, get_storage_namespace};
use crate::tracker::stats_tracker_storage::redis::connect_stats_storage;
use crate::tracker::StatsTracker;
use crate::{TRANSACTION_LOGGING_ENV_NAME, TRANSACTION_LOGGING_TARGET_NAME, VERSION};
use arc_swap::ArcSwap;
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
    #[arg(
        env,
        long,
        help = "Ignore initialization and maintenance locks. This is useful when some unexpected error happens during the initialization or maintenance process and you want to restart the gas station without waiting for the locks to expire.",
        default_value_t = false
    )]
    ignore_locks: bool,
    #[arg(
        env,
        short,
        long,
        help = "Allow reinitialization of the gas station when the target init balance parameter changes",
        default_value_t = false
    )]
    allow_reinit: bool,
    #[arg(
        env,
        short = 'd',
        long,
        help = "Delete the coin registry before starting the gas station. This will delete all data associated with the sponsor address.",
        default_value_t = false
    )]
    delete_coin_registry: bool,
}

impl Command {
    pub async fn execute(self) {
        let config = GasStationConfig::load(&self.config_path).expect("Failed to load config file");
        let cold_params = ColdParams::from_config(&config);

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
            max_gas_budget,
            mut access_controller,
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

        let namespace_prefix = get_storage_namespace(&fullnode_url, &sponsor_address);
        let storage = connect_storage(
            &gas_station_config,
            sponsor_address,
            &namespace_prefix,
            storage_metrics,
        )
        .await;
        let iota_client = IotaClient::new(&fullnode_url, fullnode_basic_auth).await;

        let mut rescan_config = RescanGasObjectsTrigger::new(
            coin_init_config
                .clone()
                .unwrap_or_default()
                .target_init_balance,
        );
        let rescan_trigger_receiver = rescan_config.create_receiver();

        let cold_params_changes = cold_params
            .check_if_changed(&storage, &get_cold_params_key(&namespace_prefix))
            .await
            .expect("failed to check cold params changes");

        let force_full_rescan = if !cold_params_changes.is_empty() {
            if !self.allow_reinit {
                panic!("Configuration changes requiring re-initialization detected: {} but automatic reinitialization is not allowed. Please restart the gas station with the --allow-reinit flag to allow full rescan.", cold_params_changes.join(", "));
            }
            info!(
                "Configuration changes requiring re-initialization detected: {}",
                cold_params_changes.join(", ")
            );
            true
        } else if self.delete_coin_registry && self.allow_reinit {
            info!("The coin registry will be deleted and a new initialization will be started");
            true
        } else {
            false
        };

        let _coin_init_task = if let Some(coin_init_config) = coin_init_config {
            let task = GasStationInitializer::start(
                iota_client.clone(),
                storage.clone(),
                coin_init_config,
                signer.clone(),
                rescan_trigger_receiver,
                force_full_rescan,
                self.ignore_locks,
            )
            .await;
            Some(task)
        } else {
            None
        };
        cold_params
            .save_to_storage(&storage, &get_cold_params_key(&namespace_prefix))
            .await
            .expect("failed to save cold params to storage");
        let core_metrics = GasStationCoreMetrics::new(&prometheus_registry);
        let stats_storage = connect_stats_storage(&gas_station_config, &namespace_prefix).await;
        let stats_tracker = StatsTracker::new(Arc::new(stats_storage));
        let container = GasStationContainer::new(
            signer,
            storage,
            iota_client,
            daily_gas_usage_cap,
            max_gas_budget,
            core_metrics,
            rescan_config,
        )
        .await;
        let rpc_metrics = GasStationRpcMetrics::new(&prometheus_registry);
        access_controller
            .initialize()
            .await
            .expect("Failed to initialize the access controller");
        info!(
            "Access controller initialized with {} rules",
            access_controller.rules.len()
        );
        let access_controller = Arc::new(ArcSwap::new(Arc::new(access_controller)));

        let server = GasStationServer::new(
            container.get_gas_station_arc(),
            rpc_host_ip,
            rpc_port,
            rpc_metrics,
            access_controller,
            stats_tracker,
            self.config_path.clone(),
        )
        .await;
        server.handle.await.unwrap();
    }
}

fn get_cold_params_key(namespace_prefix: &str) -> String {
    format!("{namespace_prefix}:cold_params")
}
