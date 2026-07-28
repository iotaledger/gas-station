// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{self, Display, Formatter};

use once_cell::sync::OnceCell;
use serde::Serialize;
use tracing_subscriber::filter::Directive;
use tracing_subscriber::EnvFilter;

use crate::{TRANSACTION_LOGGING_ENV_NAME, TRANSACTION_LOGGING_TARGET_NAME};

/// Filter directive used when `RUST_LOG` is not set (or fails to parse).
///
/// Mirrors the default that used to be passed to
/// `telemetry_subscribers::TelemetryConfig::with_log_level`: everything is
/// silenced except this crate, which logs at `debug`.
const DEFAULT_LOG_DIRECTIVE: &str = "off,iota_gas_station=debug";

/// Initialize the global `tracing` subscriber for the running binary.
///
/// `RUST_LOG` takes full precedence over the built-in default when it is set
/// and parses successfully — this matches the previous
/// `telemetry_subscribers` behavior, which built its `EnvFilter` via
/// `EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directives))`.
/// When `RUST_LOG` is unset/invalid, [`DEFAULT_LOG_DIRECTIVE`] is used instead
/// (note: this is a plain fallback, not a merge with `RUST_LOG`).
///
/// Also restores the old `telemetry_subscribers`-era behavior of the
/// `TRANSACTIONS_LOGGING` env var: when set to `"true"`, it forces the
/// `"transactions"` target (used by `rpc/server.rs` to emit full
/// transaction-effects audit records) to `trace` level regardless of the
/// base filter above — this target doesn't match `iota_gas_station=...`
/// directives since it's a bare custom target, not a crate path, so without
/// this it is silently swallowed by `DEFAULT_LOG_DIRECTIVE`'s `off` default.
pub fn init() {
    let mut env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_DIRECTIVE));
    if std::env::var(TRANSACTION_LOGGING_ENV_NAME).as_deref() == Ok("true") {
        let directive: Directive = format!("{TRANSACTION_LOGGING_TARGET_NAME}=trace")
            .parse()
            .expect("static directive string is always valid");
        env_filter = env_filter.add_directive(directive);
    }
    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}

static TEST_LOGGER: OnceCell<()> = OnceCell::new();

/// Idempotent `tracing` subscriber setup for tests.
///
/// Safe to call from many tests/threads: only the first call installs the
/// global subscriber, later calls are no-ops. This replaces
/// `telemetry_subscribers::init_for_testing()`, which used the same
/// once-only pattern (there via `once_cell::sync::Lazy`).
pub fn init_for_testing() {
    TEST_LOGGER.get_or_init(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::builder()
                    .with_default_directive(tracing::level_filters::LevelFilter::INFO.into())
                    .from_env_lossy(),
            )
            .with_file(true)
            .with_line_number(true)
            .with_test_writer()
            .try_init();
    });
}

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
