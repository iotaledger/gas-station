// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// The AccessPolicy enum represents the access policy of the gas station.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd, Eq, Ord, Default)]
#[serde(untagged, rename_all = "kebab-case")]
pub enum AccessPolicy {
    #[default]
    /// The access controller is disabled, meaning there is no control over the access.
    Disabled,
    /// The access controller is set to deny for all transactions. You create rules to allow a transactions.
    DenyAll,
    /// The access controller is set to allow for all transactions. You create rules to deny a transactions.
    AllowAll,
}
