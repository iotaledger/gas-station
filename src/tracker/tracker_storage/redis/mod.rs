use fastcrypto::hash::*;

use anyhow::Result;
use itertools::Itertools;
use redis::aio::ConnectionManager;
use script_manager::ScriptManager;
use serde_json::Value;
use serde_json_canonicalizer::to_string;

use super::{Aggregate, AggregateType, TrackerStorageLike};

mod script_manager;

#[derive(Clone)]
pub struct RedisTrackerStorage {
    conn_manager: ConnectionManager,
    // String format of the sponsor address to avoid converting it to string multiple times.
    sponsor_str: String,
}

impl RedisTrackerStorage {
    pub async fn new(redis_url: impl AsRef<str>, sponsor: impl AsRef<str>) -> Self {
        let client = redis::Client::open(redis_url.as_ref()).unwrap();
        let conn_manager = ConnectionManager::new(client).await.unwrap();
        Self {
            conn_manager,
            sponsor_str: sponsor.as_ref().to_string(),
        }
    }
}

impl TrackerStorageLike for RedisTrackerStorage {
    async fn update_aggr<'a>(
        &self,
        key: impl IntoIterator<Item = (&'a String, &'a Value)>,
        update: &Aggregate,
    ) -> Result<f64> {
        let hash = generate_hash_from_key(key);
        let key = format!("{}:{}:{}", update.name, update.aggr_type, hash);

        match update.aggr_type {
            AggregateType::Sum => {
                let script = ScriptManager::increment_aggr_sum_script();
                let mut conn = self.conn_manager.clone();
                let new_value: f64 = script
                    .arg(self.sponsor_str.to_string())
                    .arg(key)
                    .arg(update.value)
                    .arg(update.window.as_secs())
                    .invoke_async(&mut conn)
                    .await?;
                Ok(new_value)
            }
        }
    }
}

// we should generate the canonical hash key from the given key
fn generate_hash_from_key<'a>(key: impl IntoIterator<Item = (&'a String, &'a Value)>) -> String {
    let mut hash_key = String::new();
    for (k, v) in key.into_iter().sorted_by(|a, b| a.0.cmp(&b.0)) {
        hash_key.push_str(k);
        hash_key.push_str(&to_string(&v).unwrap());
    }

    let mut hasher = Sha256::default();
    hasher.update(hash_key.as_bytes());
    hasher.finalize().to_string()
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use serde_json::json;
    use tokio::time;

    use super::*;

    #[tokio::test]
    async fn update_aggr() {
        let redis_url = "redis://127.0.0.1:6379";
        let sponsor = "sponsor_key";
        let window_size = Duration::from_secs(3);
        let aggregate = Aggregate {
            name: "gas_usage".to_string(),
            window: window_size,
            aggr_type: AggregateType::Sum,
            value: 1.0,
        };
        let key_meta = json!(
        {
            "sender_address" : "0x1234567890abcdef",
        })
        .as_object()
        .unwrap()
        .to_owned();
        let storage = RedisTrackerStorage::new(redis_url, sponsor).await;

        let result = storage.update_aggr(&key_meta, &aggregate).await.unwrap();
        assert_eq!(result, 1.0);

        let result = storage
            .update_aggr(&key_meta, &aggregate.clone().with_value(2.0))
            .await
            .unwrap();
        assert_eq!(result, 3.0);

        time::sleep(window_size + Duration::from_secs(1)).await;
        let result = storage
            .update_aggr(&key_meta, &aggregate.clone().with_value(2.0))
            .await
            .unwrap();
        assert_eq!(result, 2.0);
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
        let key_reversed = json!(
            {
                "a": map_data_reversed,
            }
        );

        let hash_key = generate_hash_from_key(key.as_object().unwrap());
        let hash_key_reversed = generate_hash_from_key(key_reversed.as_object().unwrap());

        assert_eq!(hash_key, hash_key_reversed);
    }
}
