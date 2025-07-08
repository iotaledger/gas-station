// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use tracing::warn;

pub mod access_controller;
pub mod benchmarks;
pub mod command;
pub mod config;
pub mod errors;
pub mod gas_station;
pub mod gas_station_initializer;
pub mod iota_client;
pub mod logging;
pub mod metrics;
pub mod rpc;
pub mod storage;
pub mod tracker;

#[cfg(test)]
pub mod test_env;
pub mod tx_signer;
pub mod types;

pub const AUTH_ENV_NAME: &str = "GAS_STATION_AUTH";
pub const TRANSACTION_LOGGING_ENV_NAME: &str = "TRANSACTIONS_LOGGING";
pub const TRANSACTION_LOGGING_TARGET_NAME: &str = "transactions";
pub const GIT_REVISION: &str = {
    if let Some(revision) = option_env!("GIT_REVISION") {
        revision
    } else {
        let version = git_version::git_version!(
            args = ["--always", "--abbrev=12", "--dirty", "--exclude", "*"],
            fallback = ""
        );

        if version.is_empty() {
            panic!("unable to query git revision");
        }
        version
    }
};
pub const VERSION: &str = const_str::concat!(env!("CARGO_PKG_VERSION"), "-", GIT_REVISION);

pub fn read_auth_env() -> Option<String> {
    use std::sync::OnceLock;
    static AUTH_TOKEN: OnceLock<Option<String>> = OnceLock::new();

    AUTH_TOKEN
        .get_or_init(|| match std::env::var(AUTH_ENV_NAME) {
            Ok(token) => Some(token),
            Err(_) => {
                warn!(
                    "⚠️  {} environment variable is not set. Authorization is disabled! ⚠️",
                    AUTH_ENV_NAME
                );
                None
            }
        })
        .clone()
}
