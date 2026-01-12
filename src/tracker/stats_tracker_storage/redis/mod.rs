// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use fastcrypto::hash::*;

use anyhow::Result;
use itertools::Itertools;
use redis::aio::ConnectionManager;
use script_manager::ScriptManager;
use serde_json::Value;
use serde_json_canonicalizer::to_string;

use crate::config::GasStationStorageConfig;

use super::{Aggregate, AggregateType, StatsTrackerStorage};

mod script_manager;

#[derive(Clone)]
pub struct RedisStatsTrackerStorage {
    conn_manager: ConnectionManager,
    // Namespace for the keys in the redis database. This is used to avoid keys collision between different networks.
    pub namespace: String,
}

impl RedisStatsTrackerStorage {
    pub async fn new(redis_url: impl AsRef<str>, namespace_prefix: &str) -> Self {
        let namespace = get_tracker_namespace(namespace_prefix);
        let client = redis::Client::open(redis_url.as_ref()).unwrap();
        let conn_manager = ConnectionManager::new(client).await.unwrap();
        Self {
            conn_manager,
            namespace,
        }
    }

    #[cfg(test)]
    pub async fn new_localhost() -> RedisStatsTrackerStorage {
        Self::new("redis://127.0.0.1:6379", "test").await
    }
}

fn get_tracker_namespace(namespace_prefix: &str) -> String {
    format!("{}:tracker", namespace_prefix)
}

#[async_trait]
impl StatsTrackerStorage for RedisStatsTrackerStorage {
    async fn update_aggr(
        &self,
        key: &[(String, Value)],
        aggr: &Aggregate,
        value: i64,
    ) -> Result<i64> {
        let key = get_redis_aggr_key(&aggr.name, aggr.aggr_type, key);

        match aggr.aggr_type {
            AggregateType::Sum => {
                let script = ScriptManager::increment_aggr_sum_script();
                let mut conn = self.conn_manager.clone();
                let new_value: i64 = script
                    .arg(self.namespace.clone())
                    .arg(key)
                    .arg(value)
                    .arg(aggr.window.as_secs())
                    .invoke_async(&mut conn)
                    .await?;
                Ok(new_value)
            }
        }
    }
}

pub(crate) fn get_redis_aggr_key(
    aggr_name: &str,
    aggr_type: AggregateType,
    key: &[(String, Value)],
) -> String {
    let hash = generate_hash_from_key(key);
    format!("{}:{}:{}", aggr_name, aggr_type, hash)
}

// we should generate the canonical hash key from the given key
fn generate_hash_from_key<'a>(key: &[(String, Value)]) -> String {
    let mut hash_key = String::new();
    for (k, v) in key.into_iter().sorted_by(|a, b| a.0.cmp(&b.0)) {
        hash_key.push_str(&k);
        hash_key.push_str(&to_string(&v).unwrap());
    }

    let mut hasher = Sha256::default();
    hasher.update(hash_key.as_bytes());
    hasher.finalize().to_string()
}

pub async fn connect_stats_storage(
    config: &GasStationStorageConfig,
    namespace_prefix: &str,
) -> RedisStatsTrackerStorage {
    let storage = match config {
        GasStationStorageConfig::Redis { redis_url } => {
            RedisStatsTrackerStorage::new(redis_url, namespace_prefix).await
        }
    };

    storage
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use serde_json::json;
    use tokio::time;

    use super::*;

    #[tokio::test]
    async fn update_aggr() {
        let storage = RedisStatsTrackerStorage::new_localhost().await;
        let window_size = Duration::from_secs(2);
        let aggregate = Aggregate {
            name: "gas_usage".to_string(),
            window: window_size,
            aggr_type: AggregateType::Sum,
        };
        let key_meta = json!(
        {
            "sender_address" : "0x1234567890abcdef",
        })
        .as_object()
        .unwrap()
        .to_owned()
        .into_iter()
        .collect::<Vec<_>>();

        let result = storage.update_aggr(&key_meta, &aggregate, 1).await.unwrap();
        assert_eq!(result, 1);

        let result = storage.update_aggr(&key_meta, &aggregate, 2).await.unwrap();
        assert_eq!(result, 3);

        time::sleep(window_size + Duration::from_secs(1)).await;
        let result = storage.update_aggr(&key_meta, &aggregate, 2).await.unwrap();
        assert_eq!(result, 2);
    }

    #[test]
    fn test_calculate_hash_map() {
        let map_data = json!({
            "alpha": "alpha_value",
            "bravo": "bravo_value",
        });

        let map_data_reversed = json!({
            "bravo": "bravo_value",
            "alpha": "alpha_value",
        });

        let key = json!(
            {
                "a": map_data,
            }
        );
        let key_rev = json!(
            {
                "a": map_data_reversed,
            }
        );

        let key_map = key
            .as_object()
            .unwrap()
            .to_owned()
            .into_iter()
            .collect::<Vec<_>>();
        let key_map_rev = key_rev
            .as_object()
            .unwrap()
            .to_owned()
            .into_iter()
            .collect::<Vec<_>>();
        let hash_key = generate_hash_from_key(&key_map);
        let hash_key_reversed = generate_hash_from_key(&key_map_rev);

        assert_eq!(hash_key, hash_key_reversed);
    }
}
