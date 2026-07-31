// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! JSON-RPC-shaped transaction effects DTOs.
//!
//! The SDK's own [`iota_sdk_types::TransactionEffects`] (and its `V1` payload,
//! [`TransactionEffectsV1`]) use a *compact* representation: a single
//! `changed_objects: Vec<ChangedObject>` list (each entry carrying its
//! before/after state plus an [`IdOperation`]), rather than the flat
//! `created`/`mutated`/`deleted`/... lists that this service's existing
//! `/v1/execute_tx` clients (including this SDK's own [`super::client`]) have
//! always received on the wire and depend on byte-for-byte.
//!
//! This module defines that flat, JSON-RPC-shaped wire type (mirroring
//! `IotaTransactionBlockEffectsV1` from `iota-json-rpc-types` on the
//! `iotaledger/iota` monorepo) and derives it from the SDK's compact
//! `TransactionEffectsV1`. Both the *derivation* (the `created()`/`mutated()`/
//! .../`gas_object()` logic below) and the *serde shape* (every
//! `#[serde(...)]`/`#[schemars(...)]` annotation) are ported from the
//! monorepo's `develop` branch, which has already done this exact migration
//! for the node itself against the same pinned `iota-sdk-types` revision this
//! crate uses:
//!   - `crates/iota-types/src/effects/v1.rs`
//!     (`impl TransactionEffectsAPI for TransactionEffectsV1`) for the
//!     derivation logic.
//!   - `crates/iota-json-rpc-types/src/iota_transaction.rs`
//!     (`IotaTransactionBlockEffectsV1` and
//!     `impl<T: TransactionEffectsAPI> From<T> for IotaTransactionBlockEffectsV1`)
//!     for the wire shape and the flattening logic.
//!   - `crates/iota-json-rpc-types/src/{iota_gas_cost_summary,iota_owner,iota_object,iota_primitives}.rs`
//!     for the exact per-field serde/schemars adapters (`IotaGasCostSummary`,
//!     `OwnerSchema`, `ObjectRefSchema`, `SequenceNumberU64`,
//!     `SequenceNumberString`, ...) that this module's local DTOs replicate.
//!
//! Renames encountered while porting (monorepo `iota-types` re-exports the SDK
//! types under old names in some places -- this module always uses the real
//! `iota_sdk_types` names): `ObjectIn::NotExist` -> `Missing`,
//! `ObjectOut::NotExist` -> `Missing`, `IDOperation` -> `IdOperation`.
//!
//! One design simplification versus the monorepo: `IotaTransactionBlockEffects`
//! there is an enum (`V1(IotaTransactionBlockEffectsV1)`) internally tagged on
//! `messageVersion`, so that on the wire the tag and the `V1` payload's fields
//! are flattened into one JSON object. Since the SDK's `TransactionEffects`
//! only ever has a single `V1` variant, [`TransactionEffectsDto`] below bakes
//! that flattening in directly as a single struct with a `message_version`
//! field fixed to `"v1"`, rather than modeling the enum -- this produces
//! byte-identical JSON while being simpler to define and to test.

use iota_sdk_types::{
    Address, EpochId, ExecutionStatus, GasCostSummary, IdOperation, ObjectDigest, ObjectId,
    ObjectIn, ObjectOut, ObjectReference, Owner, TransactionDigest, TransactionEffects,
    TransactionEffectsV1, TransactionEventsDigest, UnchangedSharedKind, Version,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};

/// JSON-RPC-shaped object reference: `{"objectId": "0x..", "version": N, "digest": ".."}`.
///
/// This intentionally does *not* reuse `iota_sdk_types::ObjectReference`'s own
/// `Serialize` impl (which is snake_case and not part of our wire contract);
/// note also that `version` here is a plain JSON **number**, unlike
/// [`ModifiedAtVersionDto::sequence_number`] below, which is a **string** --
/// this mirrors the monorepo's `ObjectRefSchema` (numeric, via
/// `SequenceNumberU64`) vs. `SequenceNumberString` distinction exactly; the
/// two are not interchangeable despite both ultimately wrapping a `Version`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObjectRefDto {
    #[schemars(with = "String")]
    pub object_id: ObjectId,
    pub version: u64,
    #[schemars(with = "String")]
    pub digest: ObjectDigest,
}

impl From<ObjectReference> for ObjectRefDto {
    fn from(r: ObjectReference) -> Self {
        Self {
            object_id: r.object_id,
            version: r.version.as_u64(),
            digest: r.digest,
        }
    }
}

impl From<ObjectRefDto> for ObjectReference {
    fn from(r: ObjectRefDto) -> Self {
        ObjectReference::new(r.object_id, Version::from_u64(r.version), r.digest)
    }
}

/// JSON-RPC-shaped owner, mirroring the monorepo's `OwnerSchema`/`IotaOwner`:
/// `{"AddressOwner": "0x.."}` / `{"ObjectOwner": "0x.."}` /
/// `{"Shared": {"initial_shared_version": N}}` / `"Immutable"`.
///
/// Deliberately *not* `#[serde(rename_all = "camelCase")]`: the monorepo's
/// `OwnerSchema` has no `rename_all` either, so `Shared`'s inner field stays
/// `initial_shared_version` verbatim on the wire (not `initialSharedVersion`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum OwnerDto {
    AddressOwner(#[schemars(with = "String")] Address),
    ObjectOwner(#[schemars(with = "String")] Address),
    Shared { initial_shared_version: u64 },
    Immutable,
}

impl From<Owner> for OwnerDto {
    fn from(owner: Owner) -> Self {
        match owner {
            Owner::Address(address) => OwnerDto::AddressOwner(address),
            Owner::Object(object_id) => OwnerDto::ObjectOwner(*object_id.as_address()),
            Owner::Shared(initial_shared_version) => OwnerDto::Shared {
                initial_shared_version: initial_shared_version.as_u64(),
            },
            Owner::Immutable => OwnerDto::Immutable,
            // `Owner` is `#[non_exhaustive]` in iota-sdk-types; mirrors the monorepo's own
            // defensive arm in `impl From<Owner> for OwnerSchema`.
            _ => unimplemented!("a new Owner enum variant was added and needs to be handled"),
        }
    }
}

/// `{"owner": <OwnerDto>, "reference": <ObjectRefDto>}`, mirroring the
/// monorepo's `OwnedObjectRef` (which has `#[serde(rename = "OwnedObjectRef")]`
/// but *no* `rename_all`, so the field names stay `owner`/`reference` verbatim).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OwnedObjectRefDto {
    pub owner: OwnerDto,
    pub reference: ObjectRefDto,
}

/// Mirrors the monorepo's `IotaGasCostSummary`: every field is a JSON string
/// (`DisplayFromStr`), camelCase.
#[serde_as]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GasCostSummaryDto {
    #[schemars(with = "String")]
    #[serde_as(as = "DisplayFromStr")]
    pub computation_cost: u64,
    #[schemars(with = "String")]
    #[serde_as(as = "DisplayFromStr")]
    pub computation_cost_burned: u64,
    #[schemars(with = "String")]
    #[serde_as(as = "DisplayFromStr")]
    pub storage_cost: u64,
    #[schemars(with = "String")]
    #[serde_as(as = "DisplayFromStr")]
    pub storage_rebate: u64,
    #[schemars(with = "String")]
    #[serde_as(as = "DisplayFromStr")]
    pub non_refundable_storage_fee: u64,
}

impl From<GasCostSummary> for GasCostSummaryDto {
    fn from(g: GasCostSummary) -> Self {
        Self {
            computation_cost: g.computation_cost,
            computation_cost_burned: g.computation_cost_burned,
            storage_cost: g.storage_cost,
            storage_rebate: g.storage_rebate,
            non_refundable_storage_fee: g.non_refundable_storage_fee,
        }
    }
}

/// Mirrors the monorepo's `IotaExecutionStatus`: internally tagged on
/// `"status"`, camelCase tag values (`"success"` / `"failure"`), with the
/// error rendered as its `Display` string -- *not* the clever-error /
/// Move-package-resolved form (`IotaExecutionStatus::from_native_with_clever_error`),
/// since that requires an online Move package resolver this service does not
/// have; this mirrors the plain `From<ExecutionStatus> for IotaExecutionStatus`
/// impl instead.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename = "ExecutionStatus", rename_all = "camelCase", tag = "status")]
pub enum ExecutionStatusDto {
    Success,
    Failure { error: String },
}

impl ExecutionStatusDto {
    /// Returns `true` if the transaction executed successfully.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }
}

impl From<ExecutionStatus> for ExecutionStatusDto {
    fn from(status: ExecutionStatus) -> Self {
        match status {
            ExecutionStatus::Success => Self::Success,
            ExecutionStatus::Failure {
                error,
                command: None,
            } => Self::Failure {
                error: error.to_string(),
            },
            ExecutionStatus::Failure {
                error,
                command: Some(idx),
            } => Self::Failure {
                error: format!("{error} in command {idx}"),
            },
            // `ExecutionStatus` is `#[non_exhaustive]`; mirrors the monorepo's own
            // defensive arm in `impl From<ExecutionStatus> for IotaExecutionStatus`.
            _ => unimplemented!(
                "a new ExecutionStatus enum variant was added and needs to be handled"
            ),
        }
    }
}

/// Mirrors the monorepo's `IotaTransactionBlockEffectsModifiedAtVersions`.
///
/// Note `sequence_number` here is a JSON **string** (`SequenceNumberString` in
/// the monorepo) -- unlike [`ObjectRefDto::version`], which is a plain number.
#[serde_as]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModifiedAtVersionDto {
    #[schemars(with = "String")]
    pub object_id: ObjectId,
    #[schemars(with = "String")]
    #[serde_as(as = "DisplayFromStr")]
    pub sequence_number: Version,
}

/// JSON-RPC-shaped transaction effects (`IotaTransactionBlockEffectsV1`'s wire
/// shape, flattened -- see the module doc for why this isn't modeled as an
/// enum). `#[serde(rename_all = "camelCase")]` applies to every field below.
#[serde_as]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransactionEffectsDto {
    /// Always `"v1"`: the SDK's `TransactionEffects` only has a `V1` payload today.
    pub message_version: String,
    pub status: ExecutionStatusDto,
    #[schemars(with = "String")]
    #[serde_as(as = "DisplayFromStr")]
    pub executed_epoch: EpochId,
    pub gas_used: GasCostSummaryDto,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_at_versions: Vec<ModifiedAtVersionDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_objects: Vec<ObjectRefDto>,
    #[schemars(with = "String")]
    pub transaction_digest: TransactionDigest,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub created: Vec<OwnedObjectRefDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mutated: Vec<OwnedObjectRefDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unwrapped: Vec<OwnedObjectRefDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<ObjectRefDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unwrapped_then_deleted: Vec<ObjectRefDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wrapped: Vec<ObjectRefDto>,
    pub gas_object: OwnedObjectRefDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub events_digest: Option<TransactionEventsDigest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(with = "Vec<String>")]
    pub dependencies: Vec<TransactionDigest>,
}

// ---------------------------------------------------------------------------
// Derivation from the SDK's compact `TransactionEffectsV1`, ported from
// `crates/iota-types/src/effects/v1.rs` (`impl TransactionEffectsAPI for
// TransactionEffectsV1`) on the `iotaledger/iota` monorepo `develop` branch.
// ---------------------------------------------------------------------------

fn modified_at_versions(v1: &TransactionEffectsV1) -> Vec<(ObjectId, Version)> {
    v1.changed_objects
        .iter()
        .filter_map(|changed| {
            if let ObjectIn::Data { version, .. } = &changed.input_state {
                Some((changed.object_id, *version))
            } else {
                None
            }
        })
        .collect()
}

fn created(v1: &TransactionEffectsV1) -> Vec<(ObjectReference, Owner)> {
    v1.changed_objects
        .iter()
        .filter_map(|changed| {
            match (
                &changed.input_state,
                &changed.output_state,
                &changed.id_operation,
            ) {
                (
                    ObjectIn::Missing,
                    ObjectOut::ObjectWrite { digest, owner },
                    IdOperation::Created,
                ) => Some((
                    ObjectReference::new(changed.object_id, v1.lamport_version, *digest),
                    *owner,
                )),
                // Asymmetry: package versions come from the write itself, never from
                // `lamport_version` (packages don't use the lamport clock).
                (
                    ObjectIn::Missing,
                    ObjectOut::PackageWrite { version, digest },
                    IdOperation::Created,
                ) => Some((
                    ObjectReference::new(changed.object_id, *version, *digest),
                    Owner::Immutable,
                )),
                _ => None,
            }
        })
        .collect()
}

fn mutated(v1: &TransactionEffectsV1) -> Vec<(ObjectReference, Owner)> {
    v1.changed_objects
        .iter()
        .filter_map(
            |changed| match (&changed.input_state, &changed.output_state) {
                (ObjectIn::Data { .. }, ObjectOut::ObjectWrite { digest, owner }) => Some((
                    ObjectReference::new(changed.object_id, v1.lamport_version, *digest),
                    *owner,
                )),
                // Same package-version asymmetry as in `created` above (system package upgrades).
                (ObjectIn::Data { .. }, ObjectOut::PackageWrite { version, digest }) => Some((
                    ObjectReference::new(changed.object_id, *version, *digest),
                    Owner::Immutable,
                )),
                _ => None,
            },
        )
        .collect()
}

fn unwrapped(v1: &TransactionEffectsV1) -> Vec<(ObjectReference, Owner)> {
    v1.changed_objects
        .iter()
        .filter_map(|changed| {
            match (
                &changed.input_state,
                &changed.output_state,
                &changed.id_operation,
            ) {
                (
                    ObjectIn::Missing,
                    ObjectOut::ObjectWrite { digest, owner },
                    IdOperation::None,
                ) => Some((
                    ObjectReference::new(changed.object_id, v1.lamport_version, *digest),
                    *owner,
                )),
                _ => None,
            }
        })
        .collect()
}

fn deleted(v1: &TransactionEffectsV1) -> Vec<ObjectReference> {
    v1.changed_objects
        .iter()
        .filter_map(|changed| {
            match (
                &changed.input_state,
                &changed.output_state,
                &changed.id_operation,
            ) {
                (ObjectIn::Data { .. }, ObjectOut::Missing, IdOperation::Deleted) => {
                    Some(ObjectReference::new(
                        changed.object_id,
                        v1.lamport_version,
                        ObjectDigest::OBJECT_DELETED,
                    ))
                }
                _ => None,
            }
        })
        .collect()
}

fn unwrapped_then_deleted(v1: &TransactionEffectsV1) -> Vec<ObjectReference> {
    v1.changed_objects
        .iter()
        .filter_map(|changed| {
            match (
                &changed.input_state,
                &changed.output_state,
                &changed.id_operation,
            ) {
                (ObjectIn::Missing, ObjectOut::Missing, IdOperation::Deleted) => {
                    Some(ObjectReference::new(
                        changed.object_id,
                        v1.lamport_version,
                        ObjectDigest::OBJECT_DELETED,
                    ))
                }
                _ => None,
            }
        })
        .collect()
}

fn wrapped(v1: &TransactionEffectsV1) -> Vec<ObjectReference> {
    v1.changed_objects
        .iter()
        .filter_map(|changed| {
            match (
                &changed.input_state,
                &changed.output_state,
                &changed.id_operation,
            ) {
                (ObjectIn::Data { .. }, ObjectOut::Missing, IdOperation::None) => {
                    Some(ObjectReference::new(
                        changed.object_id,
                        v1.lamport_version,
                        ObjectDigest::OBJECT_WRAPPED,
                    ))
                }
                _ => None,
            }
        })
        .collect()
}

fn gas_object(v1: &TransactionEffectsV1) -> (ObjectReference, Owner) {
    if let Some(gas_object_index) = v1.gas_object_index {
        let changed = &v1.changed_objects[gas_object_index as usize];
        match &changed.output_state {
            ObjectOut::ObjectWrite { digest, owner } => (
                ObjectReference::new(changed.object_id, v1.lamport_version, *digest),
                *owner,
            ),
            _ => panic!("Gas object must be an ObjectWrite in changed_objects"),
        }
    } else {
        // System transactions that don't require gas.
        (
            ObjectReference::new(ObjectId::ZERO, Version::default(), ObjectDigest::MIN),
            Owner::Address(Address::ZERO),
        )
    }
}

/// Ported from `TransactionEffectsAPI::input_shared_objects` (which returns
/// `Vec<InputSharedObject>`) fused with `InputSharedObject::object_ref()`,
/// since the JSON-RPC DTO only ever needs the flattened `ObjectReference`
/// list (`IotaTransactionBlockEffectsV1::shared_objects`), not the
/// `InputSharedObject` enum itself.
fn shared_object_refs(v1: &TransactionEffectsV1) -> Vec<ObjectReference> {
    v1.changed_objects
        .iter()
        .filter_map(|changed| {
            if let ObjectIn::Data {
                version,
                digest,
                owner: Owner::Shared { .. },
            } = &changed.input_state
            {
                Some(ObjectReference::new(changed.object_id, *version, *digest))
            } else {
                None
            }
        })
        .chain(v1.unchanged_shared_objects.iter().filter_map(|unchanged| {
            match &unchanged.kind {
                UnchangedSharedKind::ReadOnlyRoot { version, digest } => {
                    Some(ObjectReference::new(unchanged.object_id, *version, *digest))
                }
                UnchangedSharedKind::MutateDeleted { version } => Some(ObjectReference::new(
                    unchanged.object_id,
                    *version,
                    ObjectDigest::OBJECT_DELETED,
                )),
                UnchangedSharedKind::ReadDeleted { version } => Some(ObjectReference::new(
                    unchanged.object_id,
                    *version,
                    ObjectDigest::OBJECT_DELETED,
                )),
                UnchangedSharedKind::Cancelled { version } => Some(ObjectReference::new(
                    unchanged.object_id,
                    *version,
                    ObjectDigest::OBJECT_CANCELLED,
                )),
                // Per-epoch config objects don't require sequencing and are excluded from
                // the shared-objects view, matching `TransactionEffectsAPI::input_shared_objects`.
                UnchangedSharedKind::PerEpochConfig => None,
                // `UnchangedSharedKind` is `#[non_exhaustive]`; mirrors the monorepo's own
                // defensive arm in `TransactionEffectsV1::input_shared_objects`.
                _ => unimplemented!(
                    "a new UnchangedSharedKind enum variant was added and needs to be handled"
                ),
            }
        }))
        .collect()
}

fn to_owned_refs(refs: Vec<(ObjectReference, Owner)>) -> Vec<OwnedObjectRefDto> {
    refs.into_iter()
        .map(|(reference, owner)| OwnedObjectRefDto {
            owner: owner.into(),
            reference: reference.into(),
        })
        .collect()
}

impl From<&TransactionEffectsV1> for TransactionEffectsDto {
    fn from(v1: &TransactionEffectsV1) -> Self {
        let (gas_reference, gas_owner) = gas_object(v1);
        Self {
            message_version: "v1".to_string(),
            status: v1.status.clone().into(),
            executed_epoch: v1.epoch,
            gas_used: v1.gas_cost_summary.clone().into(),
            modified_at_versions: modified_at_versions(v1)
                .into_iter()
                .map(|(object_id, sequence_number)| ModifiedAtVersionDto {
                    object_id,
                    sequence_number,
                })
                .collect(),
            shared_objects: shared_object_refs(v1).into_iter().map(Into::into).collect(),
            transaction_digest: v1.transaction_digest,
            created: to_owned_refs(created(v1)),
            mutated: to_owned_refs(mutated(v1)),
            unwrapped: to_owned_refs(unwrapped(v1)),
            deleted: deleted(v1).into_iter().map(Into::into).collect(),
            unwrapped_then_deleted: unwrapped_then_deleted(v1)
                .into_iter()
                .map(Into::into)
                .collect(),
            wrapped: wrapped(v1).into_iter().map(Into::into).collect(),
            gas_object: OwnedObjectRefDto {
                owner: gas_owner.into(),
                reference: gas_reference.into(),
            },
            events_digest: v1.events_digest,
            dependencies: v1.dependencies.clone(),
        }
    }
}

impl From<TransactionEffectsV1> for TransactionEffectsDto {
    fn from(v1: TransactionEffectsV1) -> Self {
        Self::from(&v1)
    }
}

impl From<TransactionEffects> for TransactionEffectsDto {
    fn from(effects: TransactionEffects) -> Self {
        // `TransactionEffects` only has a `V1` payload today; `into_v1()` is an
        // inherent method on the SDK's own (non_exhaustive-to-us, but not to
        // itself) enum, so no wildcard arm is needed here.
        effects.into_v1().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iota_sdk_types::{ChangedObject, ExecutionError, UnchangedSharedObject};
    use serde_json::json;

    fn oid(byte: u8) -> ObjectId {
        ObjectId::new([byte; 32])
    }

    fn addr(byte: u8) -> Address {
        Address::new([byte; 32])
    }

    fn odig(byte: u8) -> ObjectDigest {
        ObjectDigest::new([byte; 32])
    }

    fn txdig(byte: u8) -> TransactionDigest {
        TransactionDigest::new([byte; 32])
    }

    fn base_gas_cost_summary() -> GasCostSummary {
        GasCostSummary::new(100, 10, 50, 20, 5)
    }

    fn expected_gas_used_json() -> serde_json::Value {
        json!({
            "computationCost": "100",
            "computationCostBurned": "10",
            "storageCost": "50",
            "storageRebate": "20",
            "nonRefundableStorageFee": "5",
        })
    }

    /// (a) Simple success with exactly one mutated (gas) object.
    #[test]
    fn simple_success_mutated_gas_object() {
        let gas_owner_addr = addr(0xA0);
        let v1 = TransactionEffectsV1 {
            status: ExecutionStatus::Success,
            epoch: 42,
            gas_cost_summary: base_gas_cost_summary(),
            transaction_digest: txdig(1),
            gas_object_index: Some(0),
            events_digest: None,
            dependencies: vec![],
            lamport_version: Version::from_u64(6),
            changed_objects: vec![ChangedObject {
                object_id: oid(2),
                input_state: ObjectIn::Data {
                    version: Version::from_u64(5),
                    digest: odig(3),
                    owner: Owner::Address(gas_owner_addr),
                },
                output_state: ObjectOut::ObjectWrite {
                    digest: odig(4),
                    owner: Owner::Address(gas_owner_addr),
                },
                id_operation: IdOperation::None,
            }],
            unchanged_shared_objects: vec![],
            auxiliary_data_digest: None,
        };

        let dto = TransactionEffectsDto::from(&v1);
        let actual = serde_json::to_value(&dto).unwrap();

        let expected = json!({
            "messageVersion": "v1",
            "status": { "status": "success" },
            "executedEpoch": "42",
            "gasUsed": expected_gas_used_json(),
            "modifiedAtVersions": [
                { "objectId": oid(2).to_string(), "sequenceNumber": "5" },
            ],
            "transactionDigest": txdig(1).to_string(),
            "mutated": [
                {
                    "owner": { "AddressOwner": gas_owner_addr.to_string() },
                    "reference": {
                        "objectId": oid(2).to_string(),
                        "version": 6,
                        "digest": odig(4).to_string(),
                    },
                },
            ],
            "gasObject": {
                "owner": { "AddressOwner": gas_owner_addr.to_string() },
                "reference": {
                    "objectId": oid(2).to_string(),
                    "version": 6,
                    "digest": odig(4).to_string(),
                },
            },
        });

        assert_eq!(actual, expected);
    }

    /// (b) Created + mutated + deleted objects together, plus a
    /// `modifiedAtVersions` entry and a non-empty `dependencies` list.
    #[test]
    fn created_mutated_deleted_together() {
        let owner_a = addr(0xA1);
        let owner_b = addr(0xA2);

        let created_obj = ChangedObject {
            object_id: oid(10),
            input_state: ObjectIn::Missing,
            output_state: ObjectOut::ObjectWrite {
                digest: odig(11),
                owner: Owner::Address(owner_a),
            },
            id_operation: IdOperation::Created,
        };
        let mutated_obj = ChangedObject {
            object_id: oid(20),
            input_state: ObjectIn::Data {
                version: Version::from_u64(3),
                digest: odig(21),
                owner: Owner::Address(owner_b),
            },
            output_state: ObjectOut::ObjectWrite {
                digest: odig(22),
                owner: Owner::Address(owner_b),
            },
            id_operation: IdOperation::None,
        };
        let deleted_obj = ChangedObject {
            object_id: oid(30),
            input_state: ObjectIn::Data {
                version: Version::from_u64(7),
                digest: odig(31),
                owner: Owner::Address(owner_a),
            },
            output_state: ObjectOut::Missing,
            id_operation: IdOperation::Deleted,
        };
        // Gas object, mutated, at index 1 (matches `changed_objects` ordering below).
        let gas_obj = ChangedObject {
            object_id: oid(40),
            input_state: ObjectIn::Data {
                version: Version::from_u64(1),
                digest: odig(41),
                owner: Owner::Address(owner_a),
            },
            output_state: ObjectOut::ObjectWrite {
                digest: odig(42),
                owner: Owner::Address(owner_a),
            },
            id_operation: IdOperation::None,
        };

        let v1 = TransactionEffectsV1 {
            status: ExecutionStatus::Success,
            epoch: 7,
            gas_cost_summary: base_gas_cost_summary(),
            transaction_digest: txdig(50),
            gas_object_index: Some(3),
            events_digest: Some(TransactionEventsDigest::new([60; 32])),
            dependencies: vec![txdig(70), txdig(71)],
            lamport_version: Version::from_u64(8),
            changed_objects: vec![created_obj, mutated_obj, deleted_obj, gas_obj],
            unchanged_shared_objects: vec![],
            auxiliary_data_digest: None,
        };

        let dto = TransactionEffectsDto::from(&v1);
        let actual = serde_json::to_value(&dto).unwrap();

        let expected = json!({
            "messageVersion": "v1",
            "status": { "status": "success" },
            "executedEpoch": "7",
            "gasUsed": expected_gas_used_json(),
            "modifiedAtVersions": [
                { "objectId": oid(20).to_string(), "sequenceNumber": "3" },
                { "objectId": oid(30).to_string(), "sequenceNumber": "7" },
                { "objectId": oid(40).to_string(), "sequenceNumber": "1" },
            ],
            "transactionDigest": txdig(50).to_string(),
            "created": [
                {
                    "owner": { "AddressOwner": owner_a.to_string() },
                    "reference": { "objectId": oid(10).to_string(), "version": 8, "digest": odig(11).to_string() },
                },
            ],
            "mutated": [
                {
                    "owner": { "AddressOwner": owner_b.to_string() },
                    "reference": { "objectId": oid(20).to_string(), "version": 8, "digest": odig(22).to_string() },
                },
                {
                    "owner": { "AddressOwner": owner_a.to_string() },
                    "reference": { "objectId": oid(40).to_string(), "version": 8, "digest": odig(42).to_string() },
                },
            ],
            "deleted": [
                { "objectId": oid(30).to_string(), "version": 8, "digest": ObjectDigest::OBJECT_DELETED.to_string() },
            ],
            "gasObject": {
                "owner": { "AddressOwner": owner_a.to_string() },
                "reference": { "objectId": oid(40).to_string(), "version": 8, "digest": odig(42).to_string() },
            },
            "eventsDigest": TransactionEventsDigest::new([60; 32]).to_string(),
            "dependencies": [txdig(70).to_string(), txdig(71).to_string()],
        });

        assert_eq!(actual, expected);
    }

    /// (c) A shared object appearing (read-only) in `unchanged_shared_objects`.
    #[test]
    fn shared_object_in_unchanged_shared_objects() {
        let gas_owner_addr = addr(0xB0);
        let shared_id = oid(80);
        let shared_version = Version::from_u64(15);
        let shared_digest = odig(81);

        let v1 = TransactionEffectsV1 {
            status: ExecutionStatus::Success,
            epoch: 1,
            gas_cost_summary: base_gas_cost_summary(),
            transaction_digest: txdig(90),
            gas_object_index: Some(0),
            events_digest: None,
            dependencies: vec![],
            lamport_version: Version::from_u64(2),
            changed_objects: vec![ChangedObject {
                object_id: oid(91),
                input_state: ObjectIn::Data {
                    version: Version::from_u64(1),
                    digest: odig(92),
                    owner: Owner::Address(gas_owner_addr),
                },
                output_state: ObjectOut::ObjectWrite {
                    digest: odig(93),
                    owner: Owner::Address(gas_owner_addr),
                },
                id_operation: IdOperation::None,
            }],
            unchanged_shared_objects: vec![UnchangedSharedObject {
                object_id: shared_id,
                kind: UnchangedSharedKind::ReadOnlyRoot {
                    version: shared_version,
                    digest: shared_digest,
                },
            }],
            auxiliary_data_digest: None,
        };

        let dto = TransactionEffectsDto::from(&v1);
        let actual = serde_json::to_value(&dto).unwrap();

        assert_eq!(
            actual["sharedObjects"],
            json!([
                {
                    "objectId": shared_id.to_string(),
                    "version": 15,
                    "digest": shared_digest.to_string(),
                },
            ])
        );
        // Sanity: the shared object must not leak into any of the other object lists.
        assert_eq!(actual.get("created"), None);
        assert_eq!(actual.get("deleted"), None);
    }

    /// (e) `PackageWrite`: the created package's `ObjectRefDto.version` must come
    /// from `ObjectOut::PackageWrite`'s own `version` field, *not* `lamport_version`
    /// -- this is the asymmetry called out explicitly in this stage's task.
    #[test]
    fn package_write_uses_its_own_version_not_lamport_version() {
        let package_id = oid(100);
        let package_version = Version::from_u64(3); // deliberately != lamport_version
        let package_digest = odig(101);

        let v1 = TransactionEffectsV1 {
            status: ExecutionStatus::Success,
            epoch: 1,
            gas_cost_summary: base_gas_cost_summary(),
            transaction_digest: txdig(110),
            gas_object_index: None,
            events_digest: None,
            dependencies: vec![],
            lamport_version: Version::from_u64(999), // must NOT show up on the package ref
            changed_objects: vec![ChangedObject {
                object_id: package_id,
                input_state: ObjectIn::Missing,
                output_state: ObjectOut::PackageWrite {
                    version: package_version,
                    digest: package_digest,
                },
                id_operation: IdOperation::Created,
            }],
            unchanged_shared_objects: vec![],
            auxiliary_data_digest: None,
        };

        let dto = TransactionEffectsDto::from(&v1);
        let actual = serde_json::to_value(&dto).unwrap();

        assert_eq!(
            actual["created"],
            json!([
                {
                    "owner": "Immutable",
                    "reference": {
                        "objectId": package_id.to_string(),
                        "version": 3,
                        "digest": package_digest.to_string(),
                    },
                },
            ])
        );
    }

    /// (d) Failure status golden-JSON coverage for the four `ExecutionError`
    /// variants that were permuted at the *old* (now-superseded) iota-sdk-types
    /// revision this codebase used to pin -- proving these round-trip correctly
    /// through `ExecutionStatusDto` is the most important regression check here.
    fn failure_effects_with(error: ExecutionError) -> TransactionEffectsV1 {
        TransactionEffectsV1 {
            status: ExecutionStatus::new_failure(error, None),
            epoch: 3,
            gas_cost_summary: base_gas_cost_summary(),
            transaction_digest: txdig(200),
            gas_object_index: Some(0),
            events_digest: None,
            dependencies: vec![],
            lamport_version: Version::from_u64(2),
            changed_objects: vec![ChangedObject {
                object_id: oid(201),
                input_state: ObjectIn::Data {
                    version: Version::from_u64(1),
                    digest: odig(202),
                    owner: Owner::Address(addr(0xC0)),
                },
                output_state: ObjectOut::ObjectWrite {
                    digest: odig(203),
                    owner: Owner::Address(addr(0xC0)),
                },
                id_operation: IdOperation::None,
            }],
            unchanged_shared_objects: vec![],
            auxiliary_data_digest: None,
        }
    }

    #[test]
    fn failure_status_address_denied_for_coin() {
        let error = ExecutionError::AddressDeniedForCoin {
            address: addr(0xD0),
            coin_type: "0x2::iota::IOTA".to_string(),
        };
        let expected_message = error.to_string();
        let v1 = failure_effects_with(error);

        let dto = TransactionEffectsDto::from(&v1);
        let actual = serde_json::to_value(&dto).unwrap();
        assert_eq!(
            actual["status"],
            json!({ "status": "failure", "error": expected_message })
        );
    }

    #[test]
    fn failure_status_coin_type_global_pause() {
        let error = ExecutionError::CoinTypeGlobalPause {
            coin_type: "0x2::iota::IOTA".to_string(),
        };
        let expected_message = error.to_string();
        assert_eq!(
            expected_message,
            "Coin type is globally paused for use: 0x2::iota::IOTA"
        );
        let v1 = failure_effects_with(error);

        let dto = TransactionEffectsDto::from(&v1);
        let actual = serde_json::to_value(&dto).unwrap();
        assert_eq!(
            actual["status"],
            json!({ "status": "failure", "error": expected_message })
        );
    }

    #[test]
    fn failure_status_randomness_unavailable() {
        let error = ExecutionError::ExecutionCancelledDueToRandomnessUnavailable;
        let expected_message = error.to_string();
        assert_eq!(
            expected_message,
            "Certificate is cancelled because randomness could not be generated this epoch"
        );
        let v1 = failure_effects_with(error);

        let dto = TransactionEffectsDto::from(&v1);
        let actual = serde_json::to_value(&dto).unwrap();
        assert_eq!(
            actual["status"],
            json!({ "status": "failure", "error": expected_message })
        );
    }

    #[test]
    fn failure_status_congestion_v2_with_command_index() {
        let error = ExecutionError::ExecutionCancelledDueToSharedObjectCongestionV2 {
            congested_objects: vec![oid(210), oid(211)],
            suggested_gas_price: 12_345,
        };
        // Exercise the `command: Some(idx)` branch too (`"{error} in command {idx}"`).
        let mut v1 = failure_effects_with(error.clone());
        v1.status = ExecutionStatus::new_failure(error.clone(), Some(2));
        let expected_message = format!("{error} in command 2");

        let dto = TransactionEffectsDto::from(&v1);
        let actual = serde_json::to_value(&dto).unwrap();
        assert_eq!(
            actual["status"],
            json!({ "status": "failure", "error": expected_message })
        );
    }
}
