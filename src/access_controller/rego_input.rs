// Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! Compatibility shim reproducing the legacy JSON shape of
//! `iota_types::transaction::TransactionData` for `input.transaction_data` in
//! Rego policies (see `docs/access-controller.md`).
//!
//! Rego policies are configuration deployed by customers, not code reviewed
//! in this repository, so the JSON shape of `input.transaction_data` is a
//! wire contract that must not silently change. This module defines
//! Serialize-only "shadow" types mirroring the legacy JSON shape and converts
//! a `&iota_sdk_types::Transaction` into them, instead of directly serializing
//! the SDK type (whose native JSON shape diverges from the legacy one in
//! several ways -- e.g. `Pure` inputs as base64 strings instead of byte
//! arrays, renamed variants, and tuple-vs-struct command encodings; the
//! golden tests below pin the required shape against
//! `predicates/test_files/transaction_kind.json`).
//!
//! `TypeTag`s are rendered as `Display` strings, and system-transaction
//! `TransactionKind` variants (which can't arrive through the public API)
//! preserve only the legacy variant *name*, with their payload serialized in
//! the SDK type's native shape.

use iota_sdk_types::{
    Address, Argument, Command, ConsensusCommitPrologueV1, GasPayment, GenesisTransaction, Input,
    MakeMoveVector, MergeCoins, MoveCall, ObjectDigest, ObjectId, ObjectReference, Publish,
    RandomnessStateUpdate, SplitCoins, Transaction, TransactionExpiration, TransactionKind,
    TransactionV1, TransferObjects, TypeTag, Upgrade,
};
use serde::Serialize;
use serde_json::Value;

/// Converts `transaction` into the legacy
/// `iota_types::transaction::TransactionData` JSON shape, for use as
/// `input.transaction_data` in Rego policies.
pub(crate) fn to_legacy_json(transaction: &Transaction) -> Value {
    serde_json::to_value(ShadowTransactionData::from(transaction))
        .expect("shadow transaction-data shapes are always representable as JSON")
}

/// `(package, module, function)` for every `Command::MoveCall` in a
/// programmable transaction (empty for any other transaction kind).
pub(crate) fn move_calls(transaction: &Transaction) -> Vec<(ObjectId, &str, &str)> {
    let Transaction::V1(v1) = transaction else {
        return vec![];
    };
    let TransactionKind::Programmable(pt) = &v1.kind else {
        return vec![];
    };
    pt.commands
        .iter()
        .filter_map(|command| match command {
            Command::MoveCall(mc) => Some((mc.package, mc.module.as_str(), mc.function.as_str())),
            _ => None,
        })
        .collect()
}

/// Mirrors `iota_types::transaction::TransactionData`.
#[derive(Serialize)]
enum ShadowTransactionData {
    V1(ShadowTransactionDataV1),
    /// `Transaction` is `#[non_exhaustive]` in `iota_sdk_types`; unreachable
    /// at the pinned SDK rev (which defines only `V1`), this arm only ensures
    /// a future SDK bump degrades instead of panicking on
    /// attacker-controlled transaction bytes.
    Unrecognized(String),
}

impl From<&Transaction> for ShadowTransactionData {
    fn from(transaction: &Transaction) -> Self {
        match transaction {
            Transaction::V1(v1) => ShadowTransactionData::V1(v1.into()),
            other => ShadowTransactionData::Unrecognized(format!("{other:?}")),
        }
    }
}

/// Mirrors `iota_types::transaction::TransactionDataV1`.
#[derive(Serialize)]
struct ShadowTransactionDataV1 {
    kind: ShadowTransactionKind,
    sender: Address,
    gas_data: ShadowGasData,
    expiration: ShadowTransactionExpiration,
}

impl From<&TransactionV1> for ShadowTransactionDataV1 {
    fn from(v1: &TransactionV1) -> Self {
        Self {
            kind: (&v1.kind).into(),
            sender: v1.sender,
            gas_data: (&v1.gas_payment).into(),
            expiration: (&v1.expiration).into(),
        }
    }
}

/// Mirrors `iota_types::transaction::TransactionKind`.
#[derive(Serialize)]
enum ShadowTransactionKind {
    ProgrammableTransaction(ShadowProgrammableTransaction),
    // System-transaction kinds: never submitted by a user through the gas
    // station's public API. Tag preserved for legacy-name compatibility;
    // nested payload uses the SDK type's own `Serialize` impl (see module docs).
    Genesis(GenesisTransaction),
    ConsensusCommitPrologueV1(ConsensusCommitPrologueV1),
    AuthenticatorStateUpdateV1(()),
    EndOfEpochTransaction(Vec<iota_sdk_types::EndOfEpochTransactionKind>),
    RandomnessStateUpdate(RandomnessStateUpdate),
    /// See `ShadowTransactionData::Unrecognized`.
    Unrecognized(String),
}

impl From<&TransactionKind> for ShadowTransactionKind {
    fn from(kind: &TransactionKind) -> Self {
        match kind {
            TransactionKind::Programmable(pt) => {
                ShadowTransactionKind::ProgrammableTransaction(pt.into())
            }
            TransactionKind::Genesis(genesis) => ShadowTransactionKind::Genesis(genesis.clone()),
            TransactionKind::ConsensusCommitPrologueV1(prologue) => {
                ShadowTransactionKind::ConsensusCommitPrologueV1(prologue.clone())
            }
            TransactionKind::AuthenticatorStateUpdateV1Deprecated => {
                ShadowTransactionKind::AuthenticatorStateUpdateV1(())
            }
            TransactionKind::EndOfEpoch(kinds) => {
                ShadowTransactionKind::EndOfEpochTransaction(kinds.clone())
            }
            TransactionKind::RandomnessStateUpdate(update) => {
                ShadowTransactionKind::RandomnessStateUpdate(update.clone())
            }
            other => ShadowTransactionKind::Unrecognized(format!("{other:?}")),
        }
    }
}

/// Mirrors `iota_types::transaction::ProgrammableTransaction`.
#[derive(Serialize)]
struct ShadowProgrammableTransaction {
    inputs: Vec<ShadowCallArg>,
    commands: Vec<ShadowCommand>,
}

impl From<&iota_sdk_types::ProgrammableTransaction> for ShadowProgrammableTransaction {
    fn from(pt: &iota_sdk_types::ProgrammableTransaction) -> Self {
        Self {
            inputs: pt.inputs.iter().map(Into::into).collect(),
            commands: pt.commands.iter().map(Into::into).collect(),
        }
    }
}

/// Mirrors `iota_types::transaction::CallArg`.
#[derive(Serialize)]
enum ShadowCallArg {
    Pure(Vec<u8>),
    Object(ShadowObjectArg),
    /// See `ShadowTransactionData::Unrecognized`.
    Unrecognized(String),
}

impl From<&Input> for ShadowCallArg {
    fn from(input: &Input) -> Self {
        match input {
            Input::Pure(bytes) => ShadowCallArg::Pure(bytes.clone()),
            Input::ImmutableOrOwned(object_ref) => {
                ShadowCallArg::Object(ShadowObjectArg::ImmOrOwnedObject(object_ref.into()))
            }
            Input::Shared(shared) => ShadowCallArg::Object(ShadowObjectArg::SharedObject {
                id: shared.object_id,
                initial_shared_version: shared.initial_shared_version.as_u64(),
                mutable: shared.mutable,
            }),
            Input::Receiving(object_ref) => {
                ShadowCallArg::Object(ShadowObjectArg::Receiving(object_ref.into()))
            }
            other => ShadowCallArg::Unrecognized(format!("{other:?}")),
        }
    }
}

/// Mirrors `iota_types::transaction::ObjectArg`.
#[derive(Serialize)]
enum ShadowObjectArg {
    ImmOrOwnedObject(ShadowObjectRef),
    SharedObject {
        id: ObjectId,
        initial_shared_version: u64,
        mutable: bool,
    },
    Receiving(ShadowObjectRef),
}

/// Mirrors legacy `type ObjectRef = (ObjectID, SequenceNumber, ObjectDigest)`:
/// a 3-element tuple with the version as a plain JSON number.
#[derive(Serialize)]
struct ShadowObjectRef(ObjectId, u64, ObjectDigest);

impl From<&ObjectReference> for ShadowObjectRef {
    fn from(object_ref: &ObjectReference) -> Self {
        Self(
            object_ref.object_id,
            object_ref.version.as_u64(),
            object_ref.digest,
        )
    }
}

/// Mirrors `iota_types::transaction::Command`.
#[derive(Serialize)]
enum ShadowCommand {
    MoveCall(ShadowMoveCall),
    TransferObjects(Vec<ShadowArgument>, ShadowArgument),
    SplitCoins(ShadowArgument, Vec<ShadowArgument>),
    MergeCoins(ShadowArgument, Vec<ShadowArgument>),
    Publish(Vec<Vec<u8>>, Vec<ObjectId>),
    MakeMoveVec(Option<String>, Vec<ShadowArgument>),
    Upgrade(Vec<Vec<u8>>, Vec<ObjectId>, ObjectId, ShadowArgument),
    /// See `ShadowTransactionData::Unrecognized`.
    Unrecognized(String),
}

impl From<&Command> for ShadowCommand {
    fn from(command: &Command) -> Self {
        match command {
            Command::MoveCall(move_call) => ShadowCommand::MoveCall(move_call.into()),
            Command::TransferObjects(TransferObjects { objects, address }) => {
                ShadowCommand::TransferObjects(
                    objects.iter().map(Into::into).collect(),
                    address.into(),
                )
            }
            Command::SplitCoins(SplitCoins { coin, amounts }) => ShadowCommand::SplitCoins(
                coin.into(),
                amounts.iter().map(Into::into).collect(),
            ),
            Command::MergeCoins(MergeCoins {
                coin,
                coins_to_merge,
            }) => ShadowCommand::MergeCoins(
                coin.into(),
                coins_to_merge.iter().map(Into::into).collect(),
            ),
            Command::Publish(Publish {
                modules,
                dependencies,
            }) => ShadowCommand::Publish(modules.clone(), dependencies.clone()),
            Command::MakeMoveVector(MakeMoveVector { type_, elements }) => {
                ShadowCommand::MakeMoveVec(
                    type_.as_ref().map(TypeTag::to_string),
                    elements.iter().map(Into::into).collect(),
                )
            }
            Command::Upgrade(Upgrade {
                modules,
                dependencies,
                package,
                ticket,
            }) => ShadowCommand::Upgrade(
                modules.clone(),
                dependencies.clone(),
                *package,
                ticket.into(),
            ),
            other => ShadowCommand::Unrecognized(format!("{other:?}")),
        }
    }
}

/// Mirrors `iota_types::transaction::ProgrammableMoveCall`.
#[derive(Serialize)]
struct ShadowMoveCall {
    package: ObjectId,
    module: String,
    function: String,
    type_arguments: Vec<String>,
    arguments: Vec<ShadowArgument>,
}

impl From<&MoveCall> for ShadowMoveCall {
    fn from(move_call: &MoveCall) -> Self {
        Self {
            package: move_call.package,
            module: move_call.module.as_str().to_string(),
            function: move_call.function.as_str().to_string(),
            type_arguments: move_call
                .type_arguments
                .iter()
                .map(TypeTag::to_string)
                .collect(),
            arguments: move_call.arguments.iter().map(Into::into).collect(),
        }
    }
}

/// Mirrors `iota_types::transaction::Argument`.
#[derive(Serialize)]
enum ShadowArgument {
    GasCoin,
    Input(u16),
    Result(u16),
    NestedResult(u16, u16),
    /// See `ShadowTransactionData::Unrecognized`.
    Unrecognized(String),
}

impl From<&Argument> for ShadowArgument {
    fn from(argument: &Argument) -> Self {
        match argument {
            Argument::Gas => ShadowArgument::GasCoin,
            Argument::Input(i) => ShadowArgument::Input(*i),
            Argument::Result(i) => ShadowArgument::Result(*i),
            Argument::NestedResult(i, j) => ShadowArgument::NestedResult(*i, *j),
            other => ShadowArgument::Unrecognized(format!("{other:?}")),
        }
    }
}

/// Mirrors `iota_types::transaction::GasData`.
#[derive(Serialize)]
struct ShadowGasData {
    payment: Vec<ShadowObjectRef>,
    owner: Address,
    price: u64,
    budget: u64,
}

impl From<&GasPayment> for ShadowGasData {
    fn from(gas_payment: &GasPayment) -> Self {
        Self {
            payment: gas_payment.objects.iter().map(Into::into).collect(),
            owner: gas_payment.owner,
            price: gas_payment.price,
            budget: gas_payment.budget,
        }
    }
}

/// Mirrors `iota_types::transaction::TransactionExpiration`.
#[derive(Serialize)]
enum ShadowTransactionExpiration {
    None,
    Epoch(u64),
    /// See `ShadowTransactionData::Unrecognized`.
    Unrecognized(String),
}

impl From<&TransactionExpiration> for ShadowTransactionExpiration {
    fn from(expiration: &TransactionExpiration) -> Self {
        match expiration {
            TransactionExpiration::None => ShadowTransactionExpiration::None,
            TransactionExpiration::Epoch(epoch) => ShadowTransactionExpiration::Epoch(*epoch),
            other => ShadowTransactionExpiration::Unrecognized(format!("{other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iota_sdk_types::{Identifier, ProgrammableTransaction, TransactionV1};
    use std::str::FromStr;

    const TRANSACTION_KIND_JSON: &str = include_str!("predicates/test_files/transaction_kind.json");

    fn wrap_in_transaction(kind: TransactionKind) -> Transaction {
        Transaction::V1(TransactionV1 {
            kind,
            sender: Address::ZERO,
            gas_payment: GasPayment {
                objects: vec![],
                owner: Address::ZERO,
                price: 0,
                budget: 0,
            },
            expiration: TransactionExpiration::None,
        })
    }

    /// Rebuilds the exact `ProgrammableTransaction` described by the
    /// pre-migration fixture (14 `Pure` inputs, 1 `MoveCall` command
    /// referencing all 14 as `Input(i)` arguments) straight from the
    /// fixture's own JSON, then asserts the shadow serializer's output for
    /// it is semantically identical to the fixture -- i.e. a genuine
    /// round-trip check, not just a hand-copied expectation.
    #[test]
    fn matches_pre_migration_json_shape_for_fixture_transaction() {
        let fixture: Value = serde_json::from_str(TRANSACTION_KIND_JSON).unwrap();
        let pt_json = fixture
            .get("ProgrammableTransaction")
            .expect("fixture is a ProgrammableTransaction kind");

        let inputs: Vec<Input> = pt_json["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| {
                let bytes: Vec<u8> = input["Pure"]
                    .as_array()
                    .expect("fixture inputs are all Pure byte arrays")
                    .iter()
                    .map(|b| b.as_u64().unwrap() as u8)
                    .collect();
                Input::Pure(bytes)
            })
            .collect();

        let move_call_json = &pt_json["commands"][0]["MoveCall"];
        let package =
            Address::from_str(move_call_json["package"].as_str().unwrap()).unwrap();
        let arguments: Vec<Argument> = (0..move_call_json["arguments"].as_array().unwrap().len()
            as u16)
            .map(Argument::Input)
            .collect();

        let tx = wrap_in_transaction(TransactionKind::Programmable(ProgrammableTransaction {
            inputs,
            commands: vec![Command::MoveCall(MoveCall {
                package: ObjectId::from(package),
                module: Identifier::new_unchecked(move_call_json["module"].as_str().unwrap()),
                function: Identifier::new_unchecked(
                    move_call_json["function"].as_str().unwrap(),
                ),
                type_arguments: vec![],
                arguments,
            })],
        }));

        let actual = to_legacy_json(&tx);
        let actual_kind = actual
            .get("V1")
            .and_then(|v1| v1.get("kind"))
            .expect("shadow output always has a V1.kind");

        assert_eq!(actual_kind, &fixture);
    }

    #[test]
    fn pure_input_serializes_as_byte_array_not_base64_string() {
        let tx = wrap_in_transaction(TransactionKind::Programmable(ProgrammableTransaction {
            inputs: vec![Input::Pure(vec![5, 104, 101, 108, 108, 111])],
            commands: vec![],
        }));
        let json = to_legacy_json(&tx);
        assert_eq!(
            json["V1"]["kind"]["ProgrammableTransaction"]["inputs"][0],
            serde_json::json!({ "Pure": [5, 104, 101, 108, 108, 111] })
        );
    }

    #[test]
    fn gas_data_uses_old_field_name_and_plain_numbers() {
        let tx = Transaction::V1(TransactionV1 {
            kind: TransactionKind::Programmable(ProgrammableTransaction {
                inputs: vec![],
                commands: vec![],
            }),
            sender: Address::ZERO,
            gas_payment: GasPayment {
                objects: vec![ObjectReference::new(
                    ObjectId::ZERO,
                    iota_sdk_types::Version::from_u64(7),
                    ObjectDigest::ZERO,
                )],
                owner: Address::ZERO,
                price: 1000,
                budget: 3_000_000,
            },
            expiration: TransactionExpiration::None,
        });

        let json = to_legacy_json(&tx);
        let v1 = &json["V1"];
        assert!(v1.get("gas_data").is_some(), "expected old `gas_data` key, got: {v1}");
        assert!(v1.get("gas_payment").is_none());
        assert_eq!(v1["gas_data"]["price"], serde_json::json!(1000));
        assert_eq!(v1["gas_data"]["budget"], serde_json::json!(3_000_000));
        // old shape: plain 3-element array, version as a number.
        assert_eq!(
            v1["gas_data"]["payment"][0][1],
            serde_json::json!(7),
            "expected numeric version in a 3-tuple, got: {}",
            v1["gas_data"]["payment"][0]
        );
    }

    #[test]
    fn gas_argument_serializes_as_gas_coin() {
        let tx = wrap_in_transaction(TransactionKind::Programmable(ProgrammableTransaction {
            inputs: vec![],
            commands: vec![Command::MergeCoins(MergeCoins {
                coin: Argument::Gas,
                coins_to_merge: vec![],
            })],
        }));
        let json = to_legacy_json(&tx);
        let commands = &json["V1"]["kind"]["ProgrammableTransaction"]["commands"];
        // Old-shape `MergeCoins` is a 2-tuple `[coin, coins_to_merge]`.
        assert_eq!(commands[0]["MergeCoins"][0], serde_json::json!("GasCoin"));
    }

    #[test]
    fn non_move_call_commands_use_old_tuple_array_shape() {
        let tx = wrap_in_transaction(TransactionKind::Programmable(ProgrammableTransaction {
            inputs: vec![],
            commands: vec![Command::TransferObjects(TransferObjects {
                objects: vec![Argument::Input(0)],
                address: Argument::Input(1),
            })],
        }));
        let json = to_legacy_json(&tx);
        let transfer = &json["V1"]["kind"]["ProgrammableTransaction"]["commands"][0]
            ["TransferObjects"];
        assert!(transfer.is_array(), "expected old-style array, got: {transfer}");
        assert_eq!(transfer[0], serde_json::json!([{ "Input": 0 }]));
        assert_eq!(transfer[1], serde_json::json!({ "Input": 1 }));
    }

    #[test]
    fn move_calls_extracts_package_module_function_for_ptb_only() {
        let package = Address::from_str(
            "0xb674e2ed79db3c25fa4c00d5c7d62a9c18089e1fc4c2de5b5ee8b2836a85ae26",
        )
        .unwrap();
        let tx = wrap_in_transaction(TransactionKind::Programmable(ProgrammableTransaction {
            inputs: vec![],
            commands: vec![
                Command::MoveCall(MoveCall {
                    package: ObjectId::from(package),
                    module: Identifier::new_unchecked("allowed_module_name"),
                    function: Identifier::new_unchecked("allowed_function_name"),
                    type_arguments: vec![],
                    arguments: vec![],
                }),
                Command::MergeCoins(MergeCoins {
                    coin: Argument::Gas,
                    coins_to_merge: vec![],
                }),
            ],
        }));

        let calls = move_calls(&tx);
        assert_eq!(calls.len(), 1, "the MergeCoins command must not be counted");
        assert_eq!(calls[0].0, ObjectId::from(package));
        assert_eq!(calls[0].1, "allowed_module_name");
        assert_eq!(calls[0].2, "allowed_function_name");

        // Non-programmable transaction kinds have no move calls at all.
        let system_tx = wrap_in_transaction(TransactionKind::RandomnessStateUpdate(
            RandomnessStateUpdate {
                epoch: 0,
                randomness_round: iota_sdk_types::RandomnessRound::new(0),
                random_bytes: vec![],
                randomness_obj_initial_shared_version: iota_sdk_types::Version::from_u64(1),
            },
        ));
        assert!(move_calls(&system_tx).is_empty());
    }
}
