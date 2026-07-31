// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::base64::Base64;
use crate::rpc::effects::{ObjectRefDto, TransactionEffectsDto};
use crate::types::ReservationID;
use iota_sdk_types::{Address, ObjectReference};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
pub struct ReserveGasRequest {
    pub gas_budget: u64,
    pub reserve_duration_secs: u64,
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
pub struct ReserveGasResponse {
    pub result: Option<ReserveGasResult>,
    pub error: Option<String>,
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
pub struct ReserveGasResult {
    #[schemars(with = "String")]
    pub sponsor_address: Address,
    pub reservation_id: ReservationID,
    pub gas_coins: Vec<ObjectRefDto>,
}

impl ReserveGasResponse {
    pub fn new_ok(
        sponsor_address: Address,
        reservation_id: ReservationID,
        gas_coins: Vec<ObjectReference>,
    ) -> Self {
        Self {
            result: Some(ReserveGasResult {
                sponsor_address,
                reservation_id,
                gas_coins: gas_coins.into_iter().map(|c| c.into()).collect(),
            }),
            error: None,
        }
    }

    pub fn new_err(error: anyhow::Error) -> Self {
        Self {
            result: None,
            error: Some(error.to_string()),
        }
    }
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
pub struct ExecuteTxRequest {
    pub reservation_id: ReservationID,
    pub tx_bytes: Base64,
    pub user_sig: Base64,
    pub request_type: Option<ExecuteTransactionRequestType>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ExecuteTransactionRequestType {
    WaitForEffectsCert,
    WaitForLocalExecution,
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
pub struct ExecuteTxResponse {
    pub effects: Option<TransactionEffectsDto>,
    pub error: Option<String>,
}

impl ExecuteTxResponse {
    pub fn new_ok(effects: TransactionEffectsDto) -> Self {
        Self {
            effects: Some(effects),
            error: None,
        }
    }

    pub fn new_err(error: anyhow::Error) -> Self {
        Self {
            effects: None,
            error: Some(error.to_string()),
        }
    }
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
pub struct GasStationResponse<D = ()> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<D> GasStationResponse<D> {
    pub fn new_ok(d: D) -> GasStationResponse<D> {
        Self {
            result: Some(d),
            error: None,
        }
    }

    pub fn new_err(error: anyhow::Error) -> Self {
        Self {
            result: None,
            error: Some(error.to_string()),
        }
    }

    pub fn new_err_from_str(error: impl AsRef<str>) -> Self {
        Self {
            result: None,
            error: Some(error.as_ref().to_string()),
        }
    }
}
