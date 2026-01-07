use serde::{Deserialize, Serialize};

use crate::config::GasStationConfig;

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

    /// Checks if the cold params has changes compared to the other cold params.
    pub fn has_changes(&self, other: &ColdParams) -> bool {
        self != other
    }

    /// Returns the details of the changes compared to the other cold params.
    pub fn changes_details(&self, other: &ColdParams) -> String {
        let mut changes = Vec::new();
        if self.target_init_balance != other.target_init_balance {
            changes.push(format!(
                "target_init_balance: {:?} -> {:?}",
                self.target_init_balance, other.target_init_balance
            ));
        }
        changes.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_changes() {
        let params = ColdParams {
            target_init_balance: Some(100),
        };
        assert!(!params.has_changes(&params));
        assert!(params.has_changes(&ColdParams {
            target_init_balance: Some(200),
        }));
        assert!(params.has_changes(&ColdParams {
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
            "target_init_balance: Some(100) -> Some(200)"
        );
        assert_eq!(
            other_params.changes_details(&params),
            "target_init_balance: Some(200) -> Some(100)"
        );
    }
}
