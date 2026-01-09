// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! Migration module for Redis key namespace changes.
//!
//! This module handles migration from the old key format (wallet_address:key_name)
//! to the new format (network:wallet:registry:key_name).
//!
//! Old format example: `0x1234...abcd:available_gas_coins`
//! New format example: `api.devnet.iota.cafe_443:0x1234...abcd:registry:available_gas_coins`

use redis::aio::ConnectionManager;
use redis::Script;
use tracing::{debug, info, warn};

/// Result of migration operation
#[derive(Debug, Clone)]
pub struct MigrationResult {
    pub migrated_count: i64,
    pub skipped_count: i64,
    pub error_count: i64,
}

impl MigrationResult {
    pub fn is_success(&self) -> bool {
        self.error_count == 0
    }

    pub fn has_migrations(&self) -> bool {
        self.migrated_count > 0
    }
}

/// Lua script to check if old namespace exists
fn check_old_namespace_exists_script() -> Script {
    Script::new(include_str!("lua_scripts/check_old_namespace_exists.lua"))
}

/// Lua script to migrate keys from old namespace to new namespace
fn migrate_keys_script() -> Script {
    Script::new(include_str!("lua_scripts/migrate_keys.lua"))
}

/// Check if old namespace format exists for the given sponsor address.
pub async fn check_old_namespace_exists(
    conn: &mut ConnectionManager,
    sponsor_address: &str,
) -> anyhow::Result<bool> {
    let result: i32 = check_old_namespace_exists_script()
        .arg(sponsor_address)
        .invoke_async(conn)
        .await?;

    Ok(result == 1)
}

/// Migrate keys from old namespace format to new namespace format.
pub async fn migrate_keys(
    conn: &mut ConnectionManager,
    sponsor_address: &str,
    new_namespace: &str,
) -> anyhow::Result<MigrationResult> {
    let (migrated, skipped, errors): (i64, i64, i64) = migrate_keys_script()
        .arg(sponsor_address)
        .arg(new_namespace)
        .invoke_async(conn)
        .await?;

    Ok(MigrationResult {
        migrated_count: migrated,
        skipped_count: skipped,
        error_count: errors,
    })
}

/// Check and perform migration if needed.
/// Returns `None` if no migration is needed, otherwise returns the migration result.
pub async fn maybe_migrate(
    conn: &mut ConnectionManager,
    sponsor_address: &str,
    new_namespace: &str,
) -> anyhow::Result<Option<MigrationResult>> {
    let old_exists = check_old_namespace_exists(conn, sponsor_address).await?;
    if !old_exists {
        debug!(
            "No old namespace found for sponsor {}. No migration needed.",
            sponsor_address
        );
        return Ok(None);
    }
    info!(
        "Old namespace found for sponsor {}. Starting migration to new namespace: {}",
        sponsor_address, new_namespace
    );

    let result = migrate_keys(conn, sponsor_address, new_namespace).await?;
    if result.is_success() {
        info!(
            "Migration completed successfully. Migrated: {}, Skipped: {}, Errors: {}",
            result.migrated_count, result.skipped_count, result.error_count
        );
    } else {
        warn!(
            "Migration completed with errors. Migrated: {}, Skipped: {}, Errors: {}",
            result.migrated_count, result.skipped_count, result.error_count
        );
    }

    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use redis::AsyncCommands;

    async fn setup_test_connection() -> ConnectionManager {
        let client = redis::Client::open("redis://127.0.0.1:6379").unwrap();
        ConnectionManager::new(client).await.unwrap()
    }

    async fn flush_db(conn: &mut ConnectionManager) {
        redis::cmd("FLUSHDB")
            .query_async::<_, String>(conn)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_check_old_namespace_not_exists() {
        let mut conn = setup_test_connection().await;
        flush_db(&mut conn).await;

        let sponsor = "0x0000000000000000000000000000000000000000000000000000000000000000";
        let exists = check_old_namespace_exists(&mut conn, sponsor)
            .await
            .unwrap();

        assert!(!exists);
    }

    #[tokio::test]
    async fn test_check_old_namespace_exists_with_initialized_flag() {
        let mut conn = setup_test_connection().await;
        flush_db(&mut conn).await;

        let sponsor = "0x1111111111111111111111111111111111111111111111111111111111111111";
        let old_key = format!("{}:initialized", sponsor);

        conn.set::<_, _, ()>(&old_key, "1").await.unwrap();

        let exists = check_old_namespace_exists(&mut conn, sponsor)
            .await
            .unwrap();
        assert!(exists);
    }

    #[tokio::test]
    async fn test_check_old_namespace_exists_with_coins() {
        let mut conn = setup_test_connection().await;
        flush_db(&mut conn).await;

        let sponsor = "0x2222222222222222222222222222222222222222222222222222222222222222";
        let old_key = format!("{}:available_gas_coins", sponsor);

        conn.rpush::<_, _, ()>(&old_key, "100,0x123,1,abc")
            .await
            .unwrap();

        let exists = check_old_namespace_exists(&mut conn, sponsor)
            .await
            .unwrap();
        assert!(exists);
    }

    #[tokio::test]
    async fn test_migrate_keys_basic() {
        let mut conn = setup_test_connection().await;
        flush_db(&mut conn).await;

        let sponsor = "0x3333333333333333333333333333333333333333333333333333333333333333";
        let new_namespace = "test_host_443:0x3333333333333333333333333333333333333333333333333333333333333333:registry";

        // Set up old namespace data
        conn.set::<_, _, ()>(format!("{}:initialized", sponsor), "1")
            .await
            .unwrap();
        conn.set::<_, _, ()>(format!("{}:available_coin_count", sponsor), "5")
            .await
            .unwrap();
        conn.set::<_, _, ()>(format!("{}:available_coin_total_balance", sponsor), "500")
            .await
            .unwrap();
        conn.rpush::<_, _, ()>(
            format!("{}:available_gas_coins", sponsor),
            vec!["100,0x1,1,a", "200,0x2,2,b"],
        )
        .await
        .unwrap();
        conn.set::<_, _, ()>(format!("{}:next_reservation_id", sponsor), "10")
            .await
            .unwrap();

        let result = migrate_keys(&mut conn, sponsor, new_namespace)
            .await
            .unwrap();

        assert!(result.is_success());
        assert!(result.migrated_count >= 5); // At least 5 keys should be migrated

        let old_init_exists: bool = conn
            .exists(format!("{}:initialized", sponsor))
            .await
            .unwrap();
        assert!(!old_init_exists);

        let new_init: String = conn
            .get(format!("{}:initialized", new_namespace))
            .await
            .unwrap();
        assert_eq!(new_init, "1");

        let new_count: String = conn
            .get(format!("{}:available_coin_count", new_namespace))
            .await
            .unwrap();
        assert_eq!(new_count, "5");

        let new_balance: String = conn
            .get(format!("{}:available_coin_total_balance", new_namespace))
            .await
            .unwrap();
        assert_eq!(new_balance, "500");

        let new_coins: Vec<String> = conn
            .lrange(format!("{}:available_gas_coins", new_namespace), 0, -1)
            .await
            .unwrap();
        assert_eq!(new_coins.len(), 2);
        assert_eq!(new_coins[0], "100,0x1,1,a");
        assert_eq!(new_coins[1], "200,0x2,2,b");
    }

    #[tokio::test]
    async fn test_migrate_keys_with_reservations() {
        let mut conn = setup_test_connection().await;
        flush_db(&mut conn).await;

        let sponsor = "0x4444444444444444444444444444444444444444444444444444444444444444";
        let new_namespace = "test_host_443:0x4444444444444444444444444444444444444444444444444444444444444444:registry";

        conn.set::<_, _, ()>(format!("{}:initialized", sponsor), "1")
            .await
            .unwrap();
        conn.set::<_, _, ()>(format!("{}:next_reservation_id", sponsor), "3")
            .await
            .unwrap();
        conn.set::<_, _, ()>(format!("{}:1", sponsor), "0x100,0x200")
            .await
            .unwrap();
        conn.set::<_, _, ()>(format!("{}:2", sponsor), "0x300")
            .await
            .unwrap();
        conn.zadd::<_, _, _, ()>(format!("{}:expiration_queue", sponsor), "1", 1000)
            .await
            .unwrap();
        conn.zadd::<_, _, _, ()>(format!("{}:expiration_queue", sponsor), "2", 2000)
            .await
            .unwrap();

        let result = migrate_keys(&mut conn, sponsor, new_namespace)
            .await
            .unwrap();

        assert!(result.is_success());

        let res1: String = conn.get(format!("{}:1", new_namespace)).await.unwrap();
        assert_eq!(res1, "0x100,0x200");

        let res2: String = conn.get(format!("{}:2", new_namespace)).await.unwrap();
        assert_eq!(res2, "0x300");

        let queue: Vec<(String, f64)> = conn
            .zrange_withscores(format!("{}:expiration_queue", new_namespace), 0, -1)
            .await
            .unwrap();
        assert_eq!(queue.len(), 2);
    }

    #[tokio::test]
    async fn test_migrate_skips_existing_keys() {
        let mut conn = setup_test_connection().await;
        flush_db(&mut conn).await;

        let sponsor = "0x5555555555555555555555555555555555555555555555555555555555555555";
        let new_namespace = "test_host_443:0x5555555555555555555555555555555555555555555555555555555555555555:registry";

        conn.set::<_, _, ()>(format!("{}:initialized", sponsor), "1")
            .await
            .unwrap();
        conn.set::<_, _, ()>(format!("{}:available_coin_count", sponsor), "10")
            .await
            .unwrap();

        conn.set::<_, _, ()>(format!("{}:available_coin_count", new_namespace), "20")
            .await
            .unwrap();

        let result = migrate_keys(&mut conn, sponsor, new_namespace)
            .await
            .unwrap();

        assert!(result.is_success());
        assert!(result.skipped_count >= 1); // At least one key should be skipped

        let count: String = conn
            .get(format!("{}:available_coin_count", new_namespace))
            .await
            .unwrap();
        assert_eq!(count, "20");
    }

    #[tokio::test]
    async fn test_maybe_migrate_no_migration_needed() {
        let mut conn = setup_test_connection().await;
        flush_db(&mut conn).await;

        let sponsor = "0x6666666666666666666666666666666666666666666666666666666666666666";
        let new_namespace = "test_host_443:0x6666666666666666666666666666666666666666666666666666666666666666:registry";

        let result = maybe_migrate(&mut conn, sponsor, new_namespace)
            .await
            .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_maybe_migrate_performs_migration() {
        let mut conn = setup_test_connection().await;
        flush_db(&mut conn).await;

        let sponsor = "0x7777777777777777777777777777777777777777777777777777777777777777";
        let new_namespace = "test_host_443:0x7777777777777777777777777777777777777777777777777777777777777777:registry";

        conn.set::<_, _, ()>(format!("{}:initialized", sponsor), "1")
            .await
            .unwrap();
        conn.set::<_, _, ()>(format!("{}:available_coin_count", sponsor), "100")
            .await
            .unwrap();

        let result = maybe_migrate(&mut conn, sponsor, new_namespace)
            .await
            .unwrap();

        assert!(result.is_some());
        let migration_result = result.unwrap();
        assert!(migration_result.is_success());
        assert!(migration_result.has_migrations());
    }

    #[tokio::test]
    async fn test_full_migration_scenario() {
        let mut conn = setup_test_connection().await;
        flush_db(&mut conn).await;

        let sponsor = "0x8888888888888888888888888888888888888888888888888888888888888888";
        let new_namespace = "api.devnet.iota.cafe_443:0x8888888888888888888888888888888888888888888888888888888888888888:registry";

        conn.set::<_, _, ()>(format!("{}:initialized", sponsor), "1")
            .await
            .unwrap();
        conn.set::<_, _, ()>(format!("{}:available_coin_count", sponsor), "1000")
            .await
            .unwrap();
        conn.set::<_, _, ()>(
            format!("{}:available_coin_total_balance", sponsor),
            "100000000",
        )
        .await
        .unwrap();
        conn.set::<_, _, ()>(format!("{}:next_reservation_id", sponsor), "50")
            .await
            .unwrap();

        for i in 0..10 {
            conn.rpush::<_, _, ()>(
                format!("{}:available_gas_coins", sponsor),
                format!("{},0x{:064x},1,digest{}", 100000, i, i),
            )
            .await
            .unwrap();
        }

        let result = maybe_migrate(&mut conn, sponsor, new_namespace)
            .await
            .unwrap();

        assert!(result.is_some());
        let migration_result = result.unwrap();
        assert!(migration_result.is_success());

        let init: String = conn
            .get(format!("{}:initialized", new_namespace))
            .await
            .unwrap();
        assert_eq!(init, "1");

        let count: String = conn
            .get(format!("{}:available_coin_count", new_namespace))
            .await
            .unwrap();
        assert_eq!(count, "1000");

        let balance: String = conn
            .get(format!("{}:available_coin_total_balance", new_namespace))
            .await
            .unwrap();
        assert_eq!(balance, "100000000");

        let coins: Vec<String> = conn
            .lrange(format!("{}:available_gas_coins", new_namespace), 0, -1)
            .await
            .unwrap();
        assert_eq!(coins.len(), 10);
    }
}
