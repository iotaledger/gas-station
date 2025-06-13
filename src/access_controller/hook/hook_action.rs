// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use anyhow::Context as _;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use url::Url;

fn hash_map_to_header_map(hash_map: &HookActionHeaders) -> Result<HeaderMap, anyhow::Error> {
    let mut header_map = HeaderMap::new();
    for (key, values) in hash_map.iter() {
        for value in values.iter() {
            header_map.append(
                HeaderName::from_bytes(key.as_bytes()).context("failed to parse header name")?,
                HeaderValue::from_str(&value).context("failed to parse header value")?,
            );
        }
    }

    Ok(header_map)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookActionDetailed {
    url: Url,
    #[serde(skip_serializing_if = "Option::is_none")]
    headers: Option<HookActionHeaders>,
    #[serde(skip)]
    header_map: Option<HeaderMap>,
}

impl HookActionDetailed {
    pub fn initialize(&mut self) -> Result<(), anyhow::Error> {
        if let Some(headers) = &self.headers {
            self.header_map = Some(hash_map_to_header_map(&headers)?);
        }

        Ok(())
    }

    pub fn new(url: Url) -> Self {
        Self {
            url,
            headers: None,
            header_map: None,
        }
    }

    pub fn with_headers(mut self, headers: HookActionHeaders) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Return configured request headers parsed on-the-fly or cached (if available from [`Self::initialize()`]).
    pub fn header_map(&self) -> Result<Option<HeaderMap>, anyhow::Error> {
        if self.header_map.is_some() {
            // return pre-cached headers, if available
            return Ok(self.header_map.clone());
        }
        if let Some(headers) = &self.headers {
            // or parse them from config values
            let header_map = hash_map_to_header_map(headers)?;
            return Ok(Some(header_map));
        }
        Ok(None)
    }
}

pub type HookActionHeaders = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum HookAction {
    HookActionUrl(Url),
    HookActionDetailed(HookActionDetailed),
}

impl HookAction {
    pub fn initialize(&mut self) -> Result<(), anyhow::Error> {
        match self {
            HookAction::HookActionDetailed(detailed) => detailed.initialize(),
            _ => Ok(()),
        }
    }

    /// Url the hook call is made against.
    pub fn url(&self) -> &Url {
        match self {
            HookAction::HookActionUrl(url) => url,
            HookAction::HookActionDetailed(HookActionDetailed { url, .. }) => url,
        }
    }

    /// Headers, that will be used in the hook call.
    pub fn headers(&self) -> Option<&HookActionHeaders> {
        match self {
            HookAction::HookActionDetailed(HookActionDetailed {
                headers: Some(headers),
                ..
            }) => Some(&headers),
            _ => None,
        }
    }

    /// Get configured request headers as `HeaderMap`.
    pub fn header_map(&self) -> Result<Option<HeaderMap>, anyhow::Error> {
        match self {
            HookAction::HookActionDetailed(detailed) => detailed.header_map(),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    const EXAMPLE_URL: &str = "http://example.org/";

    mod hook_action_url {
        use super::*;

        const SERIALIZED_HOOK_ACTION_URL: &str = indoc! {r###"
            ---
            "http://example.org/"
        "###};

        #[test]
        fn can_be_serialized() {
            let action = HookAction::HookActionUrl(Url::parse(EXAMPLE_URL).unwrap());
            let serialized = serde_yaml::to_string(&action).unwrap();
            assert_eq!(serialized, SERIALIZED_HOOK_ACTION_URL);
        }

        #[test]
        fn can_be_deserialized() {
            let action = HookAction::HookActionUrl(Url::parse(EXAMPLE_URL).unwrap());
            let deserialized: HookAction =
                serde_yaml::from_str(&SERIALIZED_HOOK_ACTION_URL).unwrap();
            assert_eq!(deserialized, action);
        }
    }

    mod hook_action_detailed {
        use super::*;

        mod without_headers {
            use super::*;

            const SERIALIZED_HOOK_ACTION: &str = indoc! {r###"
                ---
                url: "http://example.org/"
            "###};

            #[test]
            fn can_be_serialized() {
                let action = HookAction::HookActionDetailed(HookActionDetailed::new(
                    Url::parse(EXAMPLE_URL).unwrap(),
                ));

                let serialized = serde_yaml::to_string(&action).unwrap();

                assert_eq!(serialized, SERIALIZED_HOOK_ACTION);
            }
            #[test]
            fn can_be_deserialized() {
                let action = HookAction::HookActionDetailed(HookActionDetailed::new(
                    Url::parse(EXAMPLE_URL).unwrap(),
                ));

                let deserialized: HookAction =
                    serde_yaml::from_str(&SERIALIZED_HOOK_ACTION).unwrap();

                assert_eq!(deserialized, action);
            }
        }

        mod with_headers {
            use super::*;

            const SERIALIZED_HOOK_ACTION: &str = indoc! {r###"
                ---
                url: "http://example.org/"
                headers:
                  authorization:
                    - Bearer this-could-be-your-auth-token
                  foobar:
                    - foo
                    - bar
                  test-response:
                    - "{\"decision\": \"allow\"}"
            "###};

            fn get_test_action() -> HookAction {
                let mut hash_map: HookActionHeaders = HookActionHeaders::new();
                hash_map.insert(
                    "authorization".to_string(),
                    vec!["Bearer this-could-be-your-auth-token".to_string()],
                );
                hash_map.insert(
                    "foobar".to_string(),
                    vec!["foo".to_string(), "bar".to_string()],
                );
                hash_map.insert(
                    "test-response".to_string(),
                    vec![r#"{"decision": "allow"}"#.to_string()],
                );

                HookAction::HookActionDetailed(
                    HookActionDetailed::new(Url::parse(EXAMPLE_URL).unwrap())
                        .with_headers(hash_map),
                )
            }

            #[test]
            fn can_be_serialized() {
                let action = get_test_action();
                let serialized = serde_yaml::to_string(&action).unwrap();
                assert_eq!(serialized, SERIALIZED_HOOK_ACTION);
            }

            #[test]
            fn can_be_deserialized() {
                let action = get_test_action();

                let deserialized: HookAction =
                    serde_yaml::from_str(&SERIALIZED_HOOK_ACTION).unwrap();

                assert_eq!(deserialized, action);
            }

            #[test]
            fn can_be_initialized() {
                let action = get_test_action();
                let result = action.header_map();
                assert!(result.is_ok())
            }

            #[test]
            fn can_return_a_header_map() {
                let action = get_test_action();

                let header_map = action.header_map().unwrap().unwrap();

                let mut authorization = header_map.get_all("authorization").iter();
                assert_eq!(
                    authorization.next().unwrap(),
                    "Bearer this-could-be-your-auth-token"
                );
                assert_eq!(authorization.next(), None);
                let mut test_response_values = header_map.get_all("test-response").iter();
                assert_eq!(
                    test_response_values.next().unwrap(),
                    r#"{"decision": "allow"}"#
                );
                assert_eq!(test_response_values.next(), None);
                let mut foobar_values = header_map.get_all("foobar").iter();
                assert_eq!(foobar_values.next().unwrap(), "foo");
                assert_eq!(foobar_values.next().unwrap(), "bar");
                assert_eq!(foobar_values.next(), None);
            }
        }
    }
}
