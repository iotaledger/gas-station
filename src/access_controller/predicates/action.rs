// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::access_controller::hook::HookAction;

/// Action enum represents the action of the access controller. It can be either Allow or Deny.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    #[default]
    Allow,
    Deny,
    #[serde(untagged)]
    HookAction(HookAction),
}

impl Action {
    pub fn initialize(&mut self) -> Result<(), anyhow::Error> {
        match self {
            Action::HookAction(hook_action) => hook_action.initialize(),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod test {
    use url::Url;

    use super::Action;

    use crate::access_controller::hook::HookAction;

    #[test]
    fn test_deserialize_valid_actions() {
        let values_and_expected = vec![
            (r#""allow""#, Action::Allow),
            (r#""deny""#, Action::Deny),
            (
                r#""http://example.org/""#,
                Action::HookAction(
                    HookAction::new_url(Url::parse("http://example.org/").unwrap()).unwrap(),
                ),
            ),
        ];

        for (serialized, expected) in values_and_expected {
            assert_eq!(
                serde_json::from_str::<Action>(serialized).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn test_deserialize_invalid_actions() {
        let values = vec![r#""invalid"", r#"abc://example.org/"#];

        for serialized in values {
            assert!(serde_json::from_str::<Action>(serialized).is_err());
        }
    }

    #[test]
    fn test_serialize_actions() {
        let values_and_expected = vec![
            (Action::Allow, r#""allow""#),
            (Action::Deny, r#""deny""#),
            (
                Action::HookAction(
                    HookAction::new_url(Url::parse("http://example.org/").unwrap()).unwrap(),
                ),
                r#""http://example.org/""#,
            ),
        ];

        for (value, expected) in values_and_expected {
            assert_eq!(serde_json::to_string(&value).unwrap(), expected);
        }
    }
}
