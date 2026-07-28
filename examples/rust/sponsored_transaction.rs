// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! This example demonstrates using the gas station to create a transaction:
//!  - Reserve gas from the gas station
//!  - Create a transaction with the gas object reserved from the gas station
//!  - Sign the transaction with the user's own key
//!  - Execute the transaction with the gas station
//!
//! Unlike `hook_with_request_headers`/`hook_with_config_headers` (which talk to this
//! service's `/v1/reserve_gas`/`/v1/execute_tx` endpoints directly through
//! [`GasStationRpcClient`]), this example instead uses
//! `iota-sdk-transaction-builder`'s own **native** gas-station support --
//! [`TransactionBuilder::gas_station_sponsor`] and
//! [`TransactionBuilder::add_gas_station_header`] -- which speaks the exact same wire
//! protocol (`/version`, `/v1/reserve_gas`, `/v1/execute_tx`) internally. This is both a
//! shorter way to write the example *and* a from-the-client-side check that this
//! service's wire contract is compatible with the upstream SDK's own gas-station client
//! (verified against `iota-sdk-transaction-builder`'s `builder/gas_station.rs` at the
//! pinned SDK revision this crate migrated to).
//!
//! Before you run this example, make sure:
//!  - `USER_PRIVATE_KEY` is set to a bech32-encoded (`iotaprivkey1...`) ed25519 private
//!    key for the account that will act as the transaction *sender* -- e.g. one printed
//!    by `iota keytool generate ed25519`. This is **not** the gas station's sponsor
//!    account: the gas station supplies its own sponsor address and gas coins via
//!    `/v1/reserve_gas`, this key only needs to own (and sign for) the object being
//!    transferred below.
//!  - `GAS_STATION_AUTH` is set to the gas station's bearer token, if it requires one.
//!  - the IOTA gas station is running and configured for TESTNET.
//!  - the sender address owns at least one IOTA coin on testnet (it's transferred to
//!    itself below, purely to have something for the sponsored transaction to do).

use iota_sdk_crypto::ToFromBech32;
use iota_sdk_crypto::ed25519::Ed25519PrivateKey;
use iota_sdk_grpc_client::Client;
use iota_sdk_transaction_builder::TransactionBuilder;
use iota_sdk_types::StructTag;

const USER_PRIVATE_KEY_ENV: &str = "USER_PRIVATE_KEY";
const GAS_STATION_AUTH_ENV: &str = "GAS_STATION_AUTH";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let private_key = std::env::var(USER_PRIVATE_KEY_ENV).unwrap_or_else(|_| {
        panic!(
            "{USER_PRIVATE_KEY_ENV} must be set to a bech32-encoded (`iotaprivkey1...`) ed25519 private key"
        )
    });
    let keypair = Ed25519PrivateKey::from_bech32(&private_key)
        .unwrap_or_else(|err| panic!("{USER_PRIVATE_KEY_ENV} is not a valid bech32 private key: {err}"));
    let sender = keypair.public_key().derive_address();
    println!("Sender address: {sender}");

    let gas_station_url = url::Url::parse("http://localhost:9527")?;

    // Connect to a testnet fullnode over gRPC. The gas station's own HTTP API only
    // covers gas sponsorship and execution -- resolving the sender's own objects, the
    // reference gas price, and (via `execute`, below) confirming the executed effects
    // still all go through a fullnode.
    let client = Client::new_testnet()?;

    // Find a coin owned by the sender to use as this demo's payload: transfer one of the
    // sender's own IOTA coins back to themselves. (Mirrors the old wallet-context-based
    // version of this example, which grabbed "the first gas object owned by the
    // address" -- any object the sender owns would do here, a coin is just the simplest
    // one to look up.)
    let page = client
        .list_owned_objects(sender, Some(StructTag::new_gas_coin()), Some(1), None, None)
        .await?
        .into_inner();
    let coin_id = page
        .items
        .first()
        .unwrap_or_else(|| panic!("{sender} owns no IOTA coins on testnet"))
        .object_reference()?
        .object_id;

    let mut builder = TransactionBuilder::new(sender).with_client(&client);
    let gas_station_builder = builder
        .transfer_objects(sender, [coin_id])
        .gas_station_sponsor(gas_station_url);
    if let Ok(auth) = std::env::var(GAS_STATION_AUTH_ENV) {
        if !auth.is_empty() {
            gas_station_builder.add_gas_station_header(
                http::header::AUTHORIZATION,
                http::HeaderValue::from_str(&format!("Bearer {auth}"))?,
            );
        }
    }

    // `execute` builds the transaction (auto-selecting gas price/budget via the
    // fullnode), sends it to the gas station for sponsorship and execution, then polls
    // the fullnode until finalized and returns its effects -- all in one call.
    let effects = builder.execute(&keypair, None).await?;
    println!("Transaction effects: {effects:#?}");

    assert!(
        effects.as_v1().status.is_success(),
        "transaction did not succeed: {:?}",
        effects.as_v1().status
    );

    Ok(())
}
