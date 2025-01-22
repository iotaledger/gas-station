use std::path::Path;

use iota_config::IOTA_CLIENT_CONFIG;
use iota_gas_station::rpc::client::GasPoolRpcClient;
use iota_sdk::{wallet_context::WalletContext, IotaClientBuilder};
use iota_types::{
    gas_coin::NANOS_PER_IOTA,
    transaction::{TransactionData, TransactionDataAPI},
};

#[tokio::main]
async fn main() {
    // Before you run this example make sure the IOTA gas station is running, and
    // its configured for the TESTNET

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

    // Build a transaction. Its a TransactionKind as an output, because we build a TransactionData manually in the next step
    let tx_kind = iota_client
        .transaction_builder()
        .transfer_object_tx_kind(object.0, user)
        .await
        .unwrap();

    // Build the TransactionDat that uses the gas coin objects fetched from the gas station
    let mut tx_data = TransactionData::new(tx_kind, user, gas_coins[0], 1000000, ref_gas_price);
    // Set the gas data + sponsor account
    tx_data.gas_data_mut().owner = sponsor_account;
    tx_data.gas_data_mut().payment = gas_coins;

    // Sign the transaction with the wallet the get the signature
    let transaction = wallet_context.sign_transaction(&tx_data);
    let signature = transaction.tx_signatures()[0].to_owned();

    // Send the TransactionData together with the signature to the gas station. The gas station will execute the transaction
    let effects = gas_station_client
        .execute_tx(reservation_id, &tx_data, &signature)
        .await
        .expect("transaction should be sent");

    println!("Transaction effects: {:?}", effects);
}
