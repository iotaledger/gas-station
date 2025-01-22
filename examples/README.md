# IOTA Gas Station

This guide is for setting up and using the IOTA Gas Station on the IOTA test network.
Latest tested version: `0.7.3-rc`

## Prerequisites

Before you start, ensure you have:

1. Installed the `iota` CLI binaries.
2. Configured them for the IOTA test network.

Your configuration file `.iota/iota_config/client.yaml` should include a section for the testnet:

```yaml
- alias: testnet
  rpc: "https://api.testnet.iota.cafe"
  ws: ~
  basic_auth: ~
```

## Setting Up the Gas Station

### Step 1: Redis Setup

The gas station requires Redis to sync state across multiple instances.
To set up Redis, run the following command:

```bash
docker run -d --name redis -p 6379:6379 redis
```

### Step 2: Configuration

The gas station requires a configuration file named `config.yaml`. Below is an example configuration for the testnet:

```yaml
signer-config:
  local:
    keypair: AKT1Ghtd+yNbI9fFCQin3FpiGx8xoUdJMe7iAhoFUm4f
rpc-host-ip: 0.0.0.0
rpc-port: 9527
metrics-port: 9184
gas-pool-config:
  redis:
    redis_url: "redis://127.0.0.1"
fullnode-url: "https://api.testnet.iota.cafe"
coin-init-config:
  target-init-balance: 100000000
  refresh-interval-sec: 86400
daily-gas-usage-cap: 1500000000000
access-controller:
  access-policy: disabled
```

> **Note:** Replace the `keypair` value with your own keypair to be used as the sponsor.

### Step 3: Funding the Sponsor

Ensure the sponsor address has enough gas coins by using the faucet command:

```bash
iota client faucet --address 0x22bf13eb9ab01e1b3d6ae605a7e94af6552fa8ccab81c2ef9b50be1653eb9f9d
```

> **Note:** Replace the address above with your own sponsor address.

### Step 4: Building the Gas Station

To build the gas station binary, run:

```bash
cargo run
```

This will generate the binary at: `./target/debug/iota-gas-station`.

Once the build is complete, start the gas station with:

```bash
GAS_STATION_AUTH="a" ./target/debug/iota-gas-station --config-path config.yaml
```

### Expected Output

When the gas station starts, it will perform the initial coin-splitting procedure. You should see logs similar to the following:

```log
2024-12-16T17:12:49.369620Z  INFO iota_gas_station::gas_pool_initializer: Number of coins got so far: 392
2024-12-16T17:12:49.369690Z  INFO iota_gas_station::gas_pool_initializer: Splitting finished. Got 392 coins. New total balance: 39615604800. Spent 384395200 gas in total
2024-12-16T17:12:49.381289Z DEBUG iota_gas_station::storage::redis: After add_new_coins. New total balance: 39615604800, new coin count: 392
2024-12-16T17:12:49.381378Z DEBUG iota_gas_station::storage::redis: Releasing the init lock.
2024-12-16T17:12:49.382094Z  INFO iota_gas_station::gas_pool_initializer: New coin initialization took 0s
2024-12-16T17:12:49.383373Z  INFO iota_gas_station::rpc::server: listening on 0.0.0.0:9527
```

## Example Usage

Check the [Example](create_transaction.rs) to learn how to create sponsored transactions.
