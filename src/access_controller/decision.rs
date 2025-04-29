// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::ops::{BitAnd, BitOr};

use serde::{Deserialize, Serialize};

use super::{policy::AccessPolicy, predicates::Action};

/// The Decision enum represents the decision of the access controller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

impl From<Action> for Decision {
    fn from(action: Action) -> Self {
        match action {
            Action::Allow => Decision::Allow,
            Action::Deny => Decision::Deny,
        }
    }
}

impl BitAnd for Decision {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Decision::Allow, Decision::Allow) => Decision::Allow,
            _ => Decision::Deny,
        }
    }
}

impl BitOr for Decision {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Decision::Deny, Decision::Deny) => Decision::Deny,
            _ => Decision::Allow,
        }
    }
}

impl From<AccessPolicy> for Decision {
    fn from(policy: AccessPolicy) -> Self {
        match policy {
            AccessPolicy::AllowAll => Decision::Allow,
            AccessPolicy::DenyAll => Decision::Deny,
            AccessPolicy::Disabled => Decision::Allow,
        }
    }
}
