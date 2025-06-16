// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context as _;
use axum::http::HeaderMap;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use super::{
    ExecuteTxGasStationRequest, ExecuteTxHookRequest, ExecuteTxOkResponse, ExecuteTxRequestPayload,
    HookActionConfig, HookActionDetailed, HookActionHeaders,
};

use crate::access_controller::rule::TransactionContext;

const HOOK_REQUEST_TIMEOUT_SECONDS: u64 = 60;

fn header_map_to_hash_map(ctx: &TransactionContext) -> HookActionHeaders {
    let mut header_hashmap: HookActionHeaders = HookActionHeaders::new();
    for (k, v) in ctx.headers.clone() {
        let k = k.map(|v| v.to_string()).unwrap_or_default();
        let v = String::from_utf8_lossy(v.as_bytes()).into_owned();
        header_hashmap.entry(k).or_insert_with(Vec::new).push(v);
    }

    header_hashmap
}

fn build_execute_tx_hook_request_payload(ctx: &TransactionContext) -> ExecuteTxHookRequest {
    ExecuteTxHookRequest {
        execute_tx_request: ExecuteTxGasStationRequest {
            payload: ExecuteTxRequestPayload {
                reservation_id: ctx.reservation_id,
                tx_bytes: ctx.tx_bytes.encoded(),
                user_sig: ctx.user_sig.encoded(),
            },
            headers: header_map_to_hash_map(ctx),
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HookAction {
    config: HookActionConfig,
    #[serde(skip)]
    pub(crate) http_client: Client, // <- move to new caller struct? (--> "intercept" calls for test)
}

impl HookAction {
    pub fn new_url(url: Url) -> Result<Self, anyhow::Error> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(HOOK_REQUEST_TIMEOUT_SECONDS))
            .build()?;
        Ok(Self {
            config: HookActionConfig::HookActionUrl(url),
            http_client,
        })
    }

    pub fn new_detailed(
        url: Url,
        headers: Option<HookActionHeaders>,
    ) -> Result<Self, anyhow::Error> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(HOOK_REQUEST_TIMEOUT_SECONDS))
            .build()?;
        let mut detailed = HookActionDetailed::new(url);
        if let Some(headers) = headers {
            detailed = detailed.with_headers(headers);
        };

        Ok(Self {
            config: HookActionConfig::HookActionDetailed(detailed),
            http_client,
        })
    }

    pub fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.config.initialize()
    }

    /// Url the hook call is made against.
    pub fn url(&self) -> &Url {
        match &self.config {
            HookActionConfig::HookActionUrl(url) => url,
            HookActionConfig::HookActionDetailed(HookActionDetailed { url, .. }) => url,
        }
    }

    /// Headers, that will be used in the hook call.
    pub fn headers(&self) -> Option<&HookActionHeaders> {
        match &self.config {
            HookActionConfig::HookActionDetailed(HookActionDetailed {
                headers: Some(headers),
                ..
            }) => Some(headers),
            _ => None,
        }
    }

    /// Get configured request headers as `HeaderMap`.
    pub fn header_map(&self) -> Result<Option<HeaderMap>, anyhow::Error> {
        match &self.config {
            HookActionConfig::HookActionDetailed(detailed) => detailed.header_map(),
            _ => Ok(None),
        }
    }

    /// Call hook to let it decide about transaction processing.
    pub async fn call_hook(
        &self,
        ctx: &TransactionContext,
    ) -> Result<ExecuteTxOkResponse, anyhow::Error> {
        let body = build_execute_tx_hook_request_payload(ctx);
        let res = self
            .http_client
            .post(self.url().clone())
            .headers(self.header_map()?.unwrap_or_default())
            .json(&body)
            .send()
            .await?;

        if res.status().is_success() {
            return res
                .json()
                .await
                .context("failed to parse successful hook response body");
        } else {
            let message = format!(
                "hook call failed with status {}; {}",
                res.status(),
                res.text().await.unwrap_or_default()
            );
            anyhow::bail!(message);
        }
    }
}

impl PartialEq for HookAction {
    fn eq(&self, other: &Self) -> bool {
        self.config == other.config
    }
}

impl Eq for HookAction {}

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
            let action = HookAction::new_url(Url::parse(EXAMPLE_URL).unwrap()).unwrap();
            let serialized = serde_yaml::to_string(&action).unwrap();
            assert_eq!(serialized, SERIALIZED_HOOK_ACTION_URL);
        }

        #[test]
        fn can_be_deserialized() {
            let action = HookAction::new_url(Url::parse(EXAMPLE_URL).unwrap()).unwrap();
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
                let action =
                    HookAction::new_detailed(Url::parse(EXAMPLE_URL).unwrap(), None).unwrap();
                let serialized = serde_yaml::to_string(&action).unwrap();
                assert_eq!(serialized, SERIALIZED_HOOK_ACTION);
            }

            #[test]
            fn can_be_deserialized() {
                let action =
                    HookAction::new_detailed(Url::parse(EXAMPLE_URL).unwrap(), None).unwrap();

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
                let mut headers: HookActionHeaders = HookActionHeaders::new();
                headers.insert(
                    "authorization".to_string(),
                    vec!["Bearer this-could-be-your-auth-token".to_string()],
                );
                headers.insert(
                    "foobar".to_string(),
                    vec!["foo".to_string(), "bar".to_string()],
                );
                headers.insert(
                    "test-response".to_string(),
                    vec![r#"{"decision": "allow"}"#.to_string()],
                );

                HookAction::new_detailed(Url::parse(EXAMPLE_URL).unwrap(), Some(headers)).unwrap()
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
