// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{self, Display, Formatter};

use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TxLogMessage<D: Serialize + Clone> {
    pub timestamp: i64,
    pub level: String,
    pub host: String,
    pub message: String,
    pub details: D,
}

impl<D> TxLogMessage<D>
where
    D: Serialize + Clone,
{
    pub fn new(transaction_effects: D) -> Self {
        let hostname = hostname::get().unwrap().to_string_lossy().to_string();
        Self {
            timestamp: chrono::Utc::now().timestamp(),
            level: "trace".to_string(),
            host: hostname,
            message: "transaction data".to_string(),
            details: transaction_effects,
        }
    }
}

impl<D> Display for TxLogMessage<D>
where
    D: Serialize + Clone,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let serialized = serde_json::to_string(&self).map_err(|_| fmt::Error)?;
        write!(f, "{}", serialized)
    }
}
