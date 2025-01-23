// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Action enum represents the action of the access controller. It can be either Allow or Deny.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    #[default]
    Allow,
    Deny,
}
