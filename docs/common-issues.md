
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

## Lock Acquisition Failed During Startup

**Problem:**

When starting the Gas Station, you encounter one of the following error messages:

```log
Another instance is already performing maintenance. Please wait for it to complete or use the --ignore-locks flag to force a full rescan.
```

or

```log
Another task is already initializing the pool. Please wait for it to complete or use the --ignore-locks flag to force a new initialization.
```

**Explanation:**

The Gas Station uses distributed locks stored in Redis to prevent multiple instances from simultaneously performing critical operations:

- **Maintenance lock:** Acquired during full rescan operations to prevent concurrent registry cleanup and gas coins objects alterations.
- **Initialization lock:** Acquired during coin pool initialization to prevent multiple instances from splitting coins simultaneously.

These locks have a maximum duration (12 hours by default) and are automatically released when the operation completes. However, if the Gas Station crashes or is forcefully terminated during one of these operations, the lock may remain in Redis until it expires naturally.

**Solution:**

If you are certain that no other Gas Station instance is currently performing the locked operation (e.g., after a crash or unexpected termination), you can bypass the lock check using the `--ignore-locks` flag:

```bash
./iota-gas-station --config-path config.yaml --ignore-locks
```

Or with Docker Compose, add the flag to the command:

```yaml
  iota-gas-station:
  ...
    command: ["--config-path", "/app/config.yaml", "--ignore-locks"]
```

> **Warning:** Only use `--ignore-locks` when you are confident that no other instance is actively performing the locked operation. Running concurrent initialization or maintenance operations can lead to data inconsistencies in the coin registry.

## Configuration Changes Requiring Re-initialization

**Problem:**

When starting the Gas Station after modifying certain configuration parameters, you encounter the following error:

```log
Configuration changes requiring re-initialization detected: target_init_balance: Some(1000000000) -> Some(2000000000) but automatic reinitialization is not allowed. Please restart the gas station with the --allow-reinit flag to allow full rescan.
```

**Explanation:**

Some configuration parameters are considered "cold params" — parameters that fundamentally affect how the coin pool is structured. When these parameters change, the existing coin registry becomes incompatible and requires a full re-initialization.

Currently, the following parameters are cold params:

- **`target_init_balance`** — The target balance for each gas coin in the pool. Changing this value requires re-splitting all coins to match the new target size.

When a cold param change is detected, the Gas Station refuses to start automatically to prevent accidental data loss or extended downtime. This is a safety mechanism because:

1. Full re-initialization deletes the existing coin registry and rescans all coins from the network
2. The process can take significant time depending on the number of coins
3. During re-initialization, the instance enters maintenance mode, which blocks all coin reservation requests on the current and all other running instances

**Solution:**

If you intentionally changed a cold param and understand the implications, restart the Gas Station with the `--allow-reinit` flag:

```bash
./iota-gas-station --config-path config.yaml --allow-reinit
```

Or with Docker Compose:

```yaml
  iota-gas-station:
  ...
    command: ["--config-path", "/app/config.yaml", "--allow-reinit"]
```

> **Warning:** During full re-initialization, the Gas Station acquires a maintenance lock. While this lock is held, all other Gas Station instances sharing the same Redis storage will enter maintenance mode and reject sponsorship requests. Plan configuration changes during low-traffic periods to minimize impact on users.
