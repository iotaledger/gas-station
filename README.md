<div align="center">
  <img src=".github/banner_gas_station.png" alt="banner" />
</div>

# IOTA Gas Station

IOTA Gas Station is a component that powers sponsored transactions on IOTA at scale. It manages a database of gas coins owned
by a sponsor address and provides APIs to reserve gas coins and use them to pay for transactions. It achieves
scalability and high throughput by managing a large number of gas coin objects in the pool, so that it can sponsor a
large number of transactions concurrently.

## Documentation

For complete documentation, visit this [link](https://docs.iota.org/operator/gas-station/).

## How to run with Docker

### Prerequisites

* [Git](https://github.com/git-guides/install-git)
* [Docker](https://docs.docker.com/engine/install/)
* [Docker Compose](https://docs.docker.com/compose/install/)

### Setup Steps

1. **Clone the IOTA Gas Station Repository:**

```sh
git clone https://github.com/iotaledger/gas-station
```

2. **Navigate to the Docker Directory and Generate the Config File:**

```sh
cd gas-station/docker
../utils/./gas-station-tool.sh generate-sample-config --config-path config.yaml --docker-compose -n testnet
```

   **Note:** If the generated private key pair doesn’t meet your requirements, replace it with your own keys.

3. **Set Up Authentication:** Define a bearer token for the Gas Station API using the `GAS_STATION_AUTH` environment variable. If set, this token must be provided in all requests to the Gas Station, except for the `/` and `/version` endpoints. It can also be omitted to disable default authentication, e.g. if one wants to add a custom authentication to the server. In this case, requests against the Gas Station can be made without an authentication token.

4. **Start the Gas Station**

```sh
GAS_STATION_AUTH=[bearer_token] docker compose up
```


### Expected Output

When the gas station starts, it will perform the initial coin-splitting procedure. You should see logs similar to the following:

```log
2024-12-16T17:12:49.369620Z  INFO iota_gas_station::gas_station_initializer: Number of coins got so far: 392
2024-12-16T17:12:49.369690Z  INFO iota_gas_station::gas_station_initializer: Splitting finished. Got 392 coins. New total balance: 39615604800. Spent 384395200 gas in total
2024-12-16T17:12:49.381289Z DEBUG iota_gas_station::storage::redis: After add_new_coins. New total balance: 39615604800, new coin count: 392
2024-12-16T17:12:49.381378Z DEBUG iota_gas_station::storage::redis: Releasing the init lock.
2024-12-16T17:12:49.382094Z  INFO iota_gas_station::gas_station_initializer: New coin initialization took 0s
2024-12-16T17:12:49.383373Z  INFO iota_gas_station::rpc::server: listening on 0.0.0.0:9527
```

### API

Your Gas Station instance should now be running and accessible via its [HTTP API](https://docs.iota.org/operator/gas-station/api-reference/).

## How to build

### Build prerequisites

- [Rust 1.86](https://www.rust-lang.org/tools/install)

### Build

To build the gas station binary, run:

```bash
cargo build --release
```

### Binaries

- `./target/release/tool`: gas station helper tool
- `./target/release/iota-gas-station`: gas station server binary

## Configuration

The example configuration file `config.yaml` can be generated with the `tool`. The example of config:

```yaml
signer-config:
  local:
    keypair: AKT1Ghtd+yNbI9fFCQin3FpiGx8xoUdJMe7iAhoFUm4f
rpc-host-ip: 0.0.0.0
rpc-port: 9527
metrics-port: 9184
storage-config:
  redis:
    redis-url: "redis://127.0.0.1"
fullnode-url: "https://grpc.testnet.iota.cafe" # requires redis to be cleared, gRPC endpoint (see "Upgrading from JSON-RPC to gRPC" below)
coin-init-config:
  target-init-balance: 100000000 # requires redis to be cleared
  refresh-interval-sec: 86400
daily-gas-usage-cap: 1500000000000
max-gas-budget: 2000000000
access-controller:
  access-policy: disabled
```

### Configuration parameters

| Parameter                               | Db rebuild required?| Description                                                               | Example                                                                                         |
| --------------------------------------- |---------------------|---------------------------------------------------------------------------| ----------------------------------------------------------------------------------------------- |
| `signer-config`                         | no                  | Configuration of signer. It can be a local or an external KMS.            | See [down below](#signer-configuration)                                                         |
| `rpc-host-ip`                           | no                  | IP address for the RPC server                                             | `0.0.0.0`                                                                                       |
| `rpc-port`                              | no                  | Port for the RPC server                                                   | `9527`                                                                                          |
| `metrics-port`                          | no                  | Port for collecting and exposing metrics                                  | `9184`                                                                                          |
| `storage-config.redis.redis-url`        | no                  | Redis connection URL                                                      | `redis://127.0.0.1`                                                                             |
| `fullnode-url`                          | yes ⚠               | **gRPC** endpoint of the IOTA full node. See [Upgrading from JSON-RPC to gRPC](#upgrading-from-json-rpc-to-grpc) if you are updating an existing deployment. | `https://grpc.testnet.iota.cafe`                                                            |
| `coin-init-config.target-init-balance`  | yes ⚠               | Target balance for the new coins when we splitting new gas coins in NANOs | `100000000`                                                                                     |
| `coin-init-config.refresh-interval-sec` | no                  | Interval in seconds to refresh balance and check for new coins to split   | `86400`                                                                                         |
| `daily-gas-usage-cap`                   | no                  | Maximum allowed daily gas usage                                           | `1500000000000`                                                                                 |
| `max-gas-budget`                        | no                  | Maximum allowed reservable gas budget                                     | `2000000000`                                                                                    |
| `checkpoint-inclusion-timeout-ms`       | no                  | Milliseconds the full node should wait for a transaction to reach checkpoint inclusion (local execution) before responding, when a request asks to wait for local execution. Passed through as the gRPC `execute_transaction` call's `checkpoint_inclusion_timeout_ms`. Defaults to `10000` if omitted. | `10000` |
| `access-controller.access-policy`       | no                  | Access policy mode.                                                       | `disabled`, `allow-all`, `deny-all`. See [this link](./docs/access-controller.md) to learn more |

### Upgrading from JSON-RPC to gRPC

> **Operator-visible breaking change:** the Gas Station now talks to the IOTA full node over **gRPC** instead of JSON-RPC.

**What changes:** `fullnode-url` must now point at the full node's **gRPC** endpoint instead of its JSON-RPC endpoint. The field is still a plain URL string; only the host/port/scheme you put there changes. For the public IOTA networks, that means:

| Network  | Old JSON-RPC URL                    | New gRPC URL                        |
| -------- | ------------------------------------ | ------------------------------------ |
| Mainnet  | `https://api.mainnet.iota.cafe`     | `https://grpc.mainnet.iota.cafe` |
| Testnet  | `https://api.testnet.iota.cafe`     | `https://grpc.testnet.iota.cafe` |
| Devnet   | `https://api.devnet.iota.cafe`      | `https://grpc.devnet.iota.cafe`  |

If you run your own full node, use its gRPC address instead.

**Why:** JSON-RPC is being retired upstream in favor of gRPC as the full node transport, so the Gas Station has moved to the [iota-rust-sdk](https://github.com/iotaledger/iota-rust-sdk) gRPC client ahead of that deprecation.

**What you need to do when upgrading an existing deployment:**

1. Repoint `fullnode-url` at your full node's gRPC endpoint (see table above, or the equivalent for your own node).
2. Make sure the full node you're pointing at actually has its gRPC API enabled: `enable-grpc-api: true` in the node's own config (`grpc-api-config` can be left at its defaults unless you need to tune message-size or timeout limits). A node that only serves JSON-RPC will refuse the connection, or requests against it will fail.
3. `fullnode-basic-auth`, if you use it, is unaffected and passed through the same way.

No Redis flush is required for this change by itself — the gas coin pool doesn't depend on the transport used to talk to the full node. Note also that the new gRPC client connects lazily, so a bad or unreachable `fullnode-url` will now surface as a connection error at startup or on first use, rather than at config-load time.

#### Gas Station reinitialization

The configuration parameter `target-init-balance` requires the Redis database to be cleared (flushed) before any changes to those settings can take effect safely. If you modify these parameters, you will typically be notified that a reinitialization is required. To prevent accidental or unintended reinitializations — which may take a significant amount of time — you must explicitly start the gas station with the `--allow-reinit` flag to allow automatic reinitialization. Alternatively, you can revert the changed parameters to their original values and plan the reinitialization for a more convenient time.

#### Signer Configuration

You can configure the signer in two ways:

- **Local (hardcoded) key** _(unsafe)_

   **Example**:

   ```yaml
   local:
      keypair: AKT1Ghtd+yNbI9fFCQin3FpiGx8xoUdJMe7iAhoFUm4f # base64 encoded private key
   ```

   To convert a private key to base64, follow these steps:
   1. List available keys: `iota keytool list`
   2. Export the key for a selected alias: `iota keytool export --key-identity [alias]`
   3. Convert the bech32 key to base64: `./utils/gas-station-tool.sh convert-key --key iotaprivatkey...`

- **External key management store (KMS)**

   **Example**:

   ```yaml
   sidecar:
      sidecar-url: https://localhost:8001
   ```

   For more details, see the [documentation](https://docs.iota.org/operator/gas-station/architecture/components#key-store-manager) and the [KMS sidecar](./sample_kms_sidecar/) example.

## Sponsored Transaction Examples

- [Rust Example](examples/rust/README.md)
- [TypeScript Example](examples/ts/README.md)

## Common Issues

[See the Common Issues section](./docs/common-issues.md)

## Contributing

We would love to have you help us with the development of IOTA Identity. Each and every contribution is greatly valued!

To contribute directly to the repository, simply fork the project, push your changes to your fork and create a pull request to get them included!

The best place to get involved in discussions about this library or to look for support at is the `#gas-station-dev` channel on the [IOTA Discord](https://discord.iota.org). You can also ask questions on our [Stack Exchange](https://iota.stackexchange.com/).

