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

  1. A running instance of the `IOTA Gas Station` is required. To learn how to set up the Gas Station, please follow this [link](../../GETTING_STARTED.md).
    You should have:
    - The URL of the gas station instance.
    - A valid authentication bearer token for its API.
    - Node.js v16+ installed on your machine.

  2. Install dependencies for the project:

    ```bash
      npm install
    ```

  3. Ensure `ts-node` is available by running:
    ```bash
      npx ts-node --version
    ```

## Configuration

  1. Copy `.env.example` to `.env`:
    ```bash
    cp .env.example .env
    ```

  2. Edit the `.env` file:
    - The private key that will interact with the IOTA Gas Station instance.
    - URLs for Node, Explorer, IOTA Gas Station instance.
    - The autentication bearer token for the Gas Station API authentication, if set.

## Run the Example

Once the setup is complete, you can run the example with:

```bash
npx ts-node gasStationTest.ts
```

## Common Issues

If you encounter any problems please check [Common Issues](../../README.md#common-issues) section.
