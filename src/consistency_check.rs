// Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! Quick Consistency Check module for validating registry state against network state.
//!
//! This module provides functionality to compare aggregate balances and coin counts
//! between the Redis registry and the actual network state. Since exact matches
//! are rare, thresholds are used to detect meaningful divergence.

use tracing::warn;

/// Default threshold for balance divergence detection (20% difference).
pub const DEFAULT_BALANCE_DIVERGENCE_THRESHOLD_PERCENT: f64 = 20.0;

/// Default threshold for coin count divergence detection (20% difference).
pub const DEFAULT_COIN_COUNT_DIVERGENCE_THRESHOLD_PERCENT: f64 = 20.0;

/// Result of a consistency check comparison.
#[derive(Debug, Clone)]
pub struct ConsistencyCheckResult {
    /// Whether balance divergence was detected (above threshold).
    pub balance_diverged: bool,
    /// Whether coin count divergence was detected (above threshold).
    pub coin_count_diverged: bool,
    pub registry_balance: u64,
    pub network_balance: u64,
    pub balance_divergence_percent: f64,
    pub registry_coin_count: u64,
    pub network_coin_count: u64,
    pub coin_count_divergence_percent: f64,
}

impl ConsistencyCheckResult {
    /// Returns true if any divergence was detected.
    pub fn has_divergence(&self) -> bool {
        self.balance_diverged || self.coin_count_diverged
    }
}

/// Calculates the percentage difference between two values.
/// Returns 0.0 if both values are 0 to avoid division by zero.
fn calculate_divergence_percent(value1: u64, value2: u64) -> f64 {
    let max_val = value1.max(value2);
    if max_val == 0 {
        return 0.0;
    }
    let diff = value1.abs_diff(value2);
    (diff as f64 / max_val as f64) * 100.0
}

/// Validates the consistency between registry state and network state.
pub fn validate_consistency(
    registry_balance: u64,
    registry_coin_count: u64,
    network_balance: u64,
    network_coin_count: u64,
    balance_threshold_percent: Option<f64>,
    coin_count_threshold_percent: Option<f64>,
) -> ConsistencyCheckResult {
    let balance_threshold =
        balance_threshold_percent.unwrap_or(DEFAULT_BALANCE_DIVERGENCE_THRESHOLD_PERCENT);
    let coin_count_threshold =
        coin_count_threshold_percent.unwrap_or(DEFAULT_COIN_COUNT_DIVERGENCE_THRESHOLD_PERCENT);

    let balance_divergence_percent =
        calculate_divergence_percent(registry_balance, network_balance);
    let coin_count_divergence_percent =
        calculate_divergence_percent(registry_coin_count, network_coin_count);

    let balance_diverged = balance_divergence_percent > balance_threshold;
    let coin_count_diverged = coin_count_divergence_percent > coin_count_threshold;

    ConsistencyCheckResult {
        balance_diverged,
        coin_count_diverged,
        registry_balance,
        network_balance,
        balance_divergence_percent,
        registry_coin_count,
        network_coin_count,
        coin_count_divergence_percent,
    }
}

/// Logs warnings if consistency check detects divergence.
pub fn log_consistency_warnings(result: &ConsistencyCheckResult) {
    if result.balance_diverged {
        warn!(
            registry_balance = %result.registry_balance,
            network_balance = %result.network_balance,
            divergence_percent = format!("{:.2}%", result.balance_divergence_percent),
            "Consistency check: Balance divergence detected between registry and network. \
             This may indicate registry inconsistency or in-flight transactions. Please consider re-initializing the gas station. Use the --allow-reinit and --delete-coin-registry flags to allow re-initialization."
        );
    }

    if result.coin_count_diverged {
        warn!(
            registry_coin_count = %result.registry_coin_count,
            network_coin_count = %result.network_coin_count,
            divergence_percent = format!("{:.2}%", result.coin_count_divergence_percent),
            "Consistency check: Coin count divergence detected between registry and network. \
             This may indicate registry inconsistency or missing coins. Please consider re-initializing the gas station. Use the --allow-reinit and --delete-coin-registry flags to allow re-initialization."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_divergence_percent() {
        // No divergence
        assert_eq!(calculate_divergence_percent(100, 100), 0.0);

        // 10% divergence (both directions - order shouldn't matter)
        assert!((calculate_divergence_percent(100, 90) - 10.0).abs() < 0.001);
        assert!((calculate_divergence_percent(90, 100) - 10.0).abs() < 0.001);

        // 50% divergence
        assert!((calculate_divergence_percent(100, 50) - 50.0).abs() < 0.001);

        // Zero handling
        assert_eq!(calculate_divergence_percent(0, 0), 0.0);
        assert_eq!(calculate_divergence_percent(100, 0), 100.0);
        assert_eq!(calculate_divergence_percent(0, 100), 100.0);

        // Large values (overflow protection)
        let large = u64::MAX;
        assert_eq!(calculate_divergence_percent(large, large), 0.0);
        assert_eq!(calculate_divergence_percent(large, 0), 100.0);
        assert!((calculate_divergence_percent(large, large / 2) - 50.0).abs() < 1.0);
    }

    #[test]
    fn test_validate_consistency_within_threshold() {
        // Exact match - no divergence
        let result = validate_consistency(1000, 100, 1000, 100, None, None);
        assert!(!result.has_divergence());

        // 3% difference - within default 20% threshold
        let result = validate_consistency(1000, 100, 970, 97, None, None);
        assert!(!result.has_divergence());
    }

    #[test]
    fn test_validate_consistency_balance_divergence_only() {
        // 25% balance difference exceeds threshold, coin count unchanged
        let result = validate_consistency(1000, 100, 750, 100, None, None);
        assert!(result.balance_diverged);
        assert!(!result.coin_count_diverged);
    }

    #[test]
    fn test_validate_consistency_coin_count_divergence_only() {
        // 25% coin count difference exceeds threshold, balance unchanged
        let result = validate_consistency(1000, 100, 1000, 75, None, None);
        assert!(!result.balance_diverged);
        assert!(result.coin_count_diverged);
    }

    #[test]
    fn test_validate_consistency_custom_thresholds() {
        // 3% difference with 2% threshold - should trigger
        let result = validate_consistency(1000, 100, 970, 97, Some(2.0), Some(2.0));
        assert!(result.has_divergence());

        // 3% difference with 10% threshold - should not trigger
        let result = validate_consistency(1000, 100, 970, 97, Some(10.0), Some(10.0));
        assert!(!result.has_divergence());
    }
}
