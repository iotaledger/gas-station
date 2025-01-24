use std::path::Path;

use iota_config::IOTA_CLIENT_CONFIG;
use iota_gas_station::rpc::client::GasPoolRpcClient;
use iota_sdk::{wallet_context::WalletContext, IotaClientBuilder};
use iota_types::{
    gas_coin::NANOS_PER_IOTA,
    transaction::{TransactionData, TransactionDataAPI},
};

// This example demonstrates using the gas station to create a transaction
//  - Reserve gas from the gas station
//  - Create a transaction with the gas object reserved from the gas station
//  - Sign the transaction with the wallet
//  - Execute the transaction with the gas station

// Before you run this example make sure:
//  - GAS_STATION_AUTH env is set to the correct value
//  - the IOTA gas station is running, an its configured for the TESTNET
#[tokio::main]
async fn main() {
    // Create a new gas station client
    let gas_station_url = "http://localhost:9527".to_string();
    let gas_station_client = GasPoolRpcClient::new(gas_station_url);

    // Reserve the 1 IOTA for 10 seconds
    let (sponsor_account, reservation_id, gas_coins) = gas_station_client
        .reserve_gas(NANOS_PER_IOTA, 10)
        .await
        .expect("Failed to reserve gas");
    assert!(gas_coins.len() >= 1);

    // Create a new IOTA Client
    let iota_client = IotaClientBuilder::default().build_testnet().await.unwrap();

    // Load the config from default location (~./iota/iota_config/client.yaml)
    let config_path = format!(
        "{}/{}",
        iota_config::iota_config_dir().unwrap().to_str().unwrap(),
        IOTA_CLIENT_CONFIG
    );
    let mut wallet_context = WalletContext::new(&Path::new(&config_path), None, None).unwrap();

    // Get the first gas object owned by the address
    let user = wallet_context.active_address().unwrap();
    let object = wallet_context
        .get_one_gas_object_owned_by_address(user)
        .await
        .unwrap()
        .unwrap();

    let ref_gas_price = iota_client
        .governance_api()
        .get_reference_gas_price()
        .await
        .unwrap();

    // Create the TransactionKind.
    // TransactionKind is an type that doesn't have information about gas and sender.
    let tx_kind = iota_client
        .transaction_builder()
        .transfer_object_tx_kind(object.0, user)
        .await
        .unwrap();

    // Build the TransactionData.
    // TransactionData is unsigned version of Transaction. The maximum gas budget is 0.001 IOTA.
    let mut tx_data = TransactionData::new(tx_kind, user, gas_coins[0], 1000000, ref_gas_price);
    // Set the gas object and gas-station sponsor account fetched from the gas station
    tx_data.gas_data_mut().payment = gas_coins;
    tx_data.gas_data_mut().owner = sponsor_account;

    // Sign the TransactionData with the wallet.
    let transaction = wallet_context.sign_transaction(&tx_data);
    let signature = transaction.tx_signatures()[0].to_owned();

    // Send the TransactionData together with the signature to the Gas Station.
    // The Gas Station will execute the Transaction and returns the effects.
    let effects = gas_station_client
        .execute_tx(reservation_id, &tx_data, &signature)
        .await
        .expect("transaction should be sent");

    println!("Transaction effects: {:?}", effects);

    assert_eq!(effects.into_status(), IotaExecutionStatus::Success);
}
