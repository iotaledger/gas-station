// Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! Small helpers for deriving specific pieces of information out of a raw
//! `iota_sdk_types::TransactionEffects` (which only exposes the compact
//! `changed_objects` representation).
//!
//! `crate::rpc::effects` contains the same derivation logic for its
//! JSON-RPC-shaped DTO, but its helpers are private to that module. The two
//! helpers below are the minimal subset `gas_station_core.rs` and
//! `gas_station_initializer.rs` need: the gas coin's post-execution reference
//! and the set of newly created objects.

use iota_sdk_types::{IdOperation, ObjectIn, ObjectOut, ObjectReference, TransactionEffects};

/// The transaction's own gas coin, as it exists after execution.
///
/// Panics if `gas_object_index` is unset (a system transaction that doesn't
/// pay gas) or points at an entry that isn't a plain `ObjectWrite` -- neither
/// can happen for the signed, gas-paying, programmable transactions this
/// crate ever executes.
pub(crate) fn gas_object_reference(effects: &TransactionEffects) -> ObjectReference {
    let v1 = effects.as_v1();
    let gas_object_index = v1
        .gas_object_index
        .expect("a signed, gas-paying transaction always records a gas object") as usize;
    let changed = &v1.changed_objects[gas_object_index];
    match &changed.output_state {
        ObjectOut::ObjectWrite { digest, .. } => {
            ObjectReference::new(changed.object_id, v1.lamport_version, *digest)
        }
        _ => panic!("gas object must be an ObjectWrite in changed_objects"),
    }
}

/// Object references for every object created by the transaction.
///
/// `PackageWrite` entries are deliberately not handled: the only caller
/// (`gas_station_initializer.rs`'s coin-splitting) only ever creates new
/// `Coin<IOTA>` objects, never packages.
pub(crate) fn created_object_refs(effects: &TransactionEffects) -> Vec<ObjectReference> {
    let v1 = effects.as_v1();
    v1.changed_objects
        .iter()
        .filter_map(|changed| {
            match (
                &changed.input_state,
                &changed.output_state,
                &changed.id_operation,
            ) {
                (ObjectIn::Missing, ObjectOut::ObjectWrite { digest, .. }, IdOperation::Created) => {
                    Some(ObjectReference::new(
                        changed.object_id,
                        v1.lamport_version,
                        *digest,
                    ))
                }
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iota_sdk_types::{
        Address, ChangedObject, ExecutionStatus, GasCostSummary, ObjectDigest, ObjectId, Owner,
        TransactionDigest, TransactionEffectsV1, Version,
    };

    fn oid(byte: u8) -> ObjectId {
        ObjectId::new([byte; 32])
    }

    fn addr(byte: u8) -> Address {
        Address::new([byte; 32])
    }

    fn odig(byte: u8) -> ObjectDigest {
        ObjectDigest::new([byte; 32])
    }

    fn base_effects(changed_objects: Vec<ChangedObject>, gas_object_index: Option<u32>) -> TransactionEffects {
        TransactionEffects::V1(Box::new(TransactionEffectsV1 {
            status: ExecutionStatus::Success,
            epoch: 1,
            gas_cost_summary: GasCostSummary::new(100, 10, 50, 20, 5),
            transaction_digest: TransactionDigest::new([1; 32]),
            gas_object_index,
            events_digest: None,
            dependencies: vec![],
            lamport_version: Version::from_u64(6),
            changed_objects,
            unchanged_shared_objects: vec![],
            auxiliary_data_digest: None,
        }))
    }

    #[test]
    fn gas_object_reference_reads_the_indexed_changed_object() {
        let owner = addr(0xA0);
        let effects = base_effects(
            vec![
                // Not the gas object -- proves the index, not just "the only
                // entry", drives the result.
                ChangedObject {
                    object_id: oid(1),
                    input_state: ObjectIn::Missing,
                    output_state: ObjectOut::ObjectWrite {
                        digest: odig(2),
                        owner: Owner::Address(owner),
                    },
                    id_operation: IdOperation::Created,
                },
                ChangedObject {
                    object_id: oid(3),
                    input_state: ObjectIn::Data {
                        version: Version::from_u64(5),
                        digest: odig(4),
                        owner: Owner::Address(owner),
                    },
                    output_state: ObjectOut::ObjectWrite {
                        digest: odig(5),
                        owner: Owner::Address(owner),
                    },
                    id_operation: IdOperation::None,
                },
            ],
            Some(1),
        );

        let reference = gas_object_reference(&effects);
        assert_eq!(reference.object_id, oid(3));
        assert_eq!(reference.version, Version::from_u64(6));
        assert_eq!(reference.digest, odig(5));
    }

    #[test]
    #[should_panic(expected = "always records a gas object")]
    fn gas_object_reference_panics_without_a_gas_object_index() {
        let effects = base_effects(vec![], None);
        gas_object_reference(&effects);
    }

    #[test]
    fn created_object_refs_only_includes_missing_to_object_write_created_entries() {
        let owner = addr(0xB0);
        let created = ChangedObject {
            object_id: oid(10),
            input_state: ObjectIn::Missing,
            output_state: ObjectOut::ObjectWrite {
                digest: odig(11),
                owner: Owner::Address(owner),
            },
            id_operation: IdOperation::Created,
        };
        // Mutated, not created -- must be excluded.
        let mutated = ChangedObject {
            object_id: oid(20),
            input_state: ObjectIn::Data {
                version: Version::from_u64(1),
                digest: odig(21),
                owner: Owner::Address(owner),
            },
            output_state: ObjectOut::ObjectWrite {
                digest: odig(22),
                owner: Owner::Address(owner),
            },
            id_operation: IdOperation::None,
        };
        // Unwrapped (Missing -> ObjectWrite, but id_operation == None, not
        // Created) -- must also be excluded.
        let unwrapped = ChangedObject {
            object_id: oid(30),
            input_state: ObjectIn::Missing,
            output_state: ObjectOut::ObjectWrite {
                digest: odig(31),
                owner: Owner::Address(owner),
            },
            id_operation: IdOperation::None,
        };
        let effects = base_effects(vec![created, mutated, unwrapped], Some(1));

        let refs = created_object_refs(&effects);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].object_id, oid(10));
        assert_eq!(refs[0].version, Version::from_u64(6));
        assert_eq!(refs[0].digest, odig(11));
    }

    #[test]
    fn created_object_refs_empty_when_nothing_created() {
        let effects = base_effects(vec![], Some(0));
        assert!(created_object_refs(&effects).is_empty());
    }
}
