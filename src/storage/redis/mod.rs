// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub mod migration;
mod script_manager;

use crate::metrics::StorageMetrics;
use crate::storage::redis::script_manager::ScriptManager;
use crate::storage::{SetGetStorage, Storage, MAINTENANCE_MODE_ERROR_MESSAGE};
use crate::types::{GasCoin, ReservationID};
use anyhow::bail;
use chrono::Utc;
use iota_types::base_types::{IotaAddress, ObjectDigest, ObjectID, SequenceNumber};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use std::ops::Add;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

pub struct RedisStorage {
    conn_manager: ConnectionManager,
    sponsor_str: String,
    namespace: String,
    metrics: Arc<StorageMetrics>,
}

impl RedisStorage {
    pub async fn new(
        redis_url: &str,
        sponsor_address: IotaAddress,
        namespace_prefix: &str,
        metrics: Arc<StorageMetrics>,
    ) -> Self {
        let namespace = generate_namespace(namespace_prefix);
        let client = redis::Client::open(redis_url).unwrap();
        let mut conn_manager = ConnectionManager::new(client).await.unwrap();

        // Check and perform migration from old namespace if needed
        let sponsor_str = sponsor_address.to_string();
        if let Err(e) = Self::maybe_migrate(&mut conn_manager, &sponsor_str, &namespace).await {
            tracing::warn!(
                "Migration check failed: {:?}. Continuing without migration.",
                e
            );
        }

        Self {
            conn_manager,
            sponsor_str,
            namespace,
            metrics,
        }
    }

    /// Check for and perform migration from old namespace format if needed.
    ///
    /// Old format: `{wallet_address}:{key_name}`
    /// New format: `{namespace_prefix}:registry:{key_name}`
    async fn maybe_migrate(
        conn: &mut ConnectionManager,
        sponsor_address: &str,
        new_namespace: &str,
    ) -> anyhow::Result<()> {
        if let Some(result) = migration::maybe_migrate(conn, sponsor_address, new_namespace).await?
        {
            if result.has_migrations() {
                info!(
                    "Successfully migrated {} Redis keys from old namespace format",
                    result.migrated_count
                );
            }
        }
        Ok(())
    }
}

fn generate_namespace(namespace_prefix: &str) -> String {
    format!("{}:registry", namespace_prefix)
}

#[async_trait::async_trait]
impl SetGetStorage for RedisStorage {
    async fn set_data(&self, key: &str, value: Vec<u8>) -> anyhow::Result<()> {
        let mut conn = self.conn_manager.clone();
        conn.set::<_, _, ()>(key, value).await?;
        Ok(())
    }
    async fn get_data(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let mut conn = self.conn_manager.clone();
        let value: Option<Vec<u8>> = conn.get(key).await?;
        Ok(value)
    }
}

#[async_trait::async_trait]
impl Storage for RedisStorage {
    async fn reserve_gas_coins(
        &self,
        target_budget: u64,
        reserved_duration_ms: u64,
    ) -> anyhow::Result<(ReservationID, Vec<GasCoin>)> {
        self.metrics.num_reserve_gas_coins_requests.inc();

        let current_time = Utc::now().timestamp() as u64;
        let expiration_time = Utc::now()
            .add(Duration::from_millis(reserved_duration_ms))
            .timestamp_millis() as u64;
        let mut conn = self.conn_manager.clone();
        // We use i64 here because the script may return -1 for maintenance mode
        let (reservation_id_raw, coins, new_total_balance, new_coin_count): (
            i64,
            Vec<String>,
            i64,
            i64,
        ) = ScriptManager::reserve_gas_coins_script()
            .arg(self.namespace.clone())
            .arg(target_budget)
            .arg(expiration_time)
            .arg(current_time)
            .invoke_async(&mut conn)
            .await?;
        // The script returns (-1, []) if the gas station is in maintenance mode.
        if reservation_id_raw == -1 {
            bail!(MAINTENANCE_MODE_ERROR_MESSAGE);
        }
        let reservation_id = reservation_id_raw as ReservationID;
        // The script returns (0, []) if it is unable to find enough coins to reserve.
        // We choose to handle the error here instead of inside the script so that we could
        // provide a more readable error message.
        if coins.is_empty() {
            bail!("Unable to reserve gas coins for the given budget.");
        }
        let gas_coins: Vec<_> = coins
            .into_iter()
            .map(|s| {
                // Each coin is in the form of: balance,object_id,version,digest
                let mut splits = s.split(',');
                let balance = splits.next().unwrap().parse::<u64>().unwrap();
                let object_id = ObjectID::from_str(splits.next().unwrap()).unwrap();
                let version = SequenceNumber::from(splits.next().unwrap().parse::<u64>().unwrap());
                let digest = ObjectDigest::from_str(splits.next().unwrap()).unwrap();
                GasCoin {
                    balance,
                    object_ref: (object_id, version, digest),
                }
            })
            .collect();

        self.metrics
            .gas_station_available_gas_coin_count
            .with_label_values(&[&self.namespace])
            .set(new_coin_count);
        self.metrics
            .gas_station_available_gas_total_balance
            .with_label_values(&[&self.namespace])
            .set(new_total_balance);
        self.metrics.num_successful_reserve_gas_coins_requests.inc();
        Ok((reservation_id, gas_coins))
    }

    async fn ready_for_execution(&self, reservation_id: ReservationID) -> anyhow::Result<()> {
        self.metrics.num_ready_for_execution_requests.inc();

        let mut conn = self.conn_manager.clone();
        ScriptManager::ready_for_execution_script()
            .arg(self.namespace.clone())
            .arg(reservation_id)
            .invoke_async::<_, ()>(&mut conn)
            .await?;

        self.metrics
            .num_successful_ready_for_execution_requests
            .inc();
        Ok(())
    }

    async fn add_new_coins(&self, new_coins: Vec<GasCoin>) -> anyhow::Result<()> {
        self.metrics.num_add_new_coins_requests.inc();
        let formatted_coins = new_coins
            .iter()
            .map(|c| {
                // The format is: balance,object_id,version,digest
                // The way we turn them into strings must be consistent with the way we parse them in
                // reserve_gas_coins_script.
                format!(
                    "{},{},{},{}",
                    c.balance,
                    c.object_ref.0,
                    c.object_ref.1.value(),
                    c.object_ref.2
                )
            })
            .collect::<Vec<String>>();

        let mut conn = self.conn_manager.clone();
        let (new_total_balance, new_coin_count): (i64, i64) = ScriptManager::add_new_coins_script()
            .arg(self.namespace.clone())
            .arg(serde_json::to_string(&formatted_coins)?)
            .invoke_async(&mut conn)
            .await?;

        debug!(
            "After add_new_coins. New total balance: {}, new coin count: {}",
            new_total_balance, new_coin_count
        );
        self.metrics
            .gas_station_available_gas_coin_count
            .with_label_values(&[&self.sponsor_str])
            .set(new_coin_count);
        self.metrics
            .gas_station_available_gas_total_balance
            .with_label_values(&[&self.sponsor_str])
            .set(new_total_balance);
        self.metrics.num_successful_add_new_coins_requests.inc();
        Ok(())
    }

    async fn expire_coins(&self) -> anyhow::Result<Vec<ObjectID>> {
        self.metrics.num_expire_coins_requests.inc();

        let now = Utc::now().timestamp_millis() as u64;
        let mut conn = self.conn_manager.clone();
        let expired_coin_strings: Vec<String> = ScriptManager::expire_coins_script()
            .arg(self.namespace.clone())
            .arg(now)
            .invoke_async(&mut conn)
            .await?;
        // The script returns a list of comma separated coin ids.
        let expired_coin_ids = expired_coin_strings
            .iter()
            .flat_map(|s| s.split(',').map(|id| ObjectID::from_str(id).unwrap()))
            .collect();

        self.metrics.num_successful_expire_coins_requests.inc();
        Ok(expired_coin_ids)
    }

    async fn init_coin_stats_at_startup(&self) -> anyhow::Result<(u64, u64)> {
        let mut conn = self.conn_manager.clone();
        let (available_coin_count, available_coin_total_balance): (i64, i64) =
            ScriptManager::init_coin_stats_at_startup_script()
                .arg(self.namespace.clone())
                .invoke_async(&mut conn)
                .await?;
        info!(
            sponsor_address=?self.sponsor_str,
            "Number of available gas coins in the pool: {}, total balance: {}",
            available_coin_count,
            available_coin_total_balance
        );
        self.metrics
            .gas_station_available_gas_coin_count
            .with_label_values(&[&self.sponsor_str])
            .set(available_coin_count);
        self.metrics
            .gas_station_available_gas_total_balance
            .with_label_values(&[&self.sponsor_str])
            .set(available_coin_total_balance);
        Ok((
            available_coin_count as u64,
            available_coin_total_balance as u64,
        ))
    }

    async fn is_initialized(&self) -> anyhow::Result<bool> {
        let mut conn = self.conn_manager.clone();
        let result = ScriptManager::get_is_initialized_script()
            .arg(self.namespace.clone())
            .invoke_async::<_, bool>(&mut conn)
            .await?;
        Ok(result)
    }

    async fn acquire_init_lock(&self, lock_duration_sec: u64) -> anyhow::Result<bool> {
        let mut conn = self.conn_manager.clone();
        let cur_timestamp = Utc::now().timestamp() as u64;
        debug!(
            "Acquiring init lock at {} for {} seconds",
            cur_timestamp, lock_duration_sec
        );
        let result = ScriptManager::acquire_init_lock_script()
            .arg(self.namespace.clone())
            .arg(cur_timestamp)
            .arg(lock_duration_sec)
            .invoke_async::<_, bool>(&mut conn)
            .await?;
        Ok(result)
    }

    async fn release_init_lock(&self) -> anyhow::Result<()> {
        debug!("Releasing the init lock.");
        let mut conn = self.conn_manager.clone();
        ScriptManager::release_init_lock_script()
            .arg(self.namespace.clone())
            .invoke_async::<_, ()>(&mut conn)
            .await?;
        Ok(())
    }

    async fn acquire_maintenance_lock(&self, lock_duration_sec: u64) -> anyhow::Result<bool> {
        let mut conn = self.conn_manager.clone();
        let cur_timestamp = Utc::now().timestamp() as u64;
        debug!(
            "Acquiring maintenance lock at {} for {} seconds",
            cur_timestamp, lock_duration_sec
        );
        let result = ScriptManager::acquire_maintenance_lock_script()
            .arg(self.namespace.clone())
            .arg(cur_timestamp)
            .arg(lock_duration_sec)
            .invoke_async::<_, bool>(&mut conn)
            .await?;
        Ok(result)
    }

    async fn release_maintenance_lock(&self) -> anyhow::Result<()> {
        debug!("Releasing the maintenance lock.");
        let mut conn = self.conn_manager.clone();
        ScriptManager::release_maintenance_lock_script()
            .arg(self.namespace.clone())
            .invoke_async::<_, ()>(&mut conn)
            .await?;
        Ok(())
    }

    async fn is_maintenance_mode(&self) -> anyhow::Result<bool> {
        let mut conn = self.conn_manager.clone();
        let cur_timestamp = Utc::now().timestamp() as u64;
        let result = ScriptManager::is_maintenance_mode_script()
            .arg(self.namespace.clone())
            .arg(cur_timestamp)
            .invoke_async::<_, bool>(&mut conn)
            .await?;
        Ok(result)
    }

    async fn check_health(&self) -> anyhow::Result<()> {
        let mut conn = self.conn_manager.clone();
        redis::cmd("PING")
            .query_async::<_, String>(&mut conn)
            .await?;
        Ok(())
    }

    #[cfg(test)]
    async fn flush_db(&self) {
        let mut conn = self.conn_manager.clone();
        redis::cmd("FLUSHDB")
            .query_async::<_, String>(&mut conn)
            .await
            .unwrap();
    }

    async fn get_available_coin_count(&self) -> anyhow::Result<usize> {
        let mut conn = self.conn_manager.clone();
        let count = ScriptManager::get_available_coin_count_script()
            .arg(self.namespace.clone())
            .invoke_async::<_, usize>(&mut conn)
            .await?;
        Ok(count)
    }

    async fn get_available_coin_total_balance(&self) -> u64 {
        let mut conn = self.conn_manager.clone();
        ScriptManager::get_available_coin_total_balance_script()
            .arg(self.namespace.clone())
            .invoke_async::<_, u64>(&mut conn)
            .await
            .unwrap()
    }

    #[cfg(test)]
    async fn get_reserved_coin_count(&self) -> usize {
        let mut conn = self.conn_manager.clone();
        ScriptManager::get_reserved_coin_count_script()
            .arg(self.namespace.clone())
            .invoke_async::<_, usize>(&mut conn)
            .await
            .unwrap()
    }

    async fn clean_up_coin_registry(&self) -> anyhow::Result<()> {
        let mut conn = self.conn_manager.clone();
        let deleted_count: i64 = ScriptManager::clean_up_coin_registry_script()
            .arg(self.namespace.clone())
            .invoke_async(&mut conn)
            .await?;
        debug!(
            namespace = %self.namespace,
            deleted_count,
            "Cleaned up coin registry with {deleted_count} keys"
        );
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use iota_types::base_types::{random_object_ref, IotaAddress};
    use serde::{Deserialize, Serialize};

    use crate::{
        metrics::StorageMetrics,
        storage::{redis::RedisStorage, SetGetStorage, Storage},
        types::GasCoin,
    };

    #[tokio::test]
    async fn test_set_and_get_data() {
        let storage = setup_storage().await;
        #[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
        struct TestStruct {
            pub value: String,
        }
        let test_struct = TestStruct {
            value: "test_value".to_string(),
        };
        let key = format!("{}:test_key", storage.namespace);
        storage
            .set_data(&key, serde_json::to_vec(&test_struct).unwrap())
            .await
            .unwrap();
        let value: Option<Vec<u8>> = storage.get_data(&key).await.unwrap();
        let value: TestStruct = serde_json::from_slice(&value.unwrap()).unwrap();
        assert_eq!(value, test_struct);
    }

    #[tokio::test]
    async fn test_init_coin_stats_at_startup() {
        let storage = setup_storage().await;
        storage
            .add_new_coins(vec![
                GasCoin {
                    balance: 100,
                    object_ref: random_object_ref(),
                },
                GasCoin {
                    balance: 200,
                    object_ref: random_object_ref(),
                },
            ])
            .await
            .unwrap();
        let (coin_count, total_balance) = storage.init_coin_stats_at_startup().await.unwrap();
        assert_eq!(coin_count, 2);
        assert_eq!(total_balance, 300);
    }

    #[tokio::test]
    async fn test_add_new_coins() {
        let storage = setup_storage().await;
        storage
            .add_new_coins(vec![
                GasCoin {
                    balance: 100,
                    object_ref: random_object_ref(),
                },
                GasCoin {
                    balance: 200,
                    object_ref: random_object_ref(),
                },
            ])
            .await
            .unwrap();
        let coin_count = storage.get_available_coin_count().await.unwrap();
        assert_eq!(coin_count, 2);
        let total_balance = storage.get_available_coin_total_balance().await;
        assert_eq!(total_balance, 300);
        storage
            .add_new_coins(vec![
                GasCoin {
                    balance: 300,
                    object_ref: random_object_ref(),
                },
                GasCoin {
                    balance: 400,
                    object_ref: random_object_ref(),
                },
            ])
            .await
            .unwrap();
        let coin_count = storage.get_available_coin_count().await.unwrap();
        assert_eq!(coin_count, 4);
        let total_balance = storage.get_available_coin_total_balance().await;
        assert_eq!(total_balance, 1000);
    }

    #[tokio::test]
    async fn test_clean_up_coin_registry() {
        let storage = setup_storage().await;

        // Add some coins
        storage
            .add_new_coins(vec![
                GasCoin {
                    balance: 100,
                    object_ref: random_object_ref(),
                },
                GasCoin {
                    balance: 200,
                    object_ref: random_object_ref(),
                },
            ])
            .await
            .unwrap();

        // Verify coins are added
        let coin_count = storage.get_available_coin_count().await.unwrap();
        assert_eq!(coin_count, 2);
        let total_balance = storage.get_available_coin_total_balance().await;
        assert_eq!(total_balance, 300);

        // Set some data with absolute key
        let test_key = format!("{}:test_key", storage.namespace);
        storage
            .set_data(&test_key, b"test_value".to_vec())
            .await
            .unwrap();
        let value = storage.get_data(&test_key).await.unwrap();
        assert!(value.is_some());

        // Clean up the coin registry
        storage.clean_up_coin_registry().await.unwrap();

        // Verify all data is cleaned up - reinitialize stats to get fresh values
        let (coin_count, total_balance) = storage.init_coin_stats_at_startup().await.unwrap();
        assert_eq!(coin_count, 0);
        assert_eq!(total_balance, 0);

        // Verify the test_key is also cleaned up
        let value = storage.get_data(&test_key).await.unwrap();
        assert!(value.is_none());
    }

    #[tokio::test]
    async fn test_clean_up_preserves_locks() {
        let storage = setup_storage().await;

        // Add some coins and data
        storage
            .add_new_coins(vec![
                GasCoin {
                    balance: 100,
                    object_ref: random_object_ref(),
                },
                GasCoin {
                    balance: 200,
                    object_ref: random_object_ref(),
                },
            ])
            .await
            .unwrap();
        let test_key = format!("{}:test_key", storage.namespace);
        storage
            .set_data(&test_key, b"test_value".to_vec())
            .await
            .unwrap();

        // Acquire maintenance lock BEFORE cleanup
        let lock_acquired = storage
            .acquire_maintenance_lock(300) // 300 seconds
            .await
            .unwrap();
        assert!(lock_acquired, "Should acquire maintenance lock");

        // Verify we're in maintenance mode
        assert!(
            storage.is_maintenance_mode().await.unwrap(),
            "Should be in maintenance mode"
        );

        // Clean up the coin registry (this should NOT delete the maintenance lock)
        storage.clean_up_coin_registry().await.unwrap();

        // Verify maintenance lock is STILL held after cleanup
        assert!(
            storage.is_maintenance_mode().await.unwrap(),
            "Maintenance lock should be preserved after cleanup"
        );

        // Verify we cannot acquire the lock again (it's still held)
        let lock_acquired_again = storage.acquire_maintenance_lock(300).await.unwrap();
        assert!(
            !lock_acquired_again,
            "Should not be able to acquire lock again while it's held"
        );

        // Verify other data was cleaned up
        let (coin_count, total_balance) = storage.init_coin_stats_at_startup().await.unwrap();
        assert_eq!(coin_count, 0, "Coins should be cleaned up");
        assert_eq!(total_balance, 0, "Balance should be zero");

        let value = storage.get_data(&test_key).await.unwrap();
        assert!(value.is_none(), "Test data should be cleaned up");

        // Release the lock
        storage.release_maintenance_lock().await.unwrap();
        assert!(
            !storage.is_maintenance_mode().await.unwrap(),
            "Should not be in maintenance mode after release"
        );

        // Now we should be able to acquire it again
        let lock_acquired_final = storage.acquire_maintenance_lock(300).await.unwrap();
        assert!(
            lock_acquired_final,
            "Should be able to acquire lock after release"
        );
    }

    async fn setup_storage() -> RedisStorage {
        let sponsor = IotaAddress::ZERO;
        let namespace_prefix = "test";
        let redis_url = "redis://127.0.0.1:6379";
        let storage = RedisStorage::new(
            redis_url,
            sponsor,
            namespace_prefix,
            StorageMetrics::new_for_testing(),
        )
        .await;
        storage.flush_db().await;
        let (coin_count, total_balance) = storage.init_coin_stats_at_startup().await.unwrap();
        assert_eq!(coin_count, 0);
        assert_eq!(total_balance, 0);
        storage
    }

    #[tokio::test]
    async fn test_maintenance_mode() {
        let storage = setup_storage().await;

        // Add some coins
        storage
            .add_new_coins(vec![
                GasCoin {
                    balance: 100,
                    object_ref: random_object_ref(),
                },
                GasCoin {
                    balance: 200,
                    object_ref: random_object_ref(),
                },
            ])
            .await
            .unwrap();

        // Initially not in maintenance mode
        assert!(!storage.is_maintenance_mode().await.unwrap());

        // Acquire maintenance lock
        assert!(storage.acquire_maintenance_lock(60).await.unwrap());
        assert!(storage.is_maintenance_mode().await.unwrap());

        // Trying to acquire again should fail
        assert!(!storage.acquire_maintenance_lock(60).await.unwrap());

        // Reserving coins should fail during maintenance
        let result = storage.reserve_gas_coins(50, 1000).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("maintenance mode"));

        // Release maintenance lock
        storage.release_maintenance_lock().await.unwrap();
        assert!(!storage.is_maintenance_mode().await.unwrap());

        // Now reserving should work
        let (reservation_id, coins) = storage.reserve_gas_coins(50, 1000).await.unwrap();
        assert!(reservation_id > 0);
        assert!(!coins.is_empty());
    }

    #[tokio::test]
    async fn test_maintenance_lock_expiration() {
        let storage = setup_storage().await;

        // Acquire maintenance lock for 2 seconds
        assert!(storage.acquire_maintenance_lock(2).await.unwrap());
        assert!(storage.is_maintenance_mode().await.unwrap());

        // Wait for lock to expire
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Should no longer be in maintenance mode
        assert!(!storage.is_maintenance_mode().await.unwrap());

        // Should be able to acquire lock again
        assert!(storage.acquire_maintenance_lock(60).await.unwrap());
    }
}
