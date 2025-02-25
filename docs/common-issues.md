
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
