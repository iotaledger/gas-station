
# Common Issues

## Could not find the referenced object

**Problem:**

When you make a transaction with returned gas coins, the following error is returned:

```log
ErrorObject { code: ServerError(-32002), message: "Transaction execution failed due to issues with transaction inputs, please review the errors and try again: Could not find the referenced object 0x0494e5cf17473a41b8f51bb0f2871fbf28f27e1d890d165342edea0033f8d35e at version None.", data: None }
```

**Explanation:**

This error typically occurs because the Gas Station has returned objects that were created for a different network. For example, the network address may have changed. The Gas Station stores addresses of the gas coins locally in Redis and does not recognize switching between networks or environments. As a result, "old objects" still exist and may be returned.

**Solution:**

Clean up the Redis storage.

If you started Redis with `make redis-start`, please restart the Redis instance using `make redis-restart`.

If you have a local instance or an instance with persistent storage, you can use `redis-cli`:

> **Note:** This will delete ALL items from Redis. Ensure the Redis instance isn't shared with any other services.

```bash
redis-cli FLUSHALL
```

## Warning: Oversized Coins

**Problem:**

After executing a transaction, the following warning appears:

```log
Oversized coins found during transaction execution. Initiating rescan to split these coins. If this occurs frequently, consider adjusting target_init_balance or the maximum transaction budget.
```

**Explanation:**

The Gas Station uses a self-balancing algorithm to maintain optimal liquidity by keeping gas coins at a manageable size. When a transaction is executed, the unused portion of the gas coin is returned as change. If this change exceeds `200 × target_init_balance`, it is considered "oversized."

The Gas Station automatically downsizes oversized coins by splitting them into smaller coins (approximately `target_init_balance` each) to maintain pool liquidity. This rescan and split process consumes additional computing resources and reduces performance. If this happens frequently, it indicates that your configuration needs adjustment.

Under normal operating conditions with properly configured `target_init_balance`, the returned change should remain below the `200 × target_init_balance` threshold, allowing coins to be returned directly to the pool without requiring expensive splitting operations.

**Solution:**

To minimize oversized coin occurrences, ensure the difference between reserved gas and actual gas usage stays below `200 × target_init_balance`. You can optimize your configuration by:

- **Reducing the gas reservation size** - Reserve amounts closer to actual usage
- **Increasing `target_init_balance`** - Set it appropriate to your traffic patterns

**Monitoring:**

The Gas Station provides Prometheus metrics to help you tune your configuration:

- `reserved_gas_real_gas_usage_delta` - A histogram tracking the difference between reserved gas and actual gas usage. You can perform statistical queries on this metric to understand your usage patterns. Ideally, this delta should never exceed `200 × target_init_balance`.

- `oversized_gas_coins_count` - Tracks how frequently oversized gas coins occur. Lower is better.

The acceptable frequency depends on your performance requirements and traffic volume. For example:

- **Low volume:** 5% of transactions returning oversized gas might trigger a rescan once per day (likely acceptable)
- **High volume:** 5% of transactions could trigger constant rescans (problematic and requires configuration adjustment)

## Access Denied by Access Controller

**Problem:**

When executing a transaction, you get a 403 HTTP response with the message: `Access Denied by Access Controller`

**Explanation:**

The Access Controller in Gas Station is a robust tool designed to enable the creation of highly intricate filtering logic. However, with its complexity and flexibility, it can become hard to pin down why a certain transaction was rejected or allowed. This might become even more likely in larger and/or more fine granular rule sets.

**Solution:**

The IOTA Gas Station [PR](https://github.com/iotaledger/gas-station/pull/114) introduces the ability to inspect in detail the behavior of the Access Controller.

To see a detailed report of the transaction, you must enable the `tracing` level for the access controller module. This can be done using the `RUST_LOG` environment variable.

If you start the binary directly:

```log
   RUST_LOG=iota_gas_station::access_controller=trace ./iota-gas-station
```

In case you use Docker Compose, please edit the docker-compose file:

```yaml
  iota-gas-station:
  ...
    environment:
    - RUST_LOG=iota_gas_station::access_controller=trace
```
