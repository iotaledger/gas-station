use std::{
    fmt::{Display, Formatter},
    time::Duration,
};

use anyhow::Result;
use serde_json::Value;

pub mod redis;

pub trait TrackerStorageLike: Sync + Send {
    async fn update_aggr<'a>(
        &self,
        key_meta: impl IntoIterator<Item = (&'a String, &'a Value)> + Send,
        udpate: &Aggregate,
    ) -> Result<f64>;
}

#[derive(Debug, Clone, Default)]
pub struct Aggregate {
    pub name: String,
    pub window: Duration,
    pub aggr_type: AggregateType,
    pub value: f64,
}

impl Aggregate {
    pub fn with_name(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }
    pub fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }
    pub fn with_aggr_type(mut self, aggr_function: AggregateType) -> Self {
        self.aggr_type = aggr_function;
        self
    }
    pub fn with_value(mut self, value: f64) -> Self {
        self.value = value;
        self
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub enum AggregateType {
    #[default]
    Sum,
}

impl Display for AggregateType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AggregateType::Sum => write!(f, "sum"),
        }
    }
}
