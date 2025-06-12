// Copyright (c) 2025 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::access_controller::hook::HookAction;
use crate::access_controller::hook::{
    ExecuteTxGasStationRequest, ExecuteTxHookRequest, ExecuteTxOkResponse, ExecuteTxRequestPayload,
    HookActionHeaders,
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

// fn hash_map_to_header_map(hash_map: HookActionHeaders) -> Result<HeaderMap, anyhow::Error> {
//     let mut header_map = HeaderMap::new();
//     for (key, values) in hash_map.iter() {
//         for value in values.iter() {
//             header_map.append(
//                 HeaderName::from_bytes(key.as_bytes()).context("failed to parse header name")?,
//                 HeaderValue::from_str(&value).context("failed to parse header value")?,
//             );
//         }
//     }

//     Ok(header_map)
// }

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
        let res = client
            .post(self.url().clone())
            .headers(
                self.header_map()?
                    .map(|v| v.clone())
                    .unwrap_or_default()
                    .clone(),
            )
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
