# Gas Station Server Access Controller

The **Gas Station Server** includes an **Access Controller** mechanism to manage access to the `/execute_tx` endpoint. This feature allows you to implement filtering logic based on properties derived from transactions. Currently, the Access Controller supports filtering based on the sender's address, enabling you to block or allow specific addresses.

## Access Controller Rule syntax

|  parameter                  | mandatory  | possible values                                                |
|-----------------------------| -----------|----------------------------------------------------------------|
| `sender-address`            |  yes       | `'0x0000...'`, `[0x0000.., 0x1111...]`, `'*'`                  |
| `gas-budget`                |  no        | `'=100'`, `'<100'`,  `'<=100'`, `'>100'`, `'>=100'`, `'!=100'` |
| `move-call-package-address` |  no        | `'0x0000...'`, `[0x0000..., 0x1111...]`, `'*'`                 |
| `ptb-command-count`         |  no        | `'=10'`, `'<10'`,  `'<=10'`, `'>10'`, `'>=10'`, `'!=10'`       |
| `action`                    |  yes       | `'allow'`, `'deny'`, [Hook Server URL](#hook-server)           |
| `gas_usage`                 |  no        | See [Gas Usage Filter](#gas-usage-filter)                      |
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
           action: allow # allowed actions: 'allow', 'deny', a hook url (see "Hook Server" section)
   ```

---

- Enables All Requests and Deny Only a Specific Address

   The following configuration allows all incoming transactions except those from the specified sender address (`0x0101...`):

   ```yaml
   access-controller:
      access-policy: deny-all
      rules:
         - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
           action: deny
   ```

---

- Gas Budget Constraints

   The following configuration denies all incoming transactions except those from the specified sender address (`0x0101...`) and the transaction gas budget below the limit `1000000`

   ```yaml
   access-controller:
      access-policy: deny-all
      rules:
         - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
           transaction-gas-budget: <1000000 # allowed operators: =, !=, <, >, <=, >=
           action: allow
   ```

---

- Advanced budgeting management

   The following configuration accept all incoming transactions with gas budget below `500000`. For address sender address (`0x0101...`) the allowed gas budget is increased to `1000000`

   ```yaml
   access-controller:
      access-policy: deny-all
      rules:
         - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
           transaction-gas-budget: <=10000000
           action: allow

         - sender-address: '*'
           transaction-gas-budget: <500000
           action: allow
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
           action: allow
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
                5,
                104,
                101,
                108,
                108,
                111
              ]
            }
          ],
          "commands": [
            {
              "MoveCall": {
                "package": "0xb674e2ed79db3c25fa4c00d5c7d62a9c18089e1fc4c2de5b5ee8b2836a85ae26",
                "module": "allowed_module_name",
                "function": "allowed_module_function",
                "type_arguments": [],
                "arguments": [
                  {
                    "Input": 0
                  }
                ]
              }
            }
          ]
        }
      },
      "sender": "0xa2e17e20f97355af6491580ff5c11ecefcdcf76ea224d163e5cb92389adf2311",
      "gas_data": {
        "payment": [
          [
            "0x1369ab7cca1c229d093060d66666db7c0db1edf43310d5e56acec2dafa492ffb",
            9521034,
            "9knpRtwCc9LK1BCJ222KZwgr9ZVHDyZhiLUTPt82FtmV"
          ],
          [
            "0x1932fbdf314cb263f4d2a00656144e0951edd99fe5489b07b799b7087f8de20d",
            9521055,
            "BAubkJuEwD1PRGL1CLBRjv3GCP83p93J7ZAdSsdVtifk"
          ],
          [
            "0x19d732e1b95c8b6af724ba026f8f61e4b72b9931777a0e1d0b8d3054de1b2ef4",
            9521050,
            "GRyLJcRSepVk3r9Fjdmv9MxauX6mrzYVptAhPwzVRu61"
          ],
          [
            "0x1a5f7fc0f977a3020f8c5bd30c635fadab9b1ea0003e8255d90ec9ee48ff09ea",
            9521042,
            "BbrFR6tykrnKzskVJRcBJbWfDRcCeBqMweotE4bXVXLo"
          ],
          [
            "0x1d02df222b5149497a4d722514bf33a8bf755011724cdd3ced7ecb174d833690",
            9521046,
            "DcwkhBh2sdw8yJW1SH4tv5mVq6Ec7z4EmAaRL2H3AS27"
          ],
          [
            "0x1d8bf3742256d434ae9f1305eab214c5af458539c293ee51a25af8826b692589",
            9521046,
            "GG9mzv9JpXvAMeAC2FWPMXSqWqyTJiqeFepwLAzEpKgL"
          ],
          [
            "0x1e878332e7fa2e6fa6120a395bc8ad207ba5f5fa7cc8359ae4b56da20b50ff54",
            9521056,
            "GrK2met5oxjdVP3qJuXtNAs7hkajFFUwbe512LVxBBmW"
          ],
          [
            "0x1ed6d2105135371d901d24e4acd6fd9025b24d4d3d75277f2ad059825a5e5b38",
            9521040,
            "DJurdiKP82LVADxNrxsYnYJW9aCcDP1bxLbNbdcvdMHV"
          ],
          [
            "0x1f551af1739e258b5ed17c48e348c494de7598dc2a42aa2d5c0f284589de61e7",
            9521037,
            "4rpZtAY2eC2YyAJiDcDfq1BbCnV4sPKQQBU9rGCyTpmD"
          ],
          [
            "0x1fcf200bc9b970c9c877ddadddf28cdff19165c246914444b2968c7287587e8a",
            9521056,
            "5DDvPhXHRzmjdT9FRNqYe5T5LwkBpov2XKRpnNevhMJf"
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

### Rego Filtering Code Example

The payload above is evaluated against a Rego expression. Suppose we want to ensure that only specific move-call can be sponsored by the Gas Station. To enforce this, we can write the following expression:

1. **Extract the Commands Array:**
   Retrieve the array of commands from the payload.

2. **Restrict to a Single Command:**
   Ensure that only one command is allowed to prevent piggybacking, where the sender might attach an additional unauthorized move-call to another contract.

3. **Verify Expected Fields:**
   Confirm that the package, module, and function fields match the expected values.

```go
package matchers

default move_call_matches = false

move_call_matches if {
    cmds := input.transaction_data.V1.kind.ProgrammableTransaction.commands
    count(cmds) == 1

    mc := cmds[0].MoveCall
    mc["package"]  == "0xb674e2ed79db3c25fa4c00d5c7d62a9c18089e1fc4c2de5b5ee8b2836a85ae26"
    mc.module   == "allowed_module_name"
    mc.function == "allowed_function_name"
}
```

> **Note:** All field addresses in the Rego expression should begin with `input`.

> **Note:** For full Rego syntax details please see the [Reference](https://www.openpolicyagent.org/docs/policy-language).

### Rego Expression Sources

The Rego expressions may come from different sources:

- **File:** Example configuration to load a rule from a file.
- **Redis:** Example configuration loading a rule from Redis.
- **HTTP:** Example configuration loading a rule via HTTP.

#### Rego from File

```yaml
access-controller:
  access-policy: deny-all
  rules:
    - sender-address: "*"
      rego-expression:
        location-type: file
        path: "./source_file.rego"
        rego-rule-path: data.matchers.move_call_matches
      action: allow
```

#### Rego from Redis

```yaml
access-controller:
  access-policy: allow-all
  rules:
    - sender-address: "*"
    rego-expression:
        location-type: redis
        url: "redis://localhost"
        redis-key: source.rego
        rego-rule-path: data.matchers.move_call_matches
      action: allow
```

#### Rego from HTTP

```yaml
access-controller:
  access-policy: allow-all
  rules:
    - sender-address: "*"
      rego-expression:
        location-type: http
        url: "http://localhost:8080/source.rego"
        rego-rule-path: data.matchers.move_call_matches
      action: deny
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
        window: 1 day
        value: ">1000000"
      action: deny

    - sender-address: '*'
      action: allow
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
        value: <1000000
        window: 1 day
      action: allow
```

---

**3. Limit Gas Usage per Address**

In this example, each user is limited to `10000000` gas units per day. The additional property `count-by` allows you to maintain individual counters for each `sender-address`. Without `count-by`, **all** users would share a daily limit of `1000000` gas units.

```yaml
access-controller:
  access-policy: deny-all
  rules:
    - sender-address: "*"
      gas-usage:
        value: <1000000
        window: 1 day
        count-by: [ sender-address ]
      action: allow
```

## Hook Server

An external server (a hook), that decides whether a transaction should be executed or not can be configured. The hook receives the same input as the gas station allowing to parse inspect the transaction the same way, as the gas station does.

Hook server(s) can be configured as a term in the access controller rules, allowing to integrate hooks into existing rule sets or replacing the gas station built in access controller by a using hook only configuration.

Hooks are configured as values for the "action" keyword, by setting the `action` value to a URL instead of `allow`/`deny`. 

Hooks are the last thing that is called in an access controller rule (just before the gas usage check due to safety reasons). This reduces the amount of calls against a hook server and leads to a few possible scenarios as shown below.

```mermaid
flowchart TD
    Start(rule with hook<br>is processed)
    CallHook(call<br>hook)
    IgnoreHook(ignore<br>hook)
    AllowTx(allow tx)
    DenyTx(deny tx)
    CheckNextRule(check<br>next<br>rule)
    CheckPreviousTerm{previous<br>terms<br>apply}
    CheckResponse{process<br>response}

    Start --> CheckPreviousTerms
    CheckPreviousTerms -->|yes| CallHook
    CheckPreviousTerms -->|no| IgnoreHook

    IgnoreHook --> CheckNextRule

    CallHook --> CheckResponse
    CheckResponse -->|allow| AllowTx
    CheckResponse -->|deny| DenyTx
    CheckResponse -->|noDecision| CheckNextRule
```

A hook server has to follow the api spec defined [here](./hook-openapi.json). Also an example server that can be used as a starting point for an own hook can be found in our [examples](../examples/hook).

---

- Hook only configuration

Having a single rule with a hook action replaces the access controller completely:

```yml
access-controller:
  access-policy: deny-all
  rules:
    - sender-address: "*"
      action: http://127.0.0.1:8080
```

or even shorter:

```yml
access-controller:
  access-policy: deny-all
  rules:
    - action: http://127.0.0.1:8080
```

---

As you usually might want to reduce the number of calls against the hook a bit, you can already apply access controller logic _before_ the hook is called (all other terms except `gas-usage` are checked before the hook call). To do so, add terms as documented above, for example:

```yml
   access-controller:
      access-policy: deny-all
      rules:
        - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
          transaction-gas-budget: '<1000000' # allowed operators: =, !=, <, >, <=, >=
          action: http://127.0.0.1:8080
```

---

Hook actions don't have to be used as standalone rules and can integrate seamlessly with other rules, e.g.

```yml
   access-controller:
      access-policy: deny-all
      rules:
        - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
          transaction-gas-budget: '<1000000'
          action: allow
        - action: http://127.0.0.1:8080
        - sender-address: "*"
          gas-usage:
            value: '<1000000'
            window: 1 day
            count-by: [ sender-address ]
```

As this might look a bit confusing, let's break this one down:

- we have a privileged address `0x0101010101010101010101010101010101010101010101010101010101010101`, that can send transaction below a certain threshold
- other addresses or larger transactions by the privileged address have to go through a hook check
- the hook can then react with:
  - allowing the transaction
  - denying the transaction
  - letting the next rule decide if the transaction should be executed or not
- assuming, the hook decides not to decide about the transaction, we would now check the sender address based gas usage and decide based on this if the transaction is executed or not

## Learn More

For more information about how the rules are processed, please refer to [this link](https://docs.iota.org/operator/gas-station/architecture/features#access-controller).
