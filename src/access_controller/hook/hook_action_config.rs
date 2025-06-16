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
#[serde(untagged)]
pub enum HookActionConfig {
    HookActionUrl(Url),
    HookActionDetailed(HookActionDetailed),
}

impl HookActionConfig {
    pub fn initialize(&mut self) -> Result<(), anyhow::Error> {
        match self {
            HookActionConfig::HookActionDetailed(detailed) => detailed.initialize(),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookActionDetailed {
    pub(crate) url: Url,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) headers: Option<HookActionHeaders>,
    #[serde(skip)]
    pub(crate) header_map: Option<HeaderMap>,
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

impl PartialEq for HookActionDetailed {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
            && self.headers == other.headers
            && self.header_map == other.header_map
    }
}

impl Eq for HookActionDetailed {}

pub type HookActionHeaders = BTreeMap<String, Vec<String>>;
