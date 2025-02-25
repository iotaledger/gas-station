// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use iota_gas_station::command::Command;

#[tokio::main]
async fn main() {
    let command = Command::parse();
    command.execute().await;
}

// test change
#[cfg(test)]
mod test {

    #[test]
    fn test() {
        assert_eq!(1, 1);
    }
}
