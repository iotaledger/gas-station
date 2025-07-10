// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::types::ReservationID;
use fastcrypto::encoding::Base64;
use iota_json_rpc_types::{IotaObjectRef, IotaTransactionBlockEffects};
use iota_types::{
    base_types::{IotaAddress, ObjectRef},
    quorum_driver_types::ExecuteTransactionRequestType,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// 2 IOTA.
pub const MAX_BUDGET: u64 = 2_000_000_000;

// 10 mins.
pub const MAX_DURATION_S: u64 = 10 * 60;

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
pub struct ReserveGasRequest {
    pub gas_budget: u64,
    pub reserve_duration_secs: u64,
}

impl ReserveGasRequest {
    pub fn check_validity(&self) -> anyhow::Result<()> {
        if self.gas_budget == 0 {
            anyhow::bail!("Gas budget must be positive");
        }
        if self.gas_budget > MAX_BUDGET {
            anyhow::bail!("Gas budget must be less than {}", MAX_BUDGET);
        }
        if self.reserve_duration_secs == 0 {
            anyhow::bail!("Reserve duration must be positive");
        }
        if self.reserve_duration_secs > MAX_DURATION_S {
            anyhow::bail!(
                "Reserve duration must be less than {} seconds",
                MAX_DURATION_S
            );
        }
        Ok(())
    }
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
pub struct ReserveGasResponse {
    pub result: Option<ReserveGasResult>,
    pub error: Option<String>,
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
pub struct ReserveGasResult {
    pub sponsor_address: IotaAddress,
    pub reservation_id: ReservationID,
    pub gas_coins: Vec<IotaObjectRef>,
}

impl ReserveGasResponse {
    pub fn new_ok(
        sponsor_address: IotaAddress,
        reservation_id: ReservationID,
        gas_coins: Vec<ObjectRef>,
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
    // use separate `..._with` attributes instead of just `with` to prevent
    // [issue](https://github.com/GREsau/schemars/issues/89#issuecomment-933746151) with `JsonSchema` derive
    #[serde(
        default,
        deserialize_with = "option_execute_transaction_request_type::deserialize",
        serialize_with = "option_execute_transaction_request_type::serialize"
    )]
    pub request_type: Option<ExecuteTransactionRequestType>,
}

/// Helper module, that allows to convert `iota`s `ExecuteTransactionRequestType` to a lowercase representation of it
/// as a value in a REST request. `serde`s `remote` attribute does currently not support optional values, so added a helper
/// module as [suggested](https://github.com/serde-rs/serde/issues/1301#issuecomment-394108486).
pub(crate) mod option_execute_transaction_request_type {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::ExecuteTransactionRequestType as ExternalExecuteTransactionRequestType;

    #[derive(Serialize, Deserialize, Clone, Debug)]
    #[serde(
        remote = "ExternalExecuteTransactionRequestType",
        rename_all = "camelCase"
    )]
    pub enum ExecuteTransactionRequestType {
        WaitForEffectsCert,
        WaitForLocalExecution,
    }

    pub fn serialize<S>(
        value: &Option<ExternalExecuteTransactionRequestType>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Helper<'a>(
            #[serde(with = "ExecuteTransactionRequestType")]
            &'a ExternalExecuteTransactionRequestType,
        );

        value.as_ref().map(Helper).serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<Option<ExternalExecuteTransactionRequestType>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper(
            #[serde(with = "ExecuteTransactionRequestType")] ExternalExecuteTransactionRequestType,
        );

        let helper = Option::deserialize(deserializer)?;
        Ok(helper.map(|Helper(external)| external))
    }
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
pub struct ExecuteTxResponse {
    pub effects: Option<IotaTransactionBlockEffects>,
    pub error: Option<String>,
}

impl ExecuteTxResponse {
    pub fn new_ok(effects: IotaTransactionBlockEffects) -> Self {
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
