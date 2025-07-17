# IOTA Gas Station Docker Setup

This repository contains the Docker and Docker Compose configuration for setting up the IOTA Gas Station along with a Redis instance. The Docker Compose setup is intended for development purposes and should not be used in production.

## Prerequisites

- Docker
- Docker Compose
- CLI helper tool

Before you start, please build the project to have access to the helper CLI tool:

  ```shell
  cargo build
  ```

## How to start

Configuration assumes that you use the localnet IOTA network on your machine.

- Go to the docker folder:

  ```shell
  cd docker
  ```

- Generate the configuration file using the CLI helper tool. Choose the appropriate network: `local`, `devnet`, `testnet`, `mainnet`. The resulting configuration is saved to `config.yaml`.

  ```shell
    ../target/debug/tool generate-sample-config --docker-compose --config-path config.yaml --network testnet
  ```

  Output:

  ```shell
  Generated a new IOTA address. If you plan to use it, please make sure it has enough funds: '0x02bd6b5d4b87b7ef040ba313c804731d.....'
  ```

- Request funds for the address if you plan to use the automatically generated one:

  ```shell
  iota client faucet --address [generated address]
  ```

- Start the Docker Compose environment

  ```shell
  docker-compose up
  ```

## License

This project is licensed under the Apache 2.0 License.
