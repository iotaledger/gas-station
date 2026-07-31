// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::base64::Base64;
use anyhow::anyhow;
use iota_sdk_crypto::simple::SimpleKeypair;
use iota_sdk_crypto::IotaSigner;
use iota_sdk_types::{Address, Transaction, UserSignature};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::str::FromStr;
use std::sync::Arc;

#[async_trait::async_trait]
pub trait TxSigner: Send + Sync {
    async fn sign_transaction(&self, tx: &Transaction) -> anyhow::Result<UserSignature>;
    fn get_address(&self) -> Address;
    fn is_valid_address(&self, address: &Address) -> bool {
        self.get_address() == *address
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureResponse {
    signature: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IotaAddressResponse {
    iota_pubkey_address: Address,
}

pub struct SidecarTxSigner {
    sidecar_url: String,
    client: Client,
    iota_address: Address,
}

impl SidecarTxSigner {
    pub async fn new(sidecar_url: String) -> Arc<Self> {
        let client = Client::new();
        let resp = client
            .get(format!("{}/{}", sidecar_url, "get-pubkey-address"))
            .send()
            .await
            .unwrap_or_else(|err| panic!("Failed to get pubkey address: {}", err));
        let iota_address = resp
            .json::<IotaAddressResponse>()
            .await
            .unwrap_or_else(|err| panic!("Failed to parse address response: {}", err))
            .iota_pubkey_address;
        Arc::new(Self {
            sidecar_url,
            client,
            iota_address,
        })
    }
}

#[async_trait::async_trait]
impl TxSigner for SidecarTxSigner {
    async fn sign_transaction(&self, tx: &Transaction) -> anyhow::Result<UserSignature> {
        let bytes = Base64::encode(bcs::to_bytes(tx)?);
        let resp = self
            .client
            .post(format!("{}/{}", self.sidecar_url, "sign-transaction"))
            .header("Content-Type", "application/json")
            .json(&json!({"txBytes": bytes}))
            .send()
            .await?;
        let sig_bytes = resp.json::<SignatureResponse>().await?;
        let sig = UserSignature::from_str(&sig_bytes.signature)
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(sig)
    }

    fn get_address(&self) -> Address {
        self.iota_address
    }
}

pub struct TestTxSigner {
    keypair: SimpleKeypair,
}

impl TestTxSigner {
    pub fn new(keypair: SimpleKeypair) -> Arc<Self> {
        Arc::new(Self { keypair })
    }
}

#[async_trait::async_trait]
impl TxSigner for TestTxSigner {
    async fn sign_transaction(&self, tx: &Transaction) -> anyhow::Result<UserSignature> {
        // `IotaSigner::sign_transaction` is blanket-implemented (in
        // `iota-sdk-crypto`) for any `T: Signer<UserSignature>`, which
        // `SimpleKeypair` is. It computes the signing digest internally as
        // `blake2b(Intent::iota_transaction().to_bytes() || bcs(tx))` --
        // see `iota_sdk_types::Transaction::signing_digest` in ground truth
        // (`iota-sdk-types/src/hash.rs`) -- so the old manual two-step
        // (`IntentMessage::new(Intent::iota_transaction(), tx_data)` +
        // `Signature::new_secure(&intent_msg, &keypair)`) collapses into this
        // single call.
        self.keypair
            .sign_transaction(tx)
            .map_err(|err| anyhow!(err.to_string()))
    }

    fn get_address(&self) -> Address {
        self.keypair.public_key().derive_address()
    }
}
