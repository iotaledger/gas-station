// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

pub mod kms_stress;

use crate::rpc::client::GasStationRpcClient;
use clap::ValueEnum;
use iota_sdk_crypto::ed25519::Ed25519PrivateKey;
use iota_sdk_crypto::simple::SimpleKeypair;
use iota_sdk_crypto::IotaSigner;
use iota_sdk_types::{
    GasPayment, ProgrammableTransaction, Transaction, TransactionExpiration, TransactionKind,
    TransactionV1,
};
use parking_lot::RwLock;
use rand::rngs::OsRng;
use rand::Rng;
use std::sync::Arc;
use tokio::time::{interval, Duration, Instant};

/// Was `iota_config::node::DEFAULT_VALIDATOR_GAS_PRICE` (itself a re-export of
/// `iota_types::transaction::DEFAULT_VALIDATOR_GAS_PRICE`, confirmed still
/// `1000` at the `v1.20.1` tag). Not part of any wire format -- just a
/// stand-in gas price for this benchmark's synthetic transactions -- so a
/// local constant replaces the dependency instead of a real SDK equivalent.
const DEFAULT_VALIDATOR_GAS_PRICE: u64 = 1000;

#[derive(Copy, Clone, ValueEnum)]
pub enum BenchmarkMode {
    ReserveOnly,
    ReserveAndExecute,
}

#[derive(Clone, Default)]
struct BenchmarkStatsPerSecond {
    pub num_requests: u64,
    pub total_latency: u128,
    pub num_errors: u64,
}

impl BenchmarkStatsPerSecond {
    pub fn update_success(&mut self, latency: u128) {
        self.num_requests += 1;
        self.total_latency += latency;
    }

    pub fn update_error(&mut self) {
        self.num_requests += 1;
        self.num_errors += 1;
    }
}

impl BenchmarkMode {
    pub async fn run_benchmark(
        &self,
        gas_station_url: String,
        reserve_duration_sec: u64,
        num_clients: u64,
    ) {
        let mut handles = vec![];
        let stats = Arc::new(RwLock::new(BenchmarkStatsPerSecond::default()));
        let client = GasStationRpcClient::new(gas_station_url);
        let should_execute = matches!(self, Self::ReserveAndExecute);
        for _ in 0..num_clients {
            let client = client.clone();
            let stats = stats.clone();
            let handle = tokio::spawn(async move {
                let keypair = SimpleKeypair::from(Ed25519PrivateKey::generate(OsRng));
                let sender = keypair.public_key().derive_address();
                let mut rng = OsRng;
                loop {
                    let now = Instant::now();
                    let budget = rng.gen_range(1_000_000u64..100_000_000u64);
                    let result = client.reserve_gas(budget, reserve_duration_sec).await;
                    let (sponsor, reservation_id, gas_coins) = match result {
                        Ok(r) => r,
                        Err(err) => {
                            stats.write().update_error();
                            println!("Error: {}", err);
                            continue;
                        }
                    };
                    if !should_execute {
                        stats.write().update_success(now.elapsed().as_millis());
                        continue;
                    }

                    let pt = ProgrammableTransaction {
                        inputs: vec![],
                        commands: vec![],
                    };
                    let tx = Transaction::V1(TransactionV1 {
                        kind: TransactionKind::Programmable(pt),
                        sender,
                        gas_payment: GasPayment {
                            objects: gas_coins,
                            owner: sponsor,
                            price: DEFAULT_VALIDATOR_GAS_PRICE,
                            budget,
                        },
                        expiration: TransactionExpiration::None,
                    });
                    let user_sig = match keypair.sign_transaction(&tx) {
                        Ok(sig) => sig,
                        Err(err) => {
                            stats.write().update_error();
                            println!("Error: {}", err);
                            continue;
                        }
                    };
                    let result = client
                        .execute_tx(reservation_id, &tx, &user_sig, None, None)
                        .await;
                    if let Err(err) = result {
                        stats.write().update_error();
                        println!("Error: {}", err);
                    } else {
                        stats.write().update_success(now.elapsed().as_millis());
                    }
                }
            });
            handles.push(handle);
        }
        let handle = tokio::spawn(async move {
            let mut prev_stats = stats.read().clone();
            let mut interval = interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                let cur_stats = stats.read().clone();
                let request_per_second = cur_stats.num_requests - prev_stats.num_requests;
                let num_errors = cur_stats.num_errors - prev_stats.num_errors;
                println!(
                    "Requests per second: {}, errors per second: {}, average latency: {}ms",
                    request_per_second,
                    num_errors,
                    if request_per_second == 0 {
                        0
                    } else {
                        (cur_stats.total_latency - prev_stats.total_latency)
                            / ((request_per_second - num_errors) as u128)
                    }
                );
                prev_stats = cur_stats;
            }
        });
        handle.await.unwrap();
    }
}
