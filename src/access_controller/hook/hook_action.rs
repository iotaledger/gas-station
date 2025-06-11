// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::access_controller::hook::{
    ExecuteTxGasStationRequest, ExecuteTxHookRequest, ExecuteTxOkResponse, ExecuteTxRequestPayload,
};
use crate::access_controller::rule::TransactionContext;

const HOOK_REQUEST_TIMEOUT_SECONDS: u64 = 60;

fn convert_header_map_to_vec(ctx: &TransactionContext) -> HashMap<String, Vec<String>> {
    let mut header_hashmap: HashMap<String, Vec<String>> = HashMap::new();
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
            headers: convert_header_map_to_vec(ctx),
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookAction(pub(crate) Url);

impl HookAction {
    /// Call hook to let it decide about transaction processing.
    pub async fn call_hook(
        &self,
        ctx: &TransactionContext,
    ) -> Result<ExecuteTxOkResponse, anyhow::Error> {
        use anyhow::Context;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(HOOK_REQUEST_TIMEOUT_SECONDS))
            .build()?;
        let body = build_execute_tx_hook_request_payload(ctx);
        let res = client.post(self.0.clone()).json(&body).send().await?;

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
