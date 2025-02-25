<div align="center">
  <img src=".github/imgs/banner_gas_station.svg" alt="banner" />
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
../utils/./gas-station-tool.sh generate-sample-config --config-path config.yaml --docker-compose -e testnet
```

   **Note:** If the generated private key pair doesn’t meet your requirements, replace it with your own keys.

3. **Set Up Authentication:** Define a bearer token for the Gas Station API using the `GAS_STATION_AUTH` environment variable.

4. **Start the Gas Station**

```sh
GAS_STATION_AUTH=[bearer_token] docker-compose up
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

- [Rust 1.84](https://www.rust-lang.org/tools/install)

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
    redis_url: "redis://127.0.0.1"
fullnode-url: "https://api.testnet.iota.cafe"
coin-init-config:
  target-init-balance: 100000000
  refresh-interval-sec: 86400
daily-gas-usage-cap: 1500000000000
access-controller:
  access-policy: disabled
```

### Configuration parameters

| Parameter                               | Description                                                         | Example                          |
| --------------------------------------- | ------------------------------------------------------------------- | -------------------------------- |
| `signer-config`                         | Configuration of signer. It can be a local or an external KMS.      |  See [down below](#signer-config)|
| `rpc-host-ip`                           | IP address for the RPC server                                       | `0.0.0.0`                        |
| `rpc-port`                              | Port for the RPC server                                             | `9527`                           |
| `metrics-port`                          | Port for collecting and exposing metrics                            | `9184`                           |
| `storage-config.redis.redis_url`        | Redis connection URL                                                | `redis://127.0.0.1`              |
| `fullnode-url`                          | URL of the IOTA full node                                           | `https://api.testnet.iota.cafe`  |
| `coin-init-config.target-init-balance`  | Initial balance to maintain                                         | `100000000`                      |
| `coin-init-config.refresh-interval-sec` | Interval in seconds to refresh balance                              | `86400`                          |
| `daily-gas-usage-cap`                   | Maximum allowed daily gas usage                                     | `1500000000000`                  |
| `access-controller.access-policy`       | Access policy mode.                                                 | `disabled`, `allow-all`, `deny-all`. See [this link](./docs/access-controller.md) to learn more|

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

   For more details, see the [documentation](https://doca.iota.org/operator/gas-station/architecture/components#key-store-manager) and the [KMS sidecar](./sample_kms_sidecar/) example.

## Sponsored Transaction Examples

- [Rust Example](examples/rust/README.md)
- [TypeScript Example](examples/ts/README.md)

## Common Issues

[See the Common Issues section](./docs/common-issues.md)

## Contributing

We would love to have you help us with the development of IOTA Identity. Each and every contribution is greatly valued!

To contribute directly to the repository, simply fork the project, push your changes to your fork and create a pull request to get them included!

The best place to get involved in discussions about this library or to look for support at is the `#gas-station-dev` channel on the [IOTA Discord](https://discord.iota.org). You can also ask questions on our [Stack Exchange](https://iota.stackexchange.com/).

