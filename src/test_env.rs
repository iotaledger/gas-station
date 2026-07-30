// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::access_controller::AccessController;
use crate::config::{
    CoinInitConfig, GasStationStorageConfig, DEFAULT_CHECKPOINT_WAIT_MS,
    DEFAULT_DAILY_GAS_USAGE_CAP, DEFAULT_MAX_GAS_BUDGET,
};
use crate::gas_station::gas_station_core::{GasStationContainer, NANOS_PER_IOTA};
use crate::gas_station::rescan_trigger::RescanGasObjectsTrigger;
use crate::gas_station_initializer::GasStationInitializer;
use crate::iota_client::IotaClient;
use crate::metrics::{GasStationCoreMetrics, GasStationRpcMetrics};
use crate::rpc::GasStationServer;
use crate::storage::connect_storage_for_testing;
use crate::tracker::stats_tracker_storage::redis::connect_stats_storage;
use crate::tracker::stats_tracker_storage::{self, StatsTrackerStorage};
use crate::tracker::StatsTracker;
use crate::tx_signer::{TestTxSigner, TxSigner};
use crate::AUTH_ENV_NAME;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use iota_sdk_crypto::ed25519::Ed25519PrivateKey;
use iota_sdk_crypto::simple::SimpleKeypair;
use iota_sdk_transaction_builder::TransactionBuilderClient;
use iota_sdk_types::{Address, ObjectReference, Transaction, UserSignature};
use iota_swarm_config::genesis_config::AccountConfig;
use redis::{Commands, FromRedisValue};
use serde_json::Value;
use std::net::TcpListener;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use test_cluster::{TestCluster, TestClusterBuilder};
use tracing::debug;

pub const DEFAULT_TEST_CONFIG_PATH: &str = "./test-env-config.yaml";

/// Loopback address the gas-station server's own test-only HTTP listener
/// binds to. There is no SDK equivalent of the old
/// `iota_config::local_ip_utils::localhost_for_testing` -- this is purely
/// this crate's own test infrastructure, not part of any wire format --
/// so it is just a plain constant now.
fn localhost_for_testing() -> String {
    "127.0.0.1".to_string()
}

/// Binds an OS-assigned ephemeral TCP port on `host` and immediately drops
/// the listener, returning the port number that was assigned. Same
/// bind-then-release approach (and the same TOCTOU race against another
/// caller grabbing the port before the caller of this function does) as the
/// old `iota_config::local_ip_utils::get_available_port` it replaces; that
/// crate is gone from our dependency tree, and the SDK has no equivalent
/// (it's not part of any wire format either), so this is a small local
/// helper instead.
fn get_available_port(host: &str) -> u16 {
    TcpListener::bind((host, 0))
        .expect("failed to bind to an OS-assigned port")
        .local_addr()
        .expect("failed to read the local address of a just-bound listener")
        .port()
}

pub async fn start_iota_cluster(init_gas_amounts: Vec<u64>) -> (TestCluster, Arc<dyn TxSigner>) {
    let keypair = SimpleKeypair::from(Ed25519PrivateKey::generate(rand::rngs::OsRng));
    let sponsor = keypair.public_key().derive_address();
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
    (cluster, TestTxSigner::new(keypair))
}

pub async fn start_gas_station(
    init_gas_amounts: Vec<u64>,
    target_init_coin_balance: u64,
    max_gas_budget: Option<u64>,
) -> (TestCluster, GasStationContainer) {
    debug!("Starting Iota cluster..");
    let (test_cluster, signer) = start_iota_cluster(init_gas_amounts).await;
    // On develop, `TestCluster` starts its fullnode's gRPC API by default and
    // self-allocates the port (see `TestClusterBuilder`'s
    // `fullnode_enable_grpc_api: true` default and
    // `node_config_builder.rs`'s `local_ip_utils::get_available_port` use) --
    // `grpc_url()` (on `TestCluster` itself, not `FullNodeHandle`) is all
    // that's needed to reach it, replacing the old JSON-RPC
    // `fullnode_handle.rpc_url`, since our own `IotaClient` now speaks gRPC.
    let grpc_url = test_cluster.grpc_url();
    let sponsor_address = signer.get_address();
    debug!("Starting storage. Sponsor address: {:?}", sponsor_address);
    let storage = connect_storage_for_testing(sponsor_address).await;
    let iota_client = IotaClient::new(&grpc_url, None)
        .await
        .expect("failed to connect to the test cluster's gRPC endpoint");
    let mut rescan_config = RescanGasObjectsTrigger::new(target_init_coin_balance);
    let rescan_trigger_receiver = rescan_config.create_receiver();
    GasStationInitializer::start(
        iota_client.clone(),
        storage.clone(),
        CoinInitConfig {
            target_init_balance: target_init_coin_balance,
            ..Default::default()
        },
        signer.clone(),
        rescan_trigger_receiver,
        false,
        false,
    )
    .await;
    let station = GasStationContainer::new(
        signer,
        storage,
        iota_client,
        DEFAULT_DAILY_GAS_USAGE_CAP,
        max_gas_budget.unwrap_or(DEFAULT_MAX_GAS_BUDGET),
        DEFAULT_CHECKPOINT_WAIT_MS,
        GasStationCoreMetrics::new_for_testing(),
        rescan_config,
    )
    .await;
    (test_cluster, station)
}

pub async fn start_rpc_server_for_testing(
    init_gas_amounts: Vec<u64>,
    target_init_balance: u64,
) -> (TestCluster, GasStationContainer, GasStationServer) {
    let (test_cluster, container) =
        start_gas_station(init_gas_amounts, target_init_balance, None).await;
    let localhost = localhost_for_testing();
    let signer_address = container.get_signer_address();
    std::env::set_var(AUTH_ENV_NAME, "some secret");

    let server = GasStationServer::new(
        container.get_gas_station_arc(),
        localhost.parse().unwrap(),
        get_available_port(&localhost),
        GasStationRpcMetrics::new_for_testing(),
        Arc::new(ArcSwap::new(Arc::new(AccessController::default()))),
        new_stats_tracker_for_testing(signer_address).await,
        PathBuf::from_str(DEFAULT_TEST_CONFIG_PATH).unwrap(),
    )
    .await;
    (test_cluster, container, server)
}

pub async fn start_rpc_server_for_testing_no_auth(
    init_gas_amounts: Vec<u64>,
    target_init_balance: u64,
) -> (TestCluster, GasStationContainer, GasStationServer) {
    let (test_cluster, container) =
        start_gas_station(init_gas_amounts, target_init_balance, None).await;
    let localhost = localhost_for_testing();
    let signer_address = container.get_signer_address();

    let server = GasStationServer::new(
        container.get_gas_station_arc(),
        localhost.parse().unwrap(),
        get_available_port(&localhost),
        GasStationRpcMetrics::new_for_testing(),
        Arc::new(ArcSwap::new(Arc::new(AccessController::default()))),
        new_stats_tracker_for_testing(signer_address).await,
        PathBuf::from_str(DEFAULT_TEST_CONFIG_PATH).unwrap(),
    )
    .await;
    (test_cluster, container, server)
}

pub async fn start_rpc_server_for_testing_empty_auth(
    init_gas_amounts: Vec<u64>,
    target_init_balance: u64,
) -> (TestCluster, GasStationContainer, GasStationServer) {
    let (test_cluster, container) =
        start_gas_station(init_gas_amounts, target_init_balance, None).await;
    let localhost = localhost_for_testing();
    let signer_address = container.get_signer_address();
    std::env::set_var(AUTH_ENV_NAME, "");

    let server = GasStationServer::new(
        container.get_gas_station_arc(),
        localhost.parse().unwrap(),
        get_available_port(&localhost),
        GasStationRpcMetrics::new_for_testing(),
        Arc::new(ArcSwap::new(Arc::new(AccessController::default()))),
        new_stats_tracker_for_testing(signer_address).await,
        PathBuf::from_str(DEFAULT_TEST_CONFIG_PATH).unwrap(),
    )
    .await;
    (test_cluster, container, server)
}

pub async fn start_rpc_server_for_testing_with_access_controller(
    init_gas_amounts: Vec<u64>,
    target_init_balance: u64,
    access_controller: AccessController,
) -> (TestCluster, GasStationContainer, GasStationServer) {
    let (test_cluster, container) =
        start_gas_station(init_gas_amounts, target_init_balance, None).await;
    let localhost = localhost_for_testing();
    let signer_address = container.get_signer_address();
    std::env::set_var(AUTH_ENV_NAME, "some secret");

    let server = GasStationServer::new(
        container.get_gas_station_arc(),
        localhost.parse().unwrap(),
        get_available_port(&localhost),
        GasStationRpcMetrics::new_for_testing(),
        Arc::new(ArcSwap::new(Arc::new(access_controller))),
        new_stats_tracker_for_testing(signer_address).await,
        PathBuf::from_str(DEFAULT_TEST_CONFIG_PATH).unwrap(),
    )
    .await;
    (test_cluster, container, server)
}

/// Builds a user-signed, sponsored transaction: `user` (any cluster address
/// other than `sponsor`) transfers one of their own objects to themselves --
/// the PTB itself doesn't need to do anything meaningful, it just needs to be
/// a valid transaction for the gas station to execute -- while `sponsor`
/// pays with `gas_coins`.
///
/// Unlike the pre-migration version of this helper, which built a plain
/// (unsponsored) transaction via the old `TestTransactionBuilder` and then
/// hand-patched `gas_data_mut().payment`/`.owner` onto it afterwards (see the
/// removed `TODO: Add proper sponsored transaction support to test tx
/// builder` comment that used to sit here), this uses the new SDK
/// transaction builder's first-class sponsor support
/// (`TransactionBuilder::sponsor`/`::gas`) directly -- no post-hoc patching
/// needed.
pub async fn create_test_transaction(
    test_cluster: &TestCluster,
    sponsor: Address,
    gas_coins: Vec<ObjectReference>,
) -> (Transaction, UserSignature) {
    let user = test_cluster
        .get_addresses()
        .into_iter()
        .find(|address| *address != sponsor)
        .expect("test cluster should have an address other than the sponsor");

    // Any object owned by `user` works as the PTB's payload; genesis funds
    // `user` with exactly one coin (see `start_iota_cluster`'s second
    // `AccountConfig`), so this always finds one.
    let object_id = test_cluster
        .grpc_client()
        .objects(None, user, None, Some(1))
        .await
        .expect("failed to list user's owned objects")
        .data
        .into_iter()
        .next()
        .expect("`user` should own at least one object from genesis")
        .id();

    // `TransactionBuilder::gas` is overloaded on whether a client is
    // attached: the *offline* (`C = ()`) overload takes full `ObjectReference`s,
    // but the client-attached one -- which is what `grpc_transaction_builder`
    // returns, and what's needed below for the `object_id` input to resolve --
    // only takes bare `ObjectId`s and re-resolves each one's current
    // version/digest from the network itself. So only the id is passed
    // through here; the resolved (version, digest) is intentionally read
    // live rather than trusted from the reservation.
    let mut builder = test_cluster.grpc_transaction_builder(user);
    builder
        .transfer_objects(user, [object_id])
        .sponsor(sponsor)
        .gas(gas_coins.into_iter().map(|coin| coin.object_id));

    // Gas price is left unset on purpose: with a client attached, `finish`
    // resolves it from the network's current reference gas price, which is
    // more accurate than hand-picking a constant here.
    builder.gas_budget(10_000_000);
    let tx = builder
        .finish()
        .await
        .expect("failed to resolve test transaction");

    // `TestCluster::sign_transaction` accepts our `Transaction` directly
    // (`TransactionData` is a re-export of it on develop) but returns the
    // monorepo's own signed-transaction envelope type, not
    // `iota_sdk_types::UserSignature` -- so the single user signature is
    // pulled back out of it here, mirroring what the pre-migration version
    // did with `tx_signatures_mut_for_testing().pop()`.
    let user_sig = test_cluster
        .sign_transaction(&tx)
        .into_data()
        .signatures()
        .first()
        .cloned()
        .expect("wallet should have produced exactly one signature");

    (tx, user_sig)
}

pub async fn new_stats_tracker_for_testing(sponsor_address: Address) -> StatsTracker {
    StatsTracker::new(Arc::new(
        connect_stats_storage(
            &GasStationStorageConfig::default(),
            sponsor_address.to_string().as_str(),
        )
        .await,
    ))
}

/// Constructs an arbitrary `iota_sdk_types::Address` for use in tests.
pub fn random_address() -> Address {
    Address::random()
}

struct MockedStatsTrackerStorage;

#[async_trait]
impl StatsTrackerStorage for MockedStatsTrackerStorage {
    async fn update_aggr(
        &self,
        _key_meta: &[(String, Value)],
        _update: &stats_tracker_storage::Aggregate,
        _value: i64,
    ) -> anyhow::Result<i64> {
        Ok(0)
    }
}

pub fn mocked_stats_tracker() -> StatsTracker {
    StatsTracker::new(Arc::new(MockedStatsTrackerStorage {}))
}

pub fn fetch_redis_val<T: FromRedisValue>(redis_key: &str) -> T {
    let default_redis_url = match GasStationStorageConfig::default() {
        GasStationStorageConfig::Redis { redis_url } => redis_url,
    };
    let redis_client = redis::Client::open(default_redis_url).unwrap();
    let mut redis_connection = redis_client.get_connection().unwrap();
    let value: T = redis_connection.get(redis_key).unwrap();
    value
}

pub fn remove_redis_key<K: FromRedisValue>(redis_key: &str) {
    let default_redis_url = match GasStationStorageConfig::default() {
        GasStationStorageConfig::Redis { redis_url } => redis_url,
    };
    let redis_client = redis::Client::open(default_redis_url).unwrap();
    let mut redis_connection = redis_client.get_connection().unwrap();
    redis_connection
        .del::<String, K>(redis_key.to_string())
        .unwrap();
}
