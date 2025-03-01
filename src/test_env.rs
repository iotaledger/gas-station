// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::access_controller::policy::AccessPolicy;
use crate::access_controller::AccessController;
use crate::config::{CoinInitConfig, DEFAULT_DAILY_GAS_USAGE_CAP};
use crate::gas_station::gas_station_core::GasStationContainer;
use crate::gas_station_initializer::GasStationInitializer;
use crate::iota_client::IotaClient;
use crate::metrics::{GasStationCoreMetrics, GasStationRpcMetrics};
use crate::rpc::GasStationServer;
use crate::storage::connect_storage_for_testing;
use crate::tx_signer::{TestTxSigner, TxSigner};
use crate::AUTH_ENV_NAME;
use iota_config::local_ip_utils::{get_available_port, localhost_for_testing};
use iota_swarm_config::genesis_config::AccountConfig;
use iota_types::base_types::{IotaAddress, ObjectRef};
use iota_types::crypto::get_account_key_pair;
use iota_types::gas_coin::NANOS_PER_IOTA;
use iota_types::signature::GenericSignature;
use iota_types::transaction::{TransactionData, TransactionDataAPI};
use std::sync::Arc;
use test_cluster::{TestCluster, TestClusterBuilder};
use tracing::debug;

pub async fn start_iota_cluster(init_gas_amounts: Vec<u64>) -> (TestCluster, Arc<dyn TxSigner>) {
    let (sponsor, keypair) = get_account_key_pair();
    let cluster = TestClusterBuilder::new()
        .with_accounts(vec![
            AccountConfig {
                address: Some(sponsor),
                gas_amounts: init_gas_amounts,
            },
            // Besides sponsor, also initialize another account with 1000 IOTA.
            AccountConfig {
                address: None,
                gas_amounts: vec![1000 * NANOS_PER_IOTA],
            },
        ])
        .build()
        .await;
    (cluster, TestTxSigner::new(keypair.into()))
}

pub async fn start_gas_station(
    init_gas_amounts: Vec<u64>,
    target_init_coin_balance: u64,
) -> (TestCluster, GasStationContainer) {
    debug!("Starting Iota cluster..");
    let (test_cluster, signer) = start_iota_cluster(init_gas_amounts).await;
    let fullnode_url = test_cluster.fullnode_handle.rpc_url.clone();
    let sponsor_address = signer.get_address();
    debug!("Starting storage. Sponsor address: {:?}", sponsor_address);
    let storage = connect_storage_for_testing(sponsor_address).await;
    let iota_client = IotaClient::new(&fullnode_url, None).await;
    GasStationInitializer::start(
        iota_client.clone(),
        storage.clone(),
        CoinInitConfig {
            target_init_balance: target_init_coin_balance,
            ..Default::default()
        },
        signer.clone(),
    )
    .await;
    let station = GasStationContainer::new(
        signer,
        storage,
        iota_client,
        DEFAULT_DAILY_GAS_USAGE_CAP,
        GasStationCoreMetrics::new_for_testing(),
    )
    .await;
    (test_cluster, station)
}

pub async fn start_rpc_server_for_testing(
    init_gas_amounts: Vec<u64>,
    target_init_balance: u64,
) -> (TestCluster, GasStationContainer, GasStationServer) {
    let (test_cluster, container) = start_gas_station(init_gas_amounts, target_init_balance).await;
    let localhost = localhost_for_testing();
    std::env::set_var(AUTH_ENV_NAME, "some secret");
    let server = GasStationServer::new(
        container.get_gas_station_arc(),
        localhost.parse().unwrap(),
        get_available_port(&localhost),
        GasStationRpcMetrics::new_for_testing(),
        Arc::new(AccessController::default()),
    )
    .await;
    (test_cluster, container, server)
}

pub async fn start_rpc_server_for_testing_with_access_ctrl_deny_all(
    init_gas_amounts: Vec<u64>,
    target_init_balance: u64,
) -> (TestCluster, GasStationContainer, GasStationServer) {
    let (test_cluster, container) = start_gas_station(init_gas_amounts, target_init_balance).await;
    let localhost = localhost_for_testing();
    std::env::set_var(AUTH_ENV_NAME, "some secret");
    let server = GasStationServer::new(
        container.get_gas_station_arc(),
        localhost.parse().unwrap(),
        get_available_port(&localhost),
        GasStationRpcMetrics::new_for_testing(),
        Arc::new(AccessController::new(AccessPolicy::DenyAll, [])),
    )
    .await;
    (test_cluster, container, server)
}

pub async fn create_test_transaction(
    test_cluster: &TestCluster,
    sponsor: IotaAddress,
    gas_coins: Vec<ObjectRef>,
) -> (TransactionData, GenericSignature) {
    let user = test_cluster
        .get_addresses()
        .into_iter()
        .find(|a| *a != sponsor)
        .unwrap();
    let object = test_cluster
        .wallet
        .get_one_gas_object_owned_by_address(user)
        .await
        .unwrap()
        .unwrap();
    let mut tx_data = test_cluster
        .test_transaction_builder_with_gas_object(user, gas_coins[0])
        .await
        .transfer(object, user)
        .build();
    // TODO: Add proper sponsored transaction support to test tx builder.
    tx_data.gas_data_mut().payment = gas_coins;
    tx_data.gas_data_mut().owner = sponsor;
    let user_sig = test_cluster
        .sign_transaction(&tx_data)
        .into_data()
        .tx_signatures_mut_for_testing()
        .pop()
        .unwrap();
    (tx_data, user_sig)
}
