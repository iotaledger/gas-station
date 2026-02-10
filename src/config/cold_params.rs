// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! This module contains the cold params for the gas station.
//! The cold params are the configuration parameters that require full re-initialization
//! of the gas station coin registry.

use std::sync::Arc;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{config::GasStationConfig, storage::Storage};

/// This struct contains the cold params. The cold params requires full rescan of the coins registry.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColdParams {
    pub target_init_balance: Option<u64>,
}

impl ColdParams {
    pub fn from_config(config: &GasStationConfig) -> Self {
        Self {
            target_init_balance: config
                .coin_init_config
                .as_ref()
                .map(|config| config.target_init_balance),
        }
    }

    /// Checks if the cold params is different from the other cold params.
    pub fn is_different(&self, other: &ColdParams) -> bool {
        self != other
    }

    /// Returns the details of the changes between the current cold params and the other cold params.
    pub fn changes_details(&self, other: &ColdParams) -> Vec<ColdParamChange> {
        let mut changes = Vec::<ColdParamChange>::new();
        if self.target_init_balance != other.target_init_balance {
            changes.push(format!(
                "target_init_balance: {:?} -> {:?}",
                self.target_init_balance, other.target_init_balance
            ));
        }
        changes
    }

    /// Checks if the cold params have changes compared to the storage and returns the changes.
    pub async fn check_if_changed(
        self: &ColdParams,
        storage: &Arc<dyn Storage>,
        key: &str,
    ) -> anyhow::Result<Vec<ColdParamChange>> {
        let maybe_cold_params = storage
            .get_data(key)
            .await
            .context("unable to get cold params from storage")?;

        if maybe_cold_params.is_none() {
            return Ok(vec![]);
        }
        let old_cold_params = serde_json::from_slice(&maybe_cold_params.unwrap()).context(
        format!("unable to deserialize cold params. The entry with the key {key} is not a valid cold params structure",
    ))?;
        let changes = self.changes_details(&old_cold_params);
        Ok(changes)
    }

    pub async fn save_to_storage(
        &self,
        storage: &Arc<dyn Storage>,
        key: &str,
    ) -> anyhow::Result<()> {
        storage
            .set_data(
                key,
                serde_json::to_vec(&self)
                    .context("unable to serialize cold params and save to storage")?,
            )
            .await?;
        Ok(())
    }
}

pub type ColdParamChange = String;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_changes() {
        let params = ColdParams {
            target_init_balance: Some(100),
        };
        assert!(!params.is_different(&params));
        assert!(params.is_different(&ColdParams {
            target_init_balance: Some(200),
        }));
        assert!(params.is_different(&ColdParams {
            target_init_balance: None,
        }));
    }

    #[test]
    fn test_changes_details() {
        let params = ColdParams {
            target_init_balance: Some(100),
        };
        let other_params = ColdParams {
            target_init_balance: Some(200),
        };
        assert_eq!(
            params.changes_details(&other_params),
            vec!["target_init_balance: Some(100) -> Some(200)"]
        );
        assert_eq!(
            other_params.changes_details(&params),
            vec!["target_init_balance: Some(200) -> Some(100)"]
        );
    }
}
