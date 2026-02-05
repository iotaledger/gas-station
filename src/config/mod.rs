// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::access_controller::AccessController;
use crate::tx_signer::{SidecarTxSigner, TestTxSigner, TxSigner};
use iota_config::Config;
use iota_types::crypto::{get_account_key_pair, IotaKeyPair};
use iota_types::gas_coin::NANOS_PER_IOTA;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::net::Ipv4Addr;
use std::sync::Arc;

pub mod cold_params;

pub const DEFAULT_RPC_PORT: u16 = 9527;
pub const DEFAULT_METRICS_PORT: u16 = 9184;
// 0.1 IOTA.
pub const DEFAULT_INIT_COIN_BALANCE: u64 = NANOS_PER_IOTA / 10;
// 24 hours.
const DEFAULT_COIN_POOL_REFRESH_INTERVAL_SEC: u64 = 60 * 60 * 24;
// 1500 IOTA.
pub const DEFAULT_DAILY_GAS_USAGE_CAP: u64 = 1500 * NANOS_PER_IOTA;
// 2 IOTA.
pub const DEFAULT_MAX_GAS_BUDGET: u64 = 2 * NANOS_PER_IOTA;

// Use 127.0.0.1 for tests to avoid OS complaining about permissions.
#[cfg(test)]
pub const LOCALHOST: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
#[cfg(not(test))]
pub const LOCALHOST: Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);

/// Helper function for serde deserialization.
fn default_max_gas_budget() -> u64 {
    DEFAULT_MAX_GAS_BUDGET
}

#[serde_as]
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct GasStationConfig {
    pub signer_config: TxSignerConfig,
    pub rpc_host_ip: Ipv4Addr,
    pub rpc_port: u16,
    pub metrics_port: u16,
    pub storage_config: GasStationStorageConfig,
    pub fullnode_url: String,
    /// An optional basic auth when connecting to the fullnode. If specified, the format is
    /// (username, password).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fullnode_basic_auth: Option<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin_init_config: Option<CoinInitConfig>,
    pub daily_gas_usage_cap: u64,
    #[serde(default = "default_max_gas_budget")]
    pub max_gas_budget: u64,
    #[serde(default)]
    pub access_controller: AccessController,
}

impl Config for GasStationConfig {}

impl Default for GasStationConfig {
    fn default() -> Self {
        GasStationConfig {
            signer_config: TxSignerConfig::default(),
            rpc_host_ip: LOCALHOST,
            rpc_port: DEFAULT_RPC_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
            storage_config: GasStationStorageConfig::default(),
            fullnode_url: "http://localhost:9000".to_string(),
            fullnode_basic_auth: None,
            coin_init_config: Some(CoinInitConfig::default()),
            daily_gas_usage_cap: DEFAULT_DAILY_GAS_USAGE_CAP,
            max_gas_budget: DEFAULT_MAX_GAS_BUDGET,
            access_controller: AccessController::default(),
        }
    }
}

#[serde_as]
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum GasStationStorageConfig {
    Redis {
        #[serde(alias = "redis-url")]
        redis_url: String,
    },
}

impl Default for GasStationStorageConfig {
    fn default() -> Self {
        Self::Redis {
            redis_url: "redis://127.0.0.1:6379".to_string(),
        }
    }
}

#[serde_as]
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum TxSignerConfig {
    Local {
        keypair: IotaKeyPair,
    },
    Sidecar {
        #[serde(alias = "sidecar-url")]
        sidecar_url: String,
    },
}

impl Default for TxSignerConfig {
    fn default() -> Self {
        let (_, keypair) = get_account_key_pair();
        Self::Local {
            keypair: keypair.into(),
        }
    }
}

impl TxSignerConfig {
    pub async fn new_signer(self) -> Arc<dyn TxSigner> {
        match self {
            TxSignerConfig::Local { keypair } => TestTxSigner::new(keypair),
            TxSignerConfig::Sidecar { sidecar_url } => SidecarTxSigner::new(sidecar_url).await,
        }
    }
}

#[serde_as]
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct CoinInitConfig {
    /// When we split a new gas coin, what is the target balance for the new coins, in NANOs.
    pub target_init_balance: u64,
    /// How often do we look at whether there are new coins added to the sponsor account that
    /// requires initialization, i.e. splitting into smaller coins and add them to the Gas Station.
    /// This is in seconds.
    pub refresh_interval_sec: u64,
}

impl Default for CoinInitConfig {
    fn default() -> Self {
        CoinInitConfig {
            target_init_balance: DEFAULT_INIT_COIN_BALANCE,
            refresh_interval_sec: DEFAULT_COIN_POOL_REFRESH_INTERVAL_SEC,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_deserialize_config_urls_kebab_case() {
        let yaml = indoc! {r#"
            signer-config:
                sidecar:
                    sidecar-url: http://localhost:3000
            rpc-host-ip: 0.0.0.0
            rpc-port: 9527
            metrics-port: 9184
            storage-config:
                redis:
                    redis-url: "redis://localhost:6379"
            fullnode-url: "https://api.devnet.iota.cafe"
            daily-gas-usage-cap: 1500000000000
            max-gas-budget: 2000000000
        "#};
        let config: GasStationConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.signer_config,
            TxSignerConfig::Sidecar {
                sidecar_url: "http://localhost:3000".to_string()
            }
        );
        assert_eq!(
            config.storage_config,
            GasStationStorageConfig::Redis {
                redis_url: "redis://localhost:6379".to_string()
            }
        );
    }

    #[test]
    fn test_deserialize_config_urls_camel_case() {
        let yaml = indoc! {r#"
            signer-config:
                sidecar:
                    sidecar_url: http://localhost:3000
            rpc-host-ip: 0.0.0.0
            rpc-port: 9527
            metrics-port: 9184
            storage-config:
                redis:
                    redis_url: "redis://localhost:6379"
            fullnode-url: "https://api.devnet.iota.cafe"
            daily-gas-usage-cap: 1500000000000
            max-gas-budget: 2000000000
        "#};
        let config: GasStationConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.signer_config,
            TxSignerConfig::Sidecar {
                sidecar_url: "http://localhost:3000".to_string()
            }
        );
        assert_eq!(
            config.storage_config,
            GasStationStorageConfig::Redis {
                redis_url: "redis://localhost:6379".to_string()
            }
        );
    }
}
