use anyhow::Result;
use std::sync::Arc;

use serde_json::Value;

pub mod tracker_storage;

#[derive(Clone)]
pub struct StatsTracker<T> {
    pub storage: Arc<T>,
}

impl<T: tracker_storage::TrackerStorageLike> StatsTracker<T> {
    pub fn new(storage: T) -> Self {
        Self {
            storage: Arc::new(storage),
        }
    }

    pub async fn update_aggr<'a>(
        &self,
        key_meta: impl IntoIterator<Item = (&'a String, &'a Value)> + Send,
        update: &tracker_storage::Aggregate,
    ) -> Result<f64> {
        self.storage.update_aggr(key_meta, update).await
    }
}
