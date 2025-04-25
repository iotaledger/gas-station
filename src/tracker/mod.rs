//  Copyright (c) 2024 IOTA Stiftung
//  SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use stats_tracker_storage::StatsTrackerStorage;
use std::sync::Arc;

use serde_json::Value;

pub mod stats_tracker_storage;

#[derive(Clone)]
pub struct StatsTracker {
    pub store: Arc<dyn StatsTrackerStorage>,
}

impl StatsTracker {
    pub fn new(storage: Arc<dyn StatsTrackerStorage>) -> Self {
        Self { store: storage }
    }

    pub async fn update_aggr<'a>(
        &self,
        key_meta: impl IntoIterator<Item = (String, Value)> + Send,
        aggregate: &stats_tracker_storage::Aggregate,
        value: i64,
    ) -> Result<i64> {
        let key_meta = key_meta.into_iter().collect::<Vec<_>>();
        self.store.update_aggr(&key_meta, aggregate, value).await
    }
}
