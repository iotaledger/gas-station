// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! This example demonstrates using the gas station to create a transaction:
//!  - Reserve gas from the gas station
//!  - Create a transaction with the gas object reserved from the gas station
//!  - Sign the transaction with the user's own key
//!  - Let an external hook decide if the transaction should be executed
//!  - Execute the transaction with the gas station, if allowed
//!
//! As this is a hook example, make sure that the example hook server from this project is
//! running (`cargo run` in the `examples/hook` folder) and that your IOTA gas station is
//! using an access controller that relies on a hook.
//!
//! This example passes headers **sent to the gas station** as **part of the payload**
//! to the hook, so no need to configure additional headers in the config, and we can
//! just use the hook's url as the action value:
//!
//! ```yaml
//! access-controller:
//!   access-policy: deny-all
//!   rules:
//!   - action: "http://127.0.0.1:8080"
//! ```
//!
//! Before you run this example, make sure:
//!  - `USER_PRIVATE_KEY` is set to a bech32-encoded (`iotaprivkey1...`) ed25519 private
//!    key for the account that will act as the transaction sender -- e.g. one printed by
//!    `iota keytool generate ed25519`.
//!  - `GAS_STATION_AUTH` is set to the gas station's bearer token, if it requires one.
//!  - the IOTA gas station is running and configured for TESTNET.
//!  - the sender address owns at least one IOTA coin on testnet (it's transferred to
//!    itself below, purely to have something for the sponsored transaction to do).

use iota_gas_station::gas_station::gas_station_core::NANOS_PER_IOTA;
use iota_gas_station::rpc::client::GasStationRpcClient;
use iota_sdk_crypto::{IotaSigner, ToFromBech32, ed25519::Ed25519PrivateKey};
use iota_sdk_grpc_client::Client;
use iota_sdk_transaction_builder::TransactionBuilder;
use iota_sdk_types::StructTag;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

const USER_PRIVATE_KEY_ENV: &str = "USER_PRIVATE_KEY";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a new gas station client.
    let gas_station_client = GasStationRpcClient::new("http://localhost:9527".to_string());

    // Reserve 1 IOTA for 10 seconds.
    let (sponsor_account, reservation_id, gas_coins) = gas_station_client
        .reserve_gas(NANOS_PER_IOTA, 10)
        .await
        .expect("Failed to reserve gas");
    assert!(!gas_coins.is_empty());

    let private_key = std::env::var(USER_PRIVATE_KEY_ENV).unwrap_or_else(|_| {
        panic!(
            "{USER_PRIVATE_KEY_ENV} must be set to a bech32-encoded (`iotaprivkey1...`) ed25519 private key"
        )
    });
    let keypair = Ed25519PrivateKey::from_bech32(&private_key)
        .unwrap_or_else(|err| panic!("{USER_PRIVATE_KEY_ENV} is not a valid bech32 private key: {err}"));
    let user = keypair.public_key().derive_address();

    // Connect to a testnet fullnode over gRPC to resolve an object owned by `user` and the
    // current reference gas price -- the gas station's HTTP API only covers gas
    // sponsorship/execution, not general chain reads.
    let client = Client::new_testnet()?;

    // Get an object owned by the user to transfer -- to itself, purely as a demo payload.
    let page = client
        .list_owned_objects(user, Some(StructTag::new_gas_coin()), Some(1), None, None)
        .await?
        .into_inner();
    let object_ref = page
        .items
        .first()
        .unwrap_or_else(|| panic!("{user} owns no IOTA coins on testnet"))
        .object_reference()?;

    // With a client attached, the builder resolves the current reference gas price from
    // the network itself; the gas payment is still exactly the coins/sponsor the gas
    // station reserved above.
    let mut builder = TransactionBuilder::new(user).with_client(&client);
    builder
        .transfer_objects(user, [object_ref])
        .sponsor(sponsor_account)
        .gas(gas_coins.into_iter().map(|coin| coin.object_id))
        .gas_budget(3_000_000);
    let tx = builder.finish().await?;

    // Sign the transaction with the user's own key.
    let signature = keypair.sign_transaction(&tx)?;

    // Build the headers we want the hook to see as part of the payload sent to it (these
    // headers travel *inside* the `/v1/execute_tx` request body, not as HTTP headers of
    // that request itself).
    let mut headers = HeaderMap::new();
    headers.append(
        HeaderName::from_static("test-response"),
        HeaderValue::from_static(r#"{"decision": "allow"}"#),
    );
    // You can set the `decision` value to other values ("allow"/"deny"/"noDecision") if you
    // want to test different responses, or use the header `test-error` with a string value
    // to test errors returned from the hook.

    // Send the transaction together with the signature to the Gas Station.
    // The Gas Station will execute the transaction and return the effects.
    let effects = gas_station_client
        .execute_tx(reservation_id, &tx, &signature, None, Some(headers))
        .await
        .expect("transaction should be sent");

    println!("Transaction effects: {effects:#?}");

    assert!(
        effects.status.is_success(),
        "transaction did not succeed: {:?}",
        effects.status
    );

    Ok(())
}
