// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2026 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use iota_gas_station::command::Command;

#[tokio::main]
async fn main() {
    let command = Command::parse();
    command.execute().await;
}
