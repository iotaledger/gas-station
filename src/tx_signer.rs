// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::anyhow;
use fastcrypto::encoding::{Base64, Encoding};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use shared_crypto::intent::{Intent, IntentMessage};
use std::str::FromStr;
use std::sync::Arc;
use iota_types::base_types::IotaAddress;
use iota_types::crypto::{Signature, IotaKeyPair};
use iota_types::signature::GenericSignature;
use iota_types::transaction::TransactionData;

#[async_trait::async_trait]
pub trait TxSigner: Send + Sync {
    async fn sign_transaction(&self, tx_data: &TransactionData)
        -> anyhow::Result<GenericSignature>;
    fn get_address(&self) -> IotaAddress;
    fn is_valid_address(&self, address: &IotaAddress) -> bool {
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
    iota_pubkey_address: IotaAddress,
}

pub struct SidecarTxSigner {
    sidecar_url: String,
    client: Client,
    iota_address: IotaAddress,
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
    async fn sign_transaction(
        &self,
        tx_data: &TransactionData,
    ) -> anyhow::Result<GenericSignature> {
        let bytes = Base64::encode(bcs::to_bytes(&tx_data)?);
        let resp = self
            .client
            .post(format!("{}/{}", self.sidecar_url, "sign-transaction"))
            .header("Content-Type", "application/json")
            .json(&json!({"txBytes": bytes}))
            .send()
            .await?;
        let sig_bytes = resp.json::<SignatureResponse>().await?;
        let sig = GenericSignature::from_str(&sig_bytes.signature)
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(sig)
    }

    fn get_address(&self) -> IotaAddress {
        self.iota_address
    }
}

pub struct TestTxSigner {
    keypair: IotaKeyPair,
}

impl TestTxSigner {
    pub fn new(keypair: IotaKeyPair) -> Arc<Self> {
        Arc::new(Self { keypair })
    }
}

#[async_trait::async_trait]
impl TxSigner for TestTxSigner {
    async fn sign_transaction(
        &self,
        tx_data: &TransactionData,
    ) -> anyhow::Result<GenericSignature> {
        let intent_msg = IntentMessage::new(Intent::iota_transaction(), tx_data);
        let sponsor_sig = Signature::new_secure(&intent_msg, &self.keypair).into();
        Ok(sponsor_sig)
    }

    fn get_address(&self) -> IotaAddress {
        (&self.keypair.public()).into()
    }
}
