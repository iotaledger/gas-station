# Gas Station Example - Rust

## Prerequisites

1. Ensure the IOTA Gas Station is up and running. To learn how to set up the Gas Station, please follow this [link](../../README.md#how-to-run-with-docker).
2. The expected address of the Gas Station is `http://localhost:9527`. Modify this if necessary.
3. Set `USER_PRIVATE_KEY` to a bech32-encoded (`iotaprivkey1...`) ed25519 private key for
   the account that will act as the transaction sender -- e.g. one printed by
   `iota keytool generate ed25519`. These examples have no keystore/wallet-config support
   (the new `iota-rust-sdk` doesn't provide one), so a private key must be supplied this
   way. The sender address needs to own at least one IOTA coin on testnet.

## How to run

```bash
USER_PRIVATE_KEY='iotaprivkey1...' GAS_STATION_AUTH='your-bearer-token' cargo run --example sponsored_transaction
```

## Common Issues

If you encounter any problems please check [Common Issues](../../docs/common-issues.md) section.
