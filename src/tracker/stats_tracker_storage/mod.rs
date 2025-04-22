//  Copyright (c) 2024 IOTA Stiftung
//  SPDX-License-Identifier: Apache-2.0

use std::{
    fmt::{Display, Formatter},
    time::Duration,
};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

pub mod redis;

#[async_trait]
pub trait StatsTrackerStorage: Sync + Send {
    async fn update_aggr(
        &self,
        key_meta: &[(String, Value)],
        update: &Aggregate,
        value: f64,
    ) -> Result<f64>;
}

#[derive(Debug, Clone, Default)]
pub struct Aggregate {
    pub name: String,
    pub window: Duration,
    pub aggr_type: AggregateType,
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
