// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::rpc::effects::ObjectRefDto;
use anyhow::bail;
#[cfg(test)]
use iota_sdk_types::{ObjectDigest, Version};
use iota_sdk_types::{ObjectId, ObjectReference};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GasCoin {
    pub object_ref: ObjectReference,
    pub balance: u64,
}

#[derive(Debug, JsonSchema, Serialize, Deserialize)]
pub struct IotaGasCoin {
    pub object_ref: ObjectRefDto,
    pub balance: u64,
}

// `IotaGasCoin` is the JSON-RPC-facing (camelCase `{objectId, version, digest}`) wire
// shape for a gas coin's object reference; see `crate::rpc::effects::ObjectRefDto` for
// the exact serde contract. `GasCoin.object_ref` is the SDK's own `ObjectReference`
// (snake_case, not wire-facing). Both `ObjectRefDto` and `ObjectReference` are native
// `iota_sdk_types`-backed types now, so this is a direct field-for-field conversion --
// no string round-tripping needed (that was only ever a stopgap bridge to the old,
// now-removed `iota_json_rpc_types::IotaObjectRef`, from before `rpc/rpc_types.rs` was
// migrated).
impl From<GasCoin> for IotaGasCoin {
    fn from(gas_coin: GasCoin) -> Self {
        Self {
            object_ref: gas_coin.object_ref.into(),
            balance: gas_coin.balance,
        }
    }
}

impl From<IotaGasCoin> for GasCoin {
    fn from(gas_coin: IotaGasCoin) -> Self {
        Self {
            object_ref: gas_coin.object_ref.into(),
            balance: gas_coin.balance,
        }
    }
}

pub type ReservationID = u64;
pub type ExpirationTimeMs = u64;
pub type GasGroupKey = ObjectId;

#[derive(Clone, Default, Debug)]
pub struct UpdatedGasGroup {
    pub updated_gas_coins: Vec<GasCoin>,
    pub deleted_gas_coins: Vec<ObjectId>,
}

impl UpdatedGasGroup {
    pub fn new(updated_gas_coins: Vec<GasCoin>, deleted_gas_coins: Vec<ObjectId>) -> Self {
        Self {
            updated_gas_coins,
            deleted_gas_coins,
        }
    }
    pub fn get_group_key(&self) -> anyhow::Result<GasGroupKey> {
        let all_ids: BTreeSet<_> = self
            .updated_gas_coins
            .iter()
            .map(|coin| &coin.object_ref.object_id)
            .chain(&self.deleted_gas_coins)
            .collect();
        if all_ids.is_empty() {
            bail!("Gas group is empty");
        }
        if all_ids.len() != self.updated_gas_coins.len() + self.deleted_gas_coins.len() {
            bail!("Gas group contains duplicate ids");
        }
        // unwrap safe since we checked that it's not empty.
        Ok(*all_ids.into_iter().next().unwrap())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReservedGasGroup {
    pub objects: BTreeSet<ObjectId>,
    pub expiration_time: ExpirationTimeMs,
}

impl ReservedGasGroup {
    pub fn get_key(&self) -> GasGroupKey {
        *self.objects.iter().next().unwrap()
    }
}

/// Test-only helper equivalent to the old `iota_types::base_types::random_object_ref()`:
/// a random object id paired with version 0 and the all-zero digest (the new SDK has no
/// direct equivalent, so we reconstruct it here for use by `storage`'s test modules).
#[cfg(test)]
pub fn random_object_ref() -> ObjectReference {
    ObjectReference::new(
        ObjectId::random(),
        Version::from_u64(0),
        ObjectDigest::new([0; 32]),
    )
}
