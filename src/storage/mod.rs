// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::config::GasStationStorageConfig;
use crate::metrics::StorageMetrics;
use crate::storage::redis::RedisStorage;
use crate::types::{GasCoin, ReservationID};
use iota_sdk_types::{Address, ObjectId};
use std::sync::Arc;
use url::Url;

mod redis;

/// The protocol's `max_gas_payment_objects`, mirrored here.
///
/// **Exclusive bound** — the validators compare with a strict `<`, so the
/// largest gas payment they accept is 255. Compare with `<` everywhere.
pub const MAX_GAS_PER_QUERY: usize = 256;
pub const MAINTENANCE_MODE_ERROR_MESSAGE: &str =
    "Gas station is in maintenance mode. Please try again later.";

/// Defines the trait for a storage that manages gas coins.
/// It is expected to support concurrent access and manage atomicity internally.
/// It supports multiple addresses each with its own gas coin queue.
#[async_trait::async_trait]
pub trait Storage: SetGetStorage + Sync + Send {
    /// Reserve gas coins with total coin balance >= target_budget.
    /// If there is not enough balance, returns error.
    /// The implementation is required to guarantee that:
    /// 1. It never returns the same coin to multiple callers.
    /// 2. It keeps a record of the reserved coins with timestamp, so that in the case
    ///    when caller forgets to release them, some cleanup process can clean them up latter.
    /// 3. It must never return `MAX_GAS_PER_QUERY` coins or more -- that bound is
    ///    exclusive, so such a reservation could never be paid with.
    async fn reserve_gas_coins(
        &self,
        target_budget: u64,
        reserved_duration_ms: u64,
    ) -> anyhow::Result<(ReservationID, Vec<GasCoin>)>;

    /// Consume a reservation, but only if `payment` is exactly the set of gas
    /// coins it owns.
    ///
    /// The check is part of the contract because it must be atomic with the
    /// consumption. On mismatch, implementations must leave the reservation
    /// intact and must not disclose the reserved object ids in the error.
    async fn ready_for_execution(
        &self,
        reservation_id: ReservationID,
        payment: &[ObjectId],
    ) -> anyhow::Result<()>;

    async fn add_new_coins(&self, new_coins: Vec<GasCoin>) -> anyhow::Result<()>;

    async fn expire_coins(&self) -> anyhow::Result<Vec<ObjectId>>;

    /// Initialize some of the Gas Station statistics at the startup.
    /// Such as the total number of gas coins and the total balance.
    /// This is needed for several reasons:
    /// 1. To make sure that these fields are not empty when the first query comes in.
    ///    We only need this once ever though.
    /// 2. To make sure we start reporting the correct metrics from the beginning.
    /// Returns the total number of gas coins and the total balance.
    async fn init_coin_stats_at_startup(&self) -> anyhow::Result<(u64, u64)>;

    /// Whether the Gas Station for the given sponsor address is initialized.
    async fn is_initialized(&self) -> anyhow::Result<bool>;

    /// Acquire a lock to initialize the Gas Station for the given sponsor address for a certain duration.
    /// Returns true if the lock is acquired, false otherwise.
    /// Once the lock is acquired, until it expires, no other caller can acquire the lock.
    /// The reason we use a lock duration is such that in case the server crashed while holding the lock,
    /// the lock will be automatically considered as released after the lock duration.
    async fn acquire_init_lock(&self, lock_duration_sec: u64) -> anyhow::Result<bool>;

    async fn release_init_lock(&self) -> anyhow::Result<()>;

    /// Acquire a maintenance lock to prevent other instances from making changes to the coin registry.
    /// Unlike init_lock which prevents concurrent initialization, maintenance mode prevents:
    /// - Reserving gas coins
    /// - Adding new coins to the pool
    /// This should be used before clean_up_coin_registry() to ensure data consistency.
    /// Returns true if the lock is acquired, false otherwise.
    async fn acquire_maintenance_lock(&self, lock_duration_sec: u64) -> anyhow::Result<bool>;

    /// Release the maintenance lock, allowing normal operations to resume.
    async fn release_maintenance_lock(&self) -> anyhow::Result<()>;

    /// Check if the gas station is currently in maintenance mode.
    async fn is_maintenance_mode(&self) -> anyhow::Result<bool>;

    async fn check_health(&self) -> anyhow::Result<()>;

    #[cfg(test)]
    async fn flush_db(&self);

    async fn get_available_coin_count(&self) -> anyhow::Result<usize>;

    async fn get_available_coin_total_balance(&self) -> u64;

    #[cfg(test)]
    async fn get_reserved_coin_count(&self) -> usize;

    /// Clean up all the data from the coin registry namespace
    async fn clean_up_coin_registry(&self) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
pub trait SetGetStorage: Sync + Send {
    /// Set data in the storage.
    /// Key is expected to be absolute (already includes namespace if needed)
    async fn set_data(&self, key: &str, value: Vec<u8>) -> anyhow::Result<()>;

    /// Get data from the storage.
    /// Key is expected to be absolute (already includes namespace if needed)
    async fn get_data(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>>;
}

pub async fn connect_storage(
    config: &GasStationStorageConfig,
    sponsor_address: Address,
    namespace_prefix: &str,
    metrics: Arc<StorageMetrics>,
) -> Arc<dyn Storage> {
    let storage: Arc<dyn Storage> = match config {
        GasStationStorageConfig::Redis { redis_url } => {
            Arc::new(RedisStorage::new(redis_url, sponsor_address, namespace_prefix, metrics).await)
        }
    };
    storage
        .check_health()
        .await
        .expect("Unable to connect to the storage layer");
    storage.init_coin_stats_at_startup().await.unwrap();
    storage
}

/// Generate the namespace for the storage based on the network URL and the sponsor address.
pub fn get_storage_namespace(network_url: &str, sponsor_address: &Address) -> String {
    let url = Url::parse(network_url).unwrap();
    let scheme = url.scheme();
    let host = url.host_str().unwrap();
    let port = if let Some(port) = url.port() {
        port
    } else {
        if scheme == "https" {
            443
        } else {
            80
        }
    };
    let host_port = format!("{}_{}", host, port);
    format!("{}:{}", host_port, sponsor_address.to_string())
}

#[cfg(test)]
pub async fn connect_storage_for_testing_with_config(
    config: &GasStationStorageConfig,
    network_url: &str,
    sponsor_address: Address,
) -> Arc<dyn Storage> {
    use std::sync::atomic::{AtomicBool, Ordering};

    static IS_FIRST_CALL: AtomicBool = AtomicBool::new(true);
    let is_first_call = IS_FIRST_CALL.fetch_and(false, Ordering::SeqCst);

    let storage = connect_storage(
        config,
        sponsor_address,
        &get_storage_namespace(network_url, &sponsor_address),
        StorageMetrics::new_for_testing(),
    )
    .await;
    if is_first_call {
        // Make sure that we only flush the DB once at the beginning of each test run.
        storage.flush_db().await;
        // Re-init coin stats again since we just flushed.
        storage.init_coin_stats_at_startup().await.unwrap();
    }
    storage
}

#[cfg(test)]
pub async fn connect_storage_for_testing(sponsor_address: Address) -> Arc<dyn Storage> {
    connect_storage_for_testing_with_config(
        &GasStationStorageConfig::default(),
        "http://localhost:9000",
        sponsor_address,
    )
    .await
}

#[cfg(test)]
mod tests {
    use crate::storage::{connect_storage_for_testing, Storage, MAX_GAS_PER_QUERY};
    use crate::types::{random_object_ref, GasCoin};
    use iota_sdk_types::{Address, ObjectDigest, ObjectId, ObjectReference, Version};
    use rand::random;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use std::time::Duration;

    async fn assert_coin_count(storage: &Arc<dyn Storage>, available: usize, reserved: usize) {
        assert_eq!(storage.get_available_coin_count().await.unwrap(), available);
        assert_eq!(storage.get_reserved_coin_count().await, reserved);
    }

    /// Every coin the reservation handed out, which is what a well-behaved
    /// caller pays with.
    fn payment_of(coins: &[GasCoin]) -> Vec<ObjectId> {
        coins.iter().map(|c| c.object_ref.object_id).collect()
    }

    async fn setup(sponsor: Address, init_balances: Vec<u64>) -> Arc<dyn Storage> {
        let storage = connect_storage_for_testing(sponsor).await;
        let gas_coins = init_balances
            .into_iter()
            .map(|balance| GasCoin {
                object_ref: ObjectReference::new(
                    ObjectId::random(),
                    Version::from_u64(random()),
                    ObjectDigest::random(),
                ),
                balance,
            })
            .collect::<Vec<_>>();
        for chunk in gas_coins.chunks(5000) {
            storage.add_new_coins(chunk.to_vec()).await.unwrap();
        }
        storage
    }

    #[tokio::test]
    async fn test_gas_station_init() {
        let sponsor = Address::random();
        let storage = connect_storage_for_testing(sponsor).await;
        assert!(!storage.is_initialized().await.unwrap());
        storage.add_new_coins(vec![]).await.unwrap();
        // Still not initialized because we are not adding any coins.
        assert!(!storage.is_initialized().await.unwrap());
        storage
            .add_new_coins(vec![GasCoin {
                object_ref: random_object_ref(),
                balance: 1,
            }])
            .await
            .unwrap();
        assert!(storage.is_initialized().await.unwrap());
    }

    #[tokio::test]
    async fn test_successful_reservation() {
        // Create a Gas Station of 100000 coins, each with balance of 1.
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100000]).await;
        assert_coin_count(&storage, 100000, 0).await;
        let mut cur_available = 100000;
        let mut expected_res_id = 1;
        // Up to MAX_GAS_PER_QUERY - 1: the bound is exclusive, matching the
        // protocol's strict `<` on gas payment objects.
        for i in 1..MAX_GAS_PER_QUERY {
            let (res_id, reserved_gas_coins) =
                storage.reserve_gas_coins(i as u64, 1000).await.unwrap();
            assert_eq!(expected_res_id, res_id);
            assert_eq!(reserved_gas_coins.len(), i);
            expected_res_id += 1;
            cur_available -= i;
        }
        assert_coin_count(&storage, cur_available, 100000 - cur_available).await;
    }

    /// A reservation stops one coin short of `MAX_GAS_PER_QUERY`, because the
    /// protocol's check is `objects.len() < max_gas_payment_objects`. Handing
    /// out exactly `MAX_GAS_PER_QUERY` coins would create a reservation that no
    /// validator can execute.
    #[tokio::test]
    async fn test_max_gas_coin_per_query() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; MAX_GAS_PER_QUERY + 1]).await;

        // The largest reservation the protocol can actually pay with.
        let (_, coins) = storage
            .reserve_gas_coins((MAX_GAS_PER_QUERY - 1) as u64, 1000)
            .await
            .unwrap();
        assert_eq!(coins.len(), MAX_GAS_PER_QUERY - 1);

        // One more coin than that cannot be satisfied, even though the pool
        // holds enough balance for it.
        let storage = setup(Address::random(), vec![1; MAX_GAS_PER_QUERY + 1]).await;
        assert!(storage
            .reserve_gas_coins(MAX_GAS_PER_QUERY as u64, 1000)
            .await
            .is_err());
        assert_coin_count(&storage, MAX_GAS_PER_QUERY + 1, 0).await;
    }

    #[tokio::test]
    async fn test_insufficient_pool_budget() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        assert!(storage.reserve_gas_coins(101, 1000).await.is_err());
        assert_coin_count(&storage, 100, 0).await;
    }

    #[tokio::test]
    async fn test_coin_release() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        for _ in 0..100 {
            // Keep reserving and putting them back.
            // Should be able to repeat this process indefinitely if balance are not changed.
            let (res_id, reserved_gas_coins) = storage.reserve_gas_coins(99, 1000).await.unwrap();
            assert_eq!(reserved_gas_coins.len(), 99);
            assert_coin_count(&storage, 1, 99).await;
            storage
                .ready_for_execution(res_id, &payment_of(&reserved_gas_coins))
                .await
                .unwrap();
            storage.add_new_coins(reserved_gas_coins).await.unwrap();
            assert_coin_count(&storage, 100, 0).await;
        }
    }

    #[tokio::test]
    async fn test_coin_release_with_updated_balance() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        for _ in 0..10 {
            let (res_id, mut reserved_gas_coins) =
                storage.reserve_gas_coins(10, 1000).await.unwrap();
            assert_eq!(
                reserved_gas_coins.iter().map(|c| c.balance).sum::<u64>(),
                10
            );
            for reserved_gas_coin in reserved_gas_coins.iter_mut() {
                if reserved_gas_coin.balance > 0 {
                    reserved_gas_coin.balance -= 1;
                }
            }
            storage
                .ready_for_execution(res_id, &payment_of(&reserved_gas_coins))
                .await
                .unwrap();
            storage.add_new_coins(reserved_gas_coins).await.unwrap();
        }
        assert_coin_count(&storage, 100, 0).await;
        assert_eq!(storage.get_available_coin_total_balance().await, 0);
        assert!(storage.reserve_gas_coins(1, 1000).await.is_err());
    }

    #[tokio::test]
    async fn test_deleted_objects() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        let (res_id, mut reserved_gas_coins) = storage.reserve_gas_coins(100, 1000).await.unwrap();
        assert_eq!(reserved_gas_coins.len(), 100);

        storage
            .ready_for_execution(res_id, &payment_of(&reserved_gas_coins))
            .await
            .unwrap();

        reserved_gas_coins.drain(0..50);
        storage.add_new_coins(reserved_gas_coins).await.unwrap();
        assert_coin_count(&storage, 50, 0).await;
    }

    /// The reservation-hijack case: naming a reservation id must not be enough
    /// to consume it. Before the binding this destroyed the victim's reservation
    /// and permanently leaked its coins.
    #[tokio::test]
    async fn test_ready_for_execution_rejects_another_reservations_coins() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        let (victim_id, victim_coins) = storage.reserve_gas_coins(10, 10000).await.unwrap();
        let (attacker_id, attacker_coins) = storage.reserve_gas_coins(10, 10000).await.unwrap();

        let err = storage
            .ready_for_execution(victim_id, &payment_of(&attacker_coins))
            .await
            .unwrap_err()
            .to_string();
        // The reserved ids must not be disclosed to a caller who guessed an id.
        for coin in &victim_coins {
            assert!(
                !err.contains(&coin.object_ref.object_id.to_string()),
                "error leaked a reserved object id: {err}"
            );
        }

        // Both reservations survive the rejection intact, and each still works
        // with its own coins.
        assert_coin_count(&storage, 80, 20).await;
        storage
            .ready_for_execution(victim_id, &payment_of(&victim_coins))
            .await
            .unwrap();
        storage
            .ready_for_execution(attacker_id, &payment_of(&attacker_coins))
            .await
            .unwrap();
    }

    /// A rejected attempt must leave the reservation on the expiration queue,
    /// or its coins are stranded just as the hijack stranded them.
    #[tokio::test]
    async fn test_rejected_payment_leaves_the_reservation_expirable() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        let (res_id, coins) = storage.reserve_gas_coins(10, 900).await.unwrap();
        let foreign = vec![ObjectId::random()];

        assert!(storage
            .ready_for_execution(res_id, &foreign)
            .await
            .is_err());
        assert_coin_count(&storage, 90, 10).await;

        tokio::time::sleep(Duration::from_secs(1)).await;
        // `expire_coins` hands the ids back rather than re-adding them, so this
        // proves the reservation was still on the queue to be found.
        let expired = storage.expire_coins().await.unwrap();
        assert_eq!(
            expired.into_iter().collect::<BTreeSet<_>>(),
            payment_of(&coins).into_iter().collect::<BTreeSet<_>>()
        );
        assert_coin_count(&storage, 90, 0).await;
    }

    /// A subset is rejected: the `DEL` is wholesale, so omitted coins would be
    /// dropped from the registry without ever being released.
    #[tokio::test]
    async fn test_ready_for_execution_rejects_partial_payment() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        let (res_id, coins) = storage.reserve_gas_coins(10, 10000).await.unwrap();

        let mut short = payment_of(&coins);
        short.pop();
        assert!(storage.ready_for_execution(res_id, &short).await.is_err());
        assert!(storage.ready_for_execution(res_id, &[]).await.is_err());

        // Untouched, so the honest payment still works.
        storage
            .ready_for_execution(res_id, &payment_of(&coins))
            .await
            .unwrap();
    }

    /// A repeated coin must not stand in for the one it replaces, which would
    /// satisfy a naive length check.
    #[tokio::test]
    async fn test_ready_for_execution_rejects_repeated_and_extra_coins() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        let (res_id, coins) = storage.reserve_gas_coins(10, 10000).await.unwrap();

        let first = coins[0].object_ref.object_id;
        let repeated = vec![first; coins.len()];
        assert!(storage.ready_for_execution(res_id, &repeated).await.is_err());

        let mut padded = payment_of(&coins);
        padded.push(ObjectId::random());
        assert!(storage.ready_for_execution(res_id, &padded).await.is_err());

        storage
            .ready_for_execution(res_id, &payment_of(&coins))
            .await
            .unwrap();
    }

    /// Set comparison, not sequence: a client orders its payment as it likes.
    #[tokio::test]
    async fn test_ready_for_execution_ignores_payment_order() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        let (res_id, coins) = storage.reserve_gas_coins(10, 10000).await.unwrap();

        let mut shuffled = payment_of(&coins);
        shuffled.reverse();
        storage
            .ready_for_execution(res_id, &shuffled)
            .await
            .unwrap();
        assert_coin_count(&storage, 90, 0).await;
    }

    /// An unknown id must still report the original message, which callers match on.
    #[tokio::test]
    async fn test_ready_for_execution_unknown_reservation() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        let err = storage
            .ready_for_execution(999_999, &[ObjectId::random()])
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Reservation no longer exist: 999999"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_coin_expiration() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        let (_res_id1, reserved_gas_coins1) = storage.reserve_gas_coins(10, 900).await.unwrap();
        assert_eq!(reserved_gas_coins1.len(), 10);
        let (_res_id2, reserved_gas_coins2) = storage.reserve_gas_coins(30, 1900).await.unwrap();
        assert_eq!(reserved_gas_coins2.len(), 30);
        // Just to make sure these two reservations will have a different expiration timestamp.
        tokio::time::sleep(Duration::from_millis(1)).await;
        let (_res_id3, reserved_gas_coins3) = storage.reserve_gas_coins(50, 1900).await.unwrap();
        assert_eq!(reserved_gas_coins3.len(), 50);
        assert_coin_count(&storage, 10, 90).await;

        assert!(storage.expire_coins().await.unwrap().is_empty());
        assert_coin_count(&storage, 10, 90).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        let expired1 = storage.expire_coins().await.unwrap();
        assert_eq!(expired1.len(), 10);
        assert_eq!(
            expired1.iter().cloned().collect::<BTreeSet<_>>(),
            reserved_gas_coins1
                .iter()
                .map(|coin| coin.object_ref.object_id)
                .collect::<BTreeSet<_>>()
        );
        assert_coin_count(&storage, 10, 80).await;

        assert!(storage.expire_coins().await.unwrap().is_empty());
        assert_coin_count(&storage, 10, 80).await;
        tokio::time::sleep(Duration::from_secs(1)).await;

        let expired2 = storage.expire_coins().await.unwrap();
        assert_eq!(expired2.len(), 80);
        assert_eq!(
            expired2.iter().cloned().collect::<BTreeSet<_>>(),
            reserved_gas_coins2
                .iter()
                .chain(&reserved_gas_coins3)
                .map(|coin| coin.object_ref.object_id)
                .collect::<BTreeSet<_>>()
        );
        assert_coin_count(&storage, 10, 0).await;
    }

    #[tokio::test]
    async fn test_multiple_sponsors() {
        let sponsors = (0..10).map(|_| Address::random()).collect::<Vec<_>>();
        let mut storages = vec![];
        for sponsor in sponsors {
            storages.push(setup(sponsor, vec![1; 100]).await);
        }
        for storage in storages {
            let (_, gas_coins) = storage.reserve_gas_coins(50, 1000).await.unwrap();
            assert_eq!(gas_coins.len(), 50);
            assert_coin_count(&storage, 50, 50).await;
        }
    }

    #[tokio::test]
    async fn test_concurrent_reservation() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100000]).await;
        let mut handles = vec![];
        for _ in 0..10 {
            let storage = storage.clone();
            handles.push(tokio::spawn(async move {
                let mut reserved_gas_coins = vec![];
                for _ in 0..100 {
                    let (_, newly_reserved) = storage.reserve_gas_coins(3, 1000).await.unwrap();
                    reserved_gas_coins.extend(newly_reserved);
                }
                reserved_gas_coins
            }));
        }
        let mut reserved_gas_coins = vec![];
        for handle in handles {
            reserved_gas_coins.extend(handle.await.unwrap());
        }
        let count = reserved_gas_coins.len();
        // Check that all object IDs are unique in all reservations.
        reserved_gas_coins.sort_by_key(|c| c.object_ref.object_id);
        reserved_gas_coins.dedup_by_key(|c| c.object_ref.object_id);
        assert_eq!(reserved_gas_coins.len(), count);
        assert_coin_count(&storage, 100000 - count, count).await;
    }

    #[tokio::test]
    async fn test_acquire_init_lock() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        assert!(storage.acquire_init_lock(5).await.unwrap());
        assert!(!storage.acquire_init_lock(1).await.unwrap());
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert!(storage.acquire_init_lock(5).await.unwrap());
    }

    #[tokio::test]
    async fn test_init_coin_stats_idempotent() {
        let sponsor = Address::random();
        let storage = setup(sponsor, vec![1; 100]).await;
        // init_coin_stats_at_startup has already been called in setup.
        // Calling it again should not change anything.
        let (coin_count, total_balance) = storage.init_coin_stats_at_startup().await.unwrap();
        assert_eq!(coin_count, 100);
        assert_eq!(total_balance, 100);
    }
}
