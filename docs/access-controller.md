# Gas Station Server Access Controller

The **Gas Station Server** includes an **Access Controller** mechanism to manage access to the `/execute_tx` endpoint. This feature allows you to implement filtering logic based on properties derived from transactions. Currently, the Access Controller supports filtering based on the sender's address, enabling you to block or allow specific addresses.

## Access Controller Rule syntax

|  parameter                  | mandatory  | possible values                                                |
|-----------------------------| -----------|----------------------------------------------------------------|
| `sender-address`            |  yes       | `'0x0000...'`, `[0x0000.., 0x1111...]`, `'*'`                  |
| `gas-budget`                |  no        | `'=100'`, `'<100'`,  `'<=100'`, `'>100'`, `'>=100'`, `'!=100'` |
| `move-call-package-address` |  no        | `'0x0000...'`, `[0x0000..., 0x1111...]`, `'*'`                 |
| `ptb-command-count`         |  no        | `'=10'`, `'<10'`,  `'<=10'`, `'>10'`, `'>=10'`, `'!=10'`       |
| `action`                    |  yes       | `'allow'`,  `'deny'`                                           |
| `gas_usage`                 |  no        | See [Gas Usage Filter](#gas-usage-filter)                |
| `rego_expression`           |  no        | See [Gas Rego Expression](#rego-expression-filter)             |

## Access Controller Examples

- Disable All Requests and Allow Only a Specific Address

   The following configuration denies all incoming transactions except for move calls to package (`0x0202....`) originating from the specified sender address (`0x0101....`):

   ```yaml
   access-controller:
      access-policy: deny-all
      rules:
         - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
           move-call-package-address: "0x0202020202020202020202020202020202020202020202020202020202020202"
           action: 'allow' # allowed actions: 'allow', 'deny'
   ```

---

- Enables All Requests and Deny Only a Specific Address

   The following configuration allows all incoming transactions except those from the specified sender address (`0x0101...`):

   ```yaml
   access-controller:
      access-policy: deny-all
      rules:
         - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
           action: 'deny'
   ```

---

- Gas Budget Constraints

   The following configuration denies all incoming transactions except those from the specified sender address (`0x0101...`) and the transaction gas budget below the limit `1000000`

   ```yaml
   access-controller:
      access-policy: deny-all
      rules:
         - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
           transaction-gas-budget: '<1000000' # allowed operators: =, !=, <, >, <=, >=
           action: 'allow'
   ```

---

- Advanced budgeting management

   The following configuration accept all incoming transactions with gas budget below `500000`. For address sender address (`0x0101...`) the allowed gas budget is increased to `1000000`

   ```yaml
   access-controller:
      access-policy: deny-all
      rules:
         - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
           transaction-gas-budget: '<=10000000'
           action: 'allow'

         - sender-address: '*'
           transaction-gas-budget: '<500000'
           action: 'allow'
   ```

---

- Programmable Transaction Command Count Limits

   To avoid users submitting transactions blocks with a large number of transactions, limits for the commands in the programmable transaction can be configured. In the following example, the sender may only submit up to one command in the programmable transaction.

   Note that this rule condition is only applied to transactions, that include a programmable transaction and will be ignored for other transaction kinds.

   ```yaml
   access-controller:
      access-policy: deny-all
      rules:
         - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
           ptb-command-count: <=1 # allowed operators: =, !=, <, >, <=, >=
           action: 'allow'
   ```

---

## Rego Expression Filter

The Rego Expression Filter allows you to evaluate incoming transaction payloads against custom logic by using the Rego language. This gives you the flexibility to check properties like the sender address or any other field available in the transaction data.

### Rego Expression Input Payload

Below is an example JSON payload against which a Rego rule is evaluated:

```json
{
  "transaction_data": {
    "V1": {
      "kind": {
        "ProgrammableTransaction": {
          "inputs": [
            {
              "Pure": [
                162,
                225,
                126,
                32,
                249,
                115,
                85,
                175,
                100,
                145,
                88,
                15,
                245,
                193,
                30,
                206,
                252,
                220,
                247,
                110,
                162,
                36,
                209,
                99,
                229,
                203,
                146,
                56,
                154,
                223,
                35,
                17
              ]
            },
            {
              "Object": {
                "ImmOrOwnedObject": [
                  "0x03ea0313a97c75f2526839742883566d3dc48c43967a1cc73a1cb7cc27c527ad",
                  116899037,
                  "AL8isXnVECWJX6V2S29HqeRfXR4GSDfefGXWbAv9TCqR"
                ]
              }
            }
          ],
          "commands": [
            {
              "TransferObjects": [
                [
                  {
                    "Input": 1
                  }
                ],
                {
                  "Input": 0
                }
              ]
            }
          ]
        }
      },
      "sender": "0xa2e17e20f97355af6491580ff5c11ecefcdcf76ea224d163e5cb92389adf2311",
      "gas_data": {
        "payment": [
          [
            "0xbfa24cd746dcd19853c8ba18bb40608548534d0eee7f7efd685934c7cf7bfbeb",
            9511748,
            "BpmGcSGwxjW9Rw7W6SYLGdBfaPYoN2ovueWHohDvutiw"
          ],
          [
            "0xbfe89eb1a6b8e2589c0226de40fd3319ce9cbf4c25f8567fa1c8184e2c340af4",
            9511752,
            "BbjrEihLL3TtQ1yDtyrYvwSJtPV75SBwTsBqj9axAFia"
          ],
          [
            "0xc03b0dcd0fcfd67a1467db23b469678c4a354f1ce83cb4380b6e169b6db46e56",
            9511753,
            "81T1q56gZ73GvmKjQLNZa5bBVUAU4vM31mhwgLwpNhfF"
          ],
          [
            "0xc051d0b7b2f1025287dd84f057f3e051581e75f2afbac5b49b62b24281492752",
            9511751,
            "8hiMawGXbZTugCP9g4aY7oa3XZpru9PPGbZfi3FnfmfW"
          ],
          [
            "0xc08908166ea55f45b743a0fe53cddc5f5102b5296de041e57e535757a0307698",
            9511748,
            "MCt4JfF72w2VruuWUXZ8HMQHsLR7ciw2kkkAisxFCjm"
          ]
        ],
        "owner": "0x27147325dafdae103c7e8f09a82654ae7a4654c3042e1e278187013065be47b7",
        "price": 1000,
        "budget": 3000000
      },
      "expiration": "None"
    }
  }
}
```

### Rego Code Example

The following Go code demonstrates how to use a Rego expression to check the sender address:

```go
package sample_rego

import rego.v1

default sender_matches := false

sender_matches if {
    input.transaction_data.V1.sender = "0xa2e17e20f97355af6491580ff5c11ecefcdcf76ea224d163e5cb92389adf2311"
}
```

> **Note:** All field addresses in the Rego expression should begin with `input`. For full syntax details, please see the [Reference](https://link_do_rego_reference).

### Rego Expression Sources

The Rego expressions may come from different sources:

- **File:** Example configuration to load a rule from a file.
- **Redis:** Example configuration loading a rule from Redis.
- **HTTP:** Example configuration loading a rule via HTTP.

#### Rego from File

```yaml
access-controller:
  access-policy: allow-all
  rules:
    - rego-expression:
        location-type: file
        url: file://./source_file.rego
        rego-rule-name: data.sample_rego.sender_matches
      action: 'deny'
```

#### Rego from Redis

```yaml
access-controller:
  access-policy: allow-all
  rules:
    - rego-expression:
        location-type: redis
        url: "redis://localhost"
        redis-key: key_with_sample_rego
        rego-rule-name: data.sample_rego.sender_matches
      action: 'deny'
```

#### Rego from HTTP

```yaml
access-controller:
  access-policy: allow-all
  rules:
    - rego-expression:
        location-type: http
        url: "http://localhost:8080"
        rego-rule-name: data.sample_rego.sender_matches
      action: 'deny'
```

## Gas Usage Filter

The **Gas Usage Limit** feature enables you to track gas consumption based on predefined parameters. When enabled, the gas tracking applies to the entire rule. The configuration syntax is:

```yaml
gas-usage:
  value: [range_of_numbers]
  window: [duration]
  count-by: [ sender-address ] # optional
```

> **Note:** The syntax of `duration` follows the specification used in the [`humantime`](https://docs.rs/humantime/latest/humantime/index.html) crate

### Gas Usage Examples

Below are two examples that demonstrate how to enforce gas usage limits.

---

**1. Limit Gas Usage per Address**

This configuration sets a daily gas usage limit for a specific address. In the example below, the sender at address `0x0101...` is restricted to a maximum daily usage of `10000000` gas units.

The time window is reset not on a daily reset time, but 24 hours (as configured below) after the first transaction of sender at address `0x0101...`, allowing to have flexible usage based scheduling across different time zones.

> Note that gas usage for other addresses remains unconstrained.

```yaml
access-controller:
  access-policy: deny-all
  rules:
    - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
      gas-usage:
        value: '>1000000'
        window: 1 day
      action: 'deny'

    - sender-address: '*'
      action: 'allow'
```

---

**2. Limit Gas Usage per Address and Module**

In this example, the configuration restricts the daily gas usage for a specific address when calling a designated module. The sender at address `0x0101...` is limited to a maximum daily usage of `10000000` gas units when accessing package `0x0202...`. For all other interactions, gas usage is **blocked**.

```yaml
access-controller:
  access-policy: deny-all
  rules:
    - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
      move-call-package-address: "0x0202020202020202020202020202020202020202020202020202020202020202"
      gas-usage:
        value: '<1000000'
        window: 1 day
      action: 'allow'
```


**3. Limit Gas Usage per Address**

In this example, each user is limited to `10000000` gas units per day. The additional property `count-by` allows you to maintain individual counters for each `sender-address`. Without `count-by`, **all** users would share a daily limit of `1000000` gas units.

```yaml
access-controller:
  access-policy: deny-all
  rules:
    - sender-address: "*"
      gas-usage:
        value: '<1000000'
        window: 1 day
        count-by: [ sender-address ]
      action: 'allow'
```

## Learn More

For more information about how the rules are processed, please refer to [this link](https://docs.iota.org/operator/gas-station/architecture/features#access-controller).
