# TypeScript Example: IOTA Gas Station Integration

This example demonstrates how to interact with an instance of the `IOTA Gas Station` using the TypeScript SDK. It covers:

1. Reserving gas from the gas station.
2. Creating and signing a transaction locally.
3. Submitting the transaction to the gas station for co-signing and on-chain execution.

In the example `gasStationTest.ts`, the issued transaction calls a function in the system clock package:
- packageId = `0x2`
- moduleName = `clock`
- functionName = `timestamp_ms`
- clockObjectId = `0x6`

This system package is available on every IOTA network instance (devnet, testnet, mainnet, or local), ensuring that this transaction is universally executable across all networks. The purpose of this example is to demonstrate that the caller's address (`user`) does not need to hold any funds (IOTA tokens) to execute a transaction using the gas station.

## Prerequisites

- A running instance of the `IOTA Gas Station` is required. You should have:
  - The URL of the gas station instance.
  - A valid authentication bearer token for its API.
- Node.js v16+ installed on your machine.
- The `ts-node` runtime to execute TypeScript files directly:
  - Install it globally if not already available:
    ```bash
    npm install -g ts-node
    ```
  - Ensure `ts-node` is available by running:
    ```bash
    ts-node --version
    ```
- Install dependencies for the project using `npm` (see the setup section).

## Setup

1. Copy `env_sample` to `.env`:
   ```bash
   cp env_sample .env
   ````

2. Edit the `.env` file:
- The private key (starting with `iotaprivkey...`) that will interact with the IOTA Gas Station instance.
- URLs for Node, Explorer, IOTA Gas Station instance.
- The autentication bearer token for the Gas Station API authentication, if set.

3. Install project dependencies:
   ```bash
   npm install
   ````

## Run the Example
Once the setup is complete, you can run the example with:

  ```bash
  ts-node gasStationTest.ts
  ```

## Common Issues

If you encounter any problems please check [Common Issues](../../README.md#common-issues) section.
