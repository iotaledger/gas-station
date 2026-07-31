// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::access_controller::AccessController;
use crate::gas_station::gas_station_core::NANOS_PER_IOTA;
use crate::tx_signer::{SidecarTxSigner, TestTxSigner, TxSigner};
use iota_sdk_crypto::simple::SimpleKeypair;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::fs;
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;

pub mod cold_params;

/// Provides simple YAML load/save helpers for any type that is `Serialize + DeserializeOwned`.
pub trait Config: Serialize + DeserializeOwned {
    fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(serde_yaml::from_reader(fs::File::open(path)?)?)
    }
    fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        Ok(fs::write(path, serde_yaml::to_string(self)?)?)
    }
}

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
// 5 seconds.
pub const DEFAULT_CHECKPOINT_INCLUSION_TIMEOUT_MS: u64 = 5_000;

// Use 127.0.0.1 for tests to avoid OS complaining about permissions.
#[cfg(test)]
pub const LOCALHOST: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
#[cfg(not(test))]
pub const LOCALHOST: Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);

/// Helper function for serde deserialization.
fn default_max_gas_budget() -> u64 {
    DEFAULT_MAX_GAS_BUDGET
}

/// Helper function for serde deserialization.
fn default_checkpoint_inclusion_timeout_ms() -> u64 {
    DEFAULT_CHECKPOINT_INCLUSION_TIMEOUT_MS
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
    /// How long the fullnode should wait for a transaction to reach full
    /// (local) execution / checkpoint inclusion before responding, in
    /// milliseconds. Only used when a request asks to wait for local
    /// execution; passed through as the gRPC `execute_transaction` call's
    /// `checkpoint_inclusion_timeout_ms`.
    #[serde(default = "default_checkpoint_inclusion_timeout_ms")]
    pub checkpoint_inclusion_timeout_ms: u64,
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
            checkpoint_inclusion_timeout_ms: DEFAULT_CHECKPOINT_INCLUSION_TIMEOUT_MS,
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
// NOTE: no `#[cfg_attr(test, derive(PartialEq, Eq))]` here (unlike
// `GasStationStorageConfig` above) -- `SimpleKeypair` (from `iota-sdk-crypto`)
// deliberately does not implement `PartialEq`/`Eq` (its wrapped private-key
// bytes are not meant to be compared), and since neither trait nor type is
// local to this crate, we cannot provide the impl for it ourselves either.
// Deriving on the whole enum would require every variant's fields to satisfy
// the trait even though only `Sidecar` is ever compared in tests; see the
// `TxSignerConfig::Local` case in `mod::test`'s config-compat test below for
// how the `Local` variant is asserted on instead (via the derived public
// key/address, not equality).
pub enum TxSignerConfig {
    Local {
        #[serde(with = "local_keypair")]
        keypair: SimpleKeypair,
    },
    Sidecar {
        #[serde(alias = "sidecar-url")]
        sidecar_url: String,
    },
}

/// Serializes/deserializes a `SimpleKeypair` as `base64(flag || privkey)`,
/// matching the on-disk format the old `iota_types::crypto::IotaKeyPair`'s
/// `Serialize`/`Deserialize` impl produced (`EncodeDecodeBase64::encode_base64`,
/// i.e. base64 over `to_bytes()`, which is `flag || privkey`). Existing
/// on-disk configs with a `Local` signer must keep loading byte-identically
/// after this migration -- see `mod::test`'s config-compat test.
///
/// `iota-sdk-crypto`'s `ToFromFlaggedBytes` trait is the SDK's own equivalent
/// for exactly this `flag || key-bytes` layout, but the crate bundles no
/// base64 helper of its own, so the base64 encode/decode step uses `base64ct`
/// directly (already a workspace dependency).
mod local_keypair {
    use iota_sdk_crypto::simple::SimpleKeypair;
    use iota_sdk_crypto::ToFromFlaggedBytes;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        keypair: &SimpleKeypair,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        use base64ct::Encoding;

        let encoded = base64ct::Base64::encode_string(&keypair.to_flagged_bytes());
        encoded.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<SimpleKeypair, D::Error> {
        use base64ct::Encoding;

        let encoded = String::deserialize(deserializer)?;
        let bytes = base64ct::Base64::decode_vec(&encoded).map_err(serde::de::Error::custom)?;
        SimpleKeypair::from_flagged_bytes(&bytes).map_err(serde::de::Error::custom)
    }
}

impl Default for TxSignerConfig {
    fn default() -> Self {
        // Only used as a config fallback (e.g. when generating a sample
        // config); never loaded from -- or persisted to -- a real deployment.
        let keypair = SimpleKeypair::from(iota_sdk_crypto::ed25519::Ed25519PrivateKey::generate(
            rand::rngs::OsRng,
        ));
        Self::Local { keypair }
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
        // `TxSignerConfig` no longer derives `PartialEq` (see the type's own
        // doc comment for why -- `SimpleKeypair` can't support it), so this
        // is asserted via pattern matching instead of `assert_eq!`.
        assert!(matches!(
            config.signer_config,
            TxSignerConfig::Sidecar { ref sidecar_url } if sidecar_url == "http://localhost:3000"
        ));
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
        assert!(matches!(
            config.signer_config,
            TxSignerConfig::Sidecar { ref sidecar_url } if sidecar_url == "http://localhost:3000"
        ));
        assert_eq!(
            config.storage_config,
            GasStationStorageConfig::Redis {
                redis_url: "redis://localhost:6379".to_string()
            }
        );
    }

    /// Loads a config in the *old* on-disk format (predating this
    /// migration): `signer-config.local.keypair` is a base64 string of
    /// `flag || 32-byte ed25519 private key seed`, exactly what
    /// `iota_types::crypto::IotaKeyPair`'s `Serialize` impl used to produce.
    /// The keypair value below is taken verbatim from a real fixture
    /// committed to this repo before this migration (`config_with_ac.yaml`,
    /// also duplicated in `docker/config.yaml.orig`), to prove that existing
    /// on-disk configs with a `Local` signer keep loading byte-identically
    /// through the new `SimpleKeypair`-based field.
    ///
    /// The expected address was computed independently (Python, ed25519 +
    /// unkeyed BLAKE2b-256, no scheme-flag prefix for ed25519 -- matching
    /// both the old `IotaAddress`-derivation rules in
    /// `iota_types::crypto::SignatureScheme::update_hasher_with_flag` and the
    /// new `iota_sdk_types::Ed25519PublicKey::derive_address`) from the same
    /// base64-decoded seed bytes, as a from-scratch cross-check that loading
    /// this fixture through `SimpleKeypair` produces the exact same key
    /// material the old `IotaKeyPair`-based config did.
    #[test]
    fn test_load_local_signer_old_format_config() {
        let yaml = indoc! {r#"
            signer-config:
                local:
                    keypair: AKJvlwEulZj0Enj/btnZtFpWqQgfwVU7/UaRmycpW6dq
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
        let TxSignerConfig::Local { keypair } = config.signer_config else {
            panic!("expected a Local signer config");
        };

        let expected_address: iota_sdk_types::Address =
            "0xf1bb78a14db34ec8206148eb2a50adf7b4a39e3123350fef20b3f06efe05995c"
                .parse()
                .unwrap();
        assert_eq!(keypair.public_key().derive_address(), expected_address);

        // Round-trips back to the exact same base64 string, confirming the
        // `Serialize` side also matches the old on-disk format byte-for-byte
        // (not just that decoding happens to produce a usable key).
        let reserialized = serde_yaml::to_string(&TxSignerConfig::Local { keypair }).unwrap();
        assert!(reserialized.contains("AKJvlwEulZj0Enj/btnZtFpWqQgfwVU7/UaRmycpW6dq"));
    }
}
