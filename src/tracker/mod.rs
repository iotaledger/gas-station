//  Copyright (c) 2024 IOTA Stiftung
//  SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use std::sync::Arc;
use tracker_storage::TrackerStorageLike;

use serde_json::Value;

pub mod tracker_storage;

#[derive(Clone)]
pub struct StatsTracker {
    pub store: Arc<dyn TrackerStorageLike>,
}

impl StatsTracker {
    pub fn new(storage: Arc<dyn TrackerStorageLike>) -> Self {
        Self { store: storage }
    }

    pub async fn update_aggr<'a>(
        &self,
        key_meta: impl IntoIterator<Item = (String, Value)> + Send,
        update: &tracker_storage::Aggregate,
    ) -> Result<f64> {
        let key_meta = key_meta.into_iter().collect::<Vec<_>>();
        self.store.update_aggr(&key_meta, update).await
    }
}
