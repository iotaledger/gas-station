# Example Hook Server

This is an example server for the [hook integration](../../docs/access-controller.md#hook-server) and is also used to generate the [openapi spec](../../docs/openapi.json) and its [endpoint types](./src/endpoint_types.rs) are reflected in the gas stations [hook server types](../../src/access_controller/hook/hook_server_types.rs).

## Starting the server

The server can be started with

```sh
cargo run --release
```

By default these addresses are bound:

```raw
hook listening on:              http://127.0.0.1:8080
OpenAPI UI served on:           http://127.0.0.1:8080/swagger-ui
OpenAPI API spec file on:       http://127.0.0.1:8080/apidoc/openapi.json
```

As you can see, a Swagger UI interface is started alongside the actual hook endpoint and can be used for analysis and debugging. If you want to opt out of this, you can disable the `swagger-ui` feature, which is included in the default features:

```sh
cargo run --release --no-default-features 
```

## Test behavior

To simulate an actual hooks behavior and its responses, the example server will return responses if the are included in specific headers, those headers are `test-response` and `test-error`.

`test-response` header, if provided must contain a JSON serialized `ExecuteTxOkResponse` object. `test-response` header may container an error message, that will be returned as a BAD_REQUEST error with this message embedded in it.

For example, if your using the `GasStationRpcClient` as done in our [Rust example](../rust/sponsored_transaction.rs), and have the example hook configured to be hit by your request, this might look as following:

```Rust
let mut headers = HeaderMap::new();
headers.insert(
    "test-response",
    HeaderValue::from_str(r#"{"decision":"allow"}"#).unwrap(),
);
let effects = gas_station_client
    .execute_tx(reservation_id, &tx_data, &signature, Some(headers))
    .await
    .expect("transaction should be sent");
```

And an error response could be triggered with this header setup (which will then panic due to the `.expect`):

```Rust
let mut headers = HeaderMap::new();
headers.insert(
    "test-error",
    HeaderValue::from_str("I'm an error message").unwrap(),
);
let effects = gas_station_client
    .execute_tx(reservation_id, &tx_data, &signature, Some(headers))
    .await
    .expect("transaction should be sent");
```
