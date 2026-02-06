// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::config::CoinInitConfig;
use crate::consistency_check::{log_consistency_warnings, validate_consistency};
use crate::iota_client::IotaClient;
use crate::retry_forever;
use crate::storage::Storage;
use crate::tx_signer::TxSigner;
use crate::types::GasCoin;
use anyhow::bail;
use iota_json_rpc_types::IotaTransactionBlockEffectsAPI;
use iota_types::base_types::IotaAddress;
use iota_types::coin::{PAY_MODULE_NAME, PAY_SPLIT_N_FUNC_NAME};
use iota_types::gas_coin::GAS;
use iota_types::programmable_transaction_builder::ProgrammableTransactionBuilder;
use iota_types::transaction::{Argument, Transaction, TransactionData};
use iota_types::IOTA_FRAMEWORK_PACKAGE_ID;
use parking_lot::Mutex;
use std::cmp::min;
use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tap::TapFallible;
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};
use tracing::{debug, error, info};

/// Any coin owned by the sponsor address with balance above target_init_coin_balance * NEW_COIN_BALANCE_FACTOR_THRESHOLD
/// is considered a new coin, and we will try to split it into smaller coins with balance close to target_init_coin_balance.
pub const NEW_COIN_BALANCE_FACTOR_THRESHOLD: u64 = 200;

/// Assume that initializing the Gas Station (i.e. splitting coins) will take at most 12 hours.
const MAX_INIT_DURATION_SEC: u64 = 60 * 60 * 12;

/// Maintenance mode duration should be long enough to complete the full rescan procedure.
/// This includes cleaning up the coin registry and rescanning all coins.
const MAX_MAINTENANCE_DURATION_SEC: u64 = 60 * 60 * 12;

#[derive(Clone)]
struct CoinSplitEnv {
    target_init_coin_balance: u64,
    gas_cost_per_object: u64,
    signer: Arc<dyn TxSigner>,
    sponsor_address: IotaAddress,
    iota_client: IotaClient,
    task_queue: Arc<Mutex<VecDeque<JoinHandle<Vec<GasCoin>>>>>,
    total_coin_count: Arc<AtomicUsize>,
    rgp: u64,
}

impl CoinSplitEnv {
    fn enqueue_task(&self, coin: GasCoin) -> Option<GasCoin> {
        if coin.balance <= (self.gas_cost_per_object + self.target_init_coin_balance) * 2 {
            debug!(
                "Skip splitting coin {:?} because it has small balance",
                coin
            );
            return Some(coin);
        }
        let env = self.clone();
        let task = tokio::task::spawn(async move { env.split_one_gas_coin(coin).await });
        self.task_queue.lock().push_back(task);
        None
    }

    fn increment_total_coin_count_by(&self, delta: usize) {
        info!(
            "Number of coins got so far: {}",
            self.total_coin_count
                .fetch_add(delta, std::sync::atomic::Ordering::Relaxed)
                + delta
        );
    }

    async fn split_one_gas_coin(self, mut coin: GasCoin) -> Vec<GasCoin> {
        let rgp = self.rgp;
        let split_count = min(
            // Max number of object mutations per transaction is 2048.
            2000,
            coin.balance / (self.gas_cost_per_object + self.target_init_coin_balance),
        );
        debug!(
            "Evenly splitting coin {:?} into {} coins",
            coin, split_count
        );
        let budget = self.gas_cost_per_object * split_count;
        let effects = loop {
            let mut pt_builder = ProgrammableTransactionBuilder::new();
            let pure_arg = pt_builder.pure(split_count).unwrap();
            pt_builder.programmable_move_call(
                IOTA_FRAMEWORK_PACKAGE_ID,
                PAY_MODULE_NAME.into(),
                PAY_SPLIT_N_FUNC_NAME.into(),
                vec![GAS::type_tag()],
                vec![Argument::GasCoin, pure_arg],
            );
            let pt = pt_builder.finish();
            let tx_data = TransactionData::new_programmable(
                self.sponsor_address,
                vec![coin.object_ref],
                pt,
                budget,
                rgp,
            );
            let sig = retry_forever!(async {
                self.signer
                    .sign_transaction(&tx_data)
                    .await
                    .tap_err(|err| error!("Failed to sign transaction: {:?}", err))
            })
            .unwrap();
            let tx = Transaction::from_generic_sig_data(tx_data, vec![sig]);
            debug!(
                "Sending transaction for execution. Tx digest: {:?}",
                tx.digest()
            );
            let result = self
                .iota_client
                .execute_transaction(tx.clone(), 10, None)
                .await;
            match result {
                Ok(effects) => {
                    assert!(
                        effects.status().is_ok(),
                        "Transaction failed. This should never happen. Tx: {:?}, effects: {:?}",
                        tx,
                        effects
                    );
                    break effects;
                }
                Err(e) => {
                    error!("Failed to execute transaction: {:?}", e);
                    coin = self
                        .iota_client
                        .get_latest_gas_objects([coin.object_ref.0])
                        .await
                        .into_iter()
                        .next()
                        .unwrap()
                        .1
                        .unwrap();
                    continue;
                }
            }
        };
        let mut result = vec![];
        let new_coin_balance = (coin.balance - budget) / split_count;
        for created in effects.created() {
            result.extend(self.enqueue_task(GasCoin {
                object_ref: created.reference.to_object_ref(),
                balance: new_coin_balance,
            }));
        }
        let remaining_coin_balance = (coin.balance - new_coin_balance * (split_count - 1)) as i64
            - effects.gas_cost_summary().net_gas_usage();
        result.extend(self.enqueue_task(GasCoin {
            object_ref: effects.gas_object().reference.to_object_ref(),
            balance: remaining_coin_balance as u64,
        }));
        self.increment_total_coin_count_by(result.len() - 1);
        result
    }
}

enum RunMode {
    Init,
    Refresh,
}

enum WakeReason {
    ForcedTrigger,
    Scheduled,
    Cancel,
}

impl WakeReason {
    fn reason(&self) -> &str {
        match self {
            WakeReason::ForcedTrigger => "Forced trigger received. Rescanning for new coins",
            WakeReason::Scheduled => "Scheduled trigger received. Rescanning for new coins",
            WakeReason::Cancel => "Coin init task is cancelled",
        }
    }
}

pub struct GasStationInitializer {
    _task_handle: JoinHandle<()>,
    // This is always Some. It is None only after the drop method is called.
    cancel_sender: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for GasStationInitializer {
    fn drop(&mut self) {
        self.cancel_sender.take().unwrap().send(()).unwrap();
    }
}

impl GasStationInitializer {
    pub async fn start(
        iota_client: IotaClient,
        storage: Arc<dyn Storage>,
        coin_init_config: CoinInitConfig,
        signer: Arc<dyn TxSigner>,
        rescan_trigger_receiver: tokio::sync::mpsc::Receiver<()>,
        force_full_rescan: bool,
        ignore_locks: bool,
    ) -> Self {
        let sponsor_address = signer.get_address();
        Self::perform_consistency_check(&iota_client, &storage, sponsor_address).await;

        if force_full_rescan {
            if storage
                .acquire_maintenance_lock(MAX_MAINTENANCE_DURATION_SEC)
                .await
                .expect("Failed to acquire maintenance lock for full rescan")
            {
                info!("Acquired maintenance lock for full rescan");
            } else {
                if ignore_locks {
                    info!("Another instance is already performing maintenance. Ignoring the lock and continuing with full rescan.");
                } else {
                    panic!("Another instance is already performing maintenance. Please wait for it to complete or use the --ignore-locks flag to force a full rescan.");
                }
            }
            let result = Self::clean_and_rescan_registry(
                iota_client.clone(),
                &storage,
                coin_init_config.target_init_balance,
                &signer,
                ignore_locks,
            )
            .await;

            // always release maintenance lock, regardless of success or failure
            if let Err(e) = storage.release_maintenance_lock().await {
                error!("Failed to release maintenance lock: {:?}", e);
            } else {
                info!("Released maintenance lock after full rescan");
            }
            if let Err(e) = result {
                panic!("Full rescan failed: {:?}", e);
            }
        } else if !storage.is_initialized().await.unwrap() {
            // If the pool has never been initialized, always run once at the beginning to make sure we have enough coins.
            if let Err(e) = Self::run_once(
                iota_client.clone(),
                &storage,
                RunMode::Init,
                coin_init_config.target_init_balance,
                &signer,
                ignore_locks,
            )
            .await
            {
                panic!("Initial coin initialization failed: {:?}", e);
            }
        }

        let (cancel_sender, cancel_receiver) = tokio::sync::oneshot::channel();
        let _task_handle = tokio::spawn(Self::run(
            iota_client,
            storage,
            coin_init_config,
            signer,
            cancel_receiver,
            rescan_trigger_receiver,
        ));
        Self {
            _task_handle,
            cancel_sender: Some(cancel_sender),
        }
    }

    /// Performs full rescan operations while holding the maintenance lock.
    async fn clean_and_rescan_registry(
        iota_client: IotaClient,
        storage: &Arc<dyn Storage>,
        target_init_balance: u64,
        signer: &Arc<dyn TxSigner>,
        ignore_init_lock: bool,
    ) -> anyhow::Result<()> {
        storage.clean_up_coin_registry().await?;
        storage.init_coin_stats_at_startup().await?;
        Self::run_once(
            iota_client,
            storage,
            RunMode::Init,
            target_init_balance,
            signer,
            ignore_init_lock,
        )
        .await?;
        Ok(())
    }

    async fn run(
        iota_client: IotaClient,
        storage: Arc<dyn Storage>,
        coin_init_config: CoinInitConfig,
        signer: Arc<dyn TxSigner>,
        mut cancel_receiver: tokio::sync::oneshot::Receiver<()>,
        mut rescan_trigger_receiver: tokio::sync::mpsc::Receiver<()>,
    ) {
        let mut ticker =
            tokio::time::interval(Duration::from_secs(coin_init_config.refresh_interval_sec));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            let wake_reason = tokio::select! {
                Some(_) = rescan_trigger_receiver.recv() => {
                    WakeReason::ForcedTrigger
                }
                _ = ticker.tick() => {
                    WakeReason::Scheduled
                }
                _ = &mut cancel_receiver => {
                    WakeReason::Cancel
                }
            };

            match wake_reason {
                WakeReason::ForcedTrigger | WakeReason::Scheduled => {
                    info!("{}", wake_reason.reason());
                    if let Err(e) = Self::run_once(
                        iota_client.clone(),
                        &storage,
                        RunMode::Refresh,
                        coin_init_config.target_init_balance,
                        &signer,
                        false,
                    )
                    .await
                    {
                        error!("Coin refresh failed: {:?}", e);
                    }
                }
                WakeReason::Cancel => {
                    info!("{}", wake_reason.reason());
                    break;
                }
            }
        }
    }

    async fn run_once(
        iota_client: IotaClient,
        storage: &Arc<dyn Storage>,
        mode: RunMode,
        target_init_coin_balance: u64,
        signer: &Arc<dyn TxSigner>,
        ignore_init_lock: bool,
    ) -> anyhow::Result<()> {
        let sponsor_address = signer.get_address();
        let lock_status = storage.acquire_init_lock(MAX_INIT_DURATION_SEC).await?;
        match lock_status {
            true => {
                info!("Acquired init lock. Starting new coin initialization");
            }
            false => {
                if ignore_init_lock {
                    info!("Ignoring the init lock and starting a new initialization");
                } else {
                    bail!("Another task is already initializing the pool. Please wait for it to complete or use the --ignore-locks flag to force a new initialization.");
                }
            }
        }

        let start = Instant::now();
        let balance_threshold = if matches!(mode, RunMode::Init) {
            info!("The pool has never been initialized. Initializing it for the first time");
            0
        } else {
            target_init_coin_balance * NEW_COIN_BALANCE_FACTOR_THRESHOLD
        };
        let coins = iota_client
            .get_all_owned_iota_coins_above_balance_threshold(sponsor_address, balance_threshold)
            .await;
        if coins.is_empty() {
            info!(
                "No coins with balance above {} found. Skipping new coin initialization",
                balance_threshold
            );
            storage.release_init_lock().await.unwrap();
            return Ok(());
        }
        let total_coin_count = Arc::new(AtomicUsize::new(coins.len()));
        let rgp = iota_client.get_reference_gas_price().await;
        let gas_cost_per_object = iota_client
            .calibrate_gas_cost_per_object(sponsor_address, &coins[0])
            .await;
        info!("Calibrated gas cost per object: {:?}", gas_cost_per_object);

        // Safety check: Validate minimum coin size prevent registry inconsistency
        if target_init_coin_balance * 99 <= gas_cost_per_object {
            let err_msg = format!(
                "target_init_coin_balance ({}) is too small relative to gas_cost_per_object ({}). \
                 target_init_coin_balance must be greater than gas_cost_per_object / 99 \
                 to prevent registry inconsistencies. Please increase target_init_coin_balance  \
                 to at least {} for current network and coin setup.",
                target_init_coin_balance,
                gas_cost_per_object,
                gas_cost_per_object / 99 + 1
            );
            error!("{}", err_msg);
            storage.release_init_lock().await.unwrap();
            return Err(anyhow::anyhow!(err_msg));
        }

        let result = Self::split_gas_coins(
            coins,
            CoinSplitEnv {
                target_init_coin_balance,
                gas_cost_per_object,
                signer: signer.clone(),
                sponsor_address,
                iota_client,
                task_queue: Default::default(),
                total_coin_count,
                rgp,
            },
        )
        .await;
        for chunk in result.chunks(5000) {
            storage.add_new_coins(chunk.to_vec()).await.unwrap();
        }
        storage.release_init_lock().await.unwrap();
        info!(
            "New coin initialization took {:?}s",
            start.elapsed().as_secs()
        );
        Ok(())
    }

    async fn split_gas_coins(coins: Vec<GasCoin>, env: CoinSplitEnv) -> Vec<GasCoin> {
        let total_balance: u64 = coins.iter().map(|c| c.balance).sum();
        info!(
            "Splitting {} coins with total balance of {} into smaller coins with target balance of {}. This will result in close to {} coins",
            coins.len(),
            total_balance,
            env.target_init_coin_balance,
            total_balance / env.target_init_coin_balance,
        );
        let mut result = vec![];
        for coin in coins {
            result.extend(env.enqueue_task(coin));
        }
        loop {
            let Some(task) = env.task_queue.lock().pop_front() else {
                break;
            };
            result.extend(task.await.unwrap());
        }
        let new_total_balance: u64 = result.iter().map(|c| c.balance).sum();
        info!(
            "Splitting finished. Got {} coins. New total balance: {}. Spent {} gas in total",
            result.len(),
            new_total_balance,
            total_balance - new_total_balance
        );
        result
    }

    /// Performs a quick consistency check comparing registry state against network state.
    async fn perform_consistency_check(
        iota_client: &IotaClient,
        storage: &Arc<dyn Storage>,
        sponsor_address: IotaAddress,
    ) {
        info!("Performing quick consistency check");
        let registry_coin_count = match storage.get_available_coin_count().await {
            Ok(count) => count as u64,
            Err(e) => {
                error!(
                    "Failed to get registry coin count for consistency check: {:?}",
                    e
                );
                return;
            }
        };
        let registry_balance = storage.get_available_coin_total_balance().await;
        let (network_coin_count, network_balance) =
            match iota_client.get_aggregate_coin_stats(sponsor_address).await {
                Ok(stats) => stats,
                Err(e) => {
                    error!(
                        "Failed to get network coin stats for consistency check: {:?}",
                        e
                    );
                    return;
                }
            };
        debug!(
            registry_balance = %registry_balance,
            network_balance = %network_balance,
            registry_coin_count = %registry_coin_count,
            network_coin_count = %network_coin_count,
            "Consistency check values"
        );

        let result = validate_consistency(
            registry_balance,
            registry_coin_count,
            network_balance,
            network_coin_count,
            None,
            None,
        );
        if result.has_divergence() {
            log_consistency_warnings(&result);
        } else {
            info!(
                "Consistency check passed: registry and network are within acceptable thresholds \
                 (balance divergence: {:.2}%, coin count divergence: {:.2}%)",
                result.balance_divergence_percent, result.coin_count_divergence_percent
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::CoinInitConfig;
    use crate::gas_station_initializer::{
        GasStationInitializer, NEW_COIN_BALANCE_FACTOR_THRESHOLD,
    };
    use crate::iota_client::IotaClient;
    use crate::storage::connect_storage_for_testing;
    use crate::test_env::start_iota_cluster;
    use iota_types::gas_coin::NANOS_PER_IOTA;
    use tokio::sync::mpsc::channel;

    // TODO: Add more accurate tests.

    #[tokio::test]
    async fn test_basic_init_flow() {
        telemetry_subscribers::init_for_testing();
        let (cluster, signer) = start_iota_cluster(vec![1000 * NANOS_PER_IOTA]).await;
        let fullnode_url = cluster.fullnode_handle.rpc_url;
        let storage = connect_storage_for_testing(signer.get_address()).await;
        let iota_client = IotaClient::new(&fullnode_url, None).await;
        let (_rescan_trigger_sender, rescan_trigger_receiver) = channel::<()>(1024);
        let _init_task = GasStationInitializer::start(
            iota_client,
            storage.clone(),
            CoinInitConfig {
                target_init_balance: NANOS_PER_IOTA,
                refresh_interval_sec: 200,
            },
            signer,
            rescan_trigger_receiver,
            false,
            false,
        )
        .await;
        assert!(storage.get_available_coin_count().await.unwrap() > 900);
    }

    #[tokio::test]
    async fn test_init_non_even_split() {
        telemetry_subscribers::init_for_testing();
        let (cluster, signer) = start_iota_cluster(vec![10000000 * NANOS_PER_IOTA]).await;
        let fullnode_url = cluster.fullnode_handle.rpc_url;
        let storage = connect_storage_for_testing(signer.get_address()).await;
        let target_init_balance = 12345 * NANOS_PER_IOTA;
        let iota_client = IotaClient::new(&fullnode_url, None).await;
        let (_rescan_trigger_sender, rescan_trigger_receiver) = channel::<()>(1024);
        let _init_task = GasStationInitializer::start(
            iota_client,
            storage.clone(),
            CoinInitConfig {
                target_init_balance,
                refresh_interval_sec: 200,
            },
            signer,
            rescan_trigger_receiver,
            false,
            false,
        )
        .await;
        assert!(storage.get_available_coin_count().await.unwrap() > 800);
    }

    #[tokio::test]
    async fn test_add_new_funds_to_pool() {
        telemetry_subscribers::init_for_testing();
        let (cluster, signer) = start_iota_cluster(vec![1000 * NANOS_PER_IOTA]).await;
        let sponsor = signer.get_address();
        let fullnode_url = cluster.fullnode_handle.rpc_url.clone();
        let storage = connect_storage_for_testing(signer.get_address()).await;
        let iota_client = IotaClient::new(&fullnode_url, None).await;
        let (_rescan_trigger_sender, rescan_trigger_receiver) = channel::<()>(1024);
        let _init_task = GasStationInitializer::start(
            iota_client,
            storage.clone(),
            CoinInitConfig {
                target_init_balance: NANOS_PER_IOTA,
                refresh_interval_sec: 1,
            },
            signer,
            rescan_trigger_receiver,
            false,
            false,
        )
        .await;
        assert!(storage.is_initialized().await.unwrap());
        let available_coin_count = storage.get_available_coin_count().await.unwrap();
        tracing::debug!("Available coin count: {}", available_coin_count);

        // Transfer some new IOTA into the sponsor account.
        let new_addr = *cluster
            .get_addresses()
            .iter()
            .find(|addr| **addr != sponsor)
            .unwrap();
        let tx_data = cluster
            .test_transaction_builder_with_sender(new_addr)
            .await
            .transfer_iota(
                Some(NEW_COIN_BALANCE_FACTOR_THRESHOLD * NANOS_PER_IOTA),
                sponsor,
            )
            .build();
        let response = cluster.sign_and_execute_transaction(&tx_data).await;
        tracing::debug!("New transfer effects: {:?}", response.effects.unwrap());

        // Give it some time for the task to pick up the new coin and split it.
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let new_available_coin_count = storage.get_available_coin_count().await.unwrap();
        assert!(
            // In an ideal world we should have NEW_COIN_BALANCE_FACTOR_THRESHOLD more coins
            // since we just send a new coin with balance NEW_COIN_BALANCE_FACTOR_THRESHOLD and split
            // into target balance of 1 IOTA each. However due to gas cost in splitting in practice
            // we are getting less, depending on gas cost which could change from time to time.
            // Subtract 5 which is an arbitrary small number just to be safe.
            new_available_coin_count
                > available_coin_count + NEW_COIN_BALANCE_FACTOR_THRESHOLD as usize - 5,
            "new_available_coin_count: {}, available_coin_count: {}",
            new_available_coin_count,
            available_coin_count
        );
    }

    #[tokio::test]
    async fn test_get_aggregate_coin_stats() {
        telemetry_subscribers::init_for_testing();
        let initial_balance = 1000 * NANOS_PER_IOTA;
        let (cluster, signer) = start_iota_cluster(vec![initial_balance]).await;
        let sponsor = signer.get_address();
        let fullnode_url = cluster.fullnode_handle.rpc_url;
        let iota_client = IotaClient::new(&fullnode_url, None).await;

        // Get aggregate stats from network
        let (coin_count, total_balance) = iota_client
            .get_aggregate_coin_stats(sponsor)
            .await
            .expect("get_aggregate_coin_stats should succeed");

        // Should have at least 1 coin with the initial balance
        assert!(
            coin_count >= 1,
            "Expected at least 1 coin, got {}",
            coin_count
        );
        assert!(
            total_balance >= initial_balance,
            "Expected at least {} balance, got {}",
            initial_balance,
            total_balance
        );

        tracing::debug!(
            "Aggregate stats: coin_count={}, total_balance={}",
            coin_count,
            total_balance
        );
    }

    #[tokio::test]
    async fn test_ignore_locks_bypasses_maintenance_lock() {
        telemetry_subscribers::init_for_testing();
        let (cluster, signer) = start_iota_cluster(vec![1000 * NANOS_PER_IOTA]).await;
        let fullnode_url = cluster.fullnode_handle.rpc_url;
        let storage = connect_storage_for_testing(signer.get_address()).await;
        let iota_client = IotaClient::new(&fullnode_url, None).await;

        let lock_acquired = storage.acquire_maintenance_lock(300).await.unwrap();
        assert!(lock_acquired, "Should be able to acquire maintenance lock");

        // double check if the lock is held
        let lock_acquired_again = storage.acquire_maintenance_lock(300).await.unwrap();
        assert!(
            !lock_acquired_again,
            "Should not be able to acquire maintenance lock again"
        );

        let (_rescan_trigger_sender, rescan_trigger_receiver) = channel::<()>(1024);
        let _init_task = GasStationInitializer::start(
            iota_client,
            storage.clone(),
            CoinInitConfig {
                target_init_balance: NANOS_PER_IOTA,
                refresh_interval_sec: 200,
            },
            signer,
            rescan_trigger_receiver,
            true,
            true, // ignore_locks=true should bypass the maintenance lock
        )
        .await;

        // Verify initialization completed successfully
        assert!(storage.is_initialized().await.unwrap());
        assert!(storage.get_available_coin_count().await.unwrap() > 0);
    }

    #[tokio::test]
    #[should_panic(
        expected = "Another instance is already performing maintenance. Please wait for it to complete or use the --ignore-locks flag to force a full rescan."
    )]
    async fn test_maintenance_lock_blocks_without_ignore_locks() {
        telemetry_subscribers::init_for_testing();
        let (cluster, signer) = start_iota_cluster(vec![1000 * NANOS_PER_IOTA]).await;
        let fullnode_url = cluster.fullnode_handle.rpc_url;
        let storage = connect_storage_for_testing(signer.get_address()).await;
        let iota_client = IotaClient::new(&fullnode_url, None).await;

        let lock_acquired = storage.acquire_maintenance_lock(300).await.unwrap();
        assert!(lock_acquired, "Should be able to acquire maintenance lock");

        let (_rescan_trigger_sender, rescan_trigger_receiver) = channel::<()>(1024);
        let _init_task = GasStationInitializer::start(
            iota_client,
            storage.clone(),
            CoinInitConfig {
                target_init_balance: NANOS_PER_IOTA,
                refresh_interval_sec: 200,
            },
            signer,
            rescan_trigger_receiver,
            true,
            false, // ignore_locks=false - should panic
        )
        .await;
    }

    #[tokio::test]
    async fn test_ignore_locks_bypasses_init_lock() {
        telemetry_subscribers::init_for_testing();
        let (cluster, signer) = start_iota_cluster(vec![1000 * NANOS_PER_IOTA]).await;
        let fullnode_url = cluster.fullnode_handle.rpc_url;
        let storage = connect_storage_for_testing(signer.get_address()).await;
        let iota_client = IotaClient::new(&fullnode_url, None).await;

        let lock_acquired = storage.acquire_init_lock(300).await.unwrap();
        assert!(lock_acquired, "Should be able to acquire init lock");

        // double check if the lock is held
        let lock_acquired_again = storage.acquire_init_lock(300).await.unwrap();
        assert!(
            !lock_acquired_again,
            "Should not be able to acquire init lock again"
        );

        let (_rescan_trigger_sender, rescan_trigger_receiver) = channel::<()>(1024);
        let _init_task = GasStationInitializer::start(
            iota_client,
            storage.clone(),
            CoinInitConfig {
                target_init_balance: NANOS_PER_IOTA,
                refresh_interval_sec: 200,
            },
            signer,
            rescan_trigger_receiver,
            false,
            true, // ignore_locks - should bypass the init lock
        )
        .await;

        assert!(storage.is_initialized().await.unwrap());
        assert!(storage.get_available_coin_count().await.unwrap() > 0);
    }

    #[tokio::test]
    #[should_panic(expected = "Initial coin initialization failed")]
    async fn test_init_lock_blocks_without_ignore_locks() {
        telemetry_subscribers::init_for_testing();
        let (cluster, signer) = start_iota_cluster(vec![1000 * NANOS_PER_IOTA]).await;
        let fullnode_url = cluster.fullnode_handle.rpc_url;
        let storage = connect_storage_for_testing(signer.get_address()).await;
        let iota_client = IotaClient::new(&fullnode_url, None).await;

        let lock_acquired = storage.acquire_init_lock(300).await.unwrap();
        assert!(lock_acquired, "Should be able to acquire init lock");

        let (_rescan_trigger_sender, rescan_trigger_receiver) = channel::<()>(1024);
        let _init_task = GasStationInitializer::start(
            iota_client,
            storage.clone(),
            CoinInitConfig {
                target_init_balance: NANOS_PER_IOTA,
                refresh_interval_sec: 200,
            },
            signer,
            rescan_trigger_receiver,
            false,
            false, // ignore_locks=false - should panic
        )
        .await;
    }
}
