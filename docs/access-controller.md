# Gas Station Server Access Controller

The **Gas Station Server** includes an **Access Controller** mechanism to manage access to the `/execute_tx` endpoint. This feature allows you to implement filtering logic based on properties derived from transactions. Currently, the Access Controller supports filtering based on the sender's address, enabling you to block or allow specific addresses.

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

### Access Controller Rule syntax

|  parameter                  | mandatory  | possible values                                                |
|-----------------------------| -----------|----------------------------------------------------------------|
| `sender-address`            |  yes       | `'0x0000...'`, `[0x0000.., 0x1111...]`, `'*'`                  |
| `gas-budget`                |  no        | `'=100'`, `'<100'`,  `'<=100'`, `'>100'`, `'>=100'`, `'!=100'` |
| `move-call-package-address` |  no        | `'0x0000...'`, `[0x0000..., 0x1111...]`, `'*'`                 |
| `ptb-command-count`         |  no        | `'=10'`, `'<10'`,  `'<=10'`, `'>10'`, `'>=10'`, `'!=10'`       |
| `action`                    |  yes       | `'allow'`,  `'deny'`                                           |
| `gas_usage`                 |  no        | See [Gas Usage Limit](#gas-usage-limit-feature)                |

Below is a revised version of the documentation with improved grammar and clarity:

---

### Gas Usage Limit Feature

The **Gas Usage Limit** feature enables you to track gas consumption based on predefined parameters. When enabled, the gas tracking applies to the entire rule. The configuration syntax is:

```yaml
gas_usage:
  limit: [range_of_numbers]
  duration: [duration]
```

> **Note:** The syntax of `duration` follows the specification used in the [`humantime`](https://docs.rs/humantime/latest/humantime/index.html) crate

#### Gas Usage Examples

Below are two examples that demonstrate how to enforce gas usage limits.

---

**1. Limit Gas Usage per Address**

This configuration sets a daily gas usage limit for a specific address. In the example below, the sender at address `0x0101...` is restricted to a maximum daily usage of `10000000` gas units. Note that gas usage for other addresses remains unconstrained.

```yaml
access-controller:
  access-policy: deny-all
  rules:
    - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
      move-call-package-address: "0x0202020202020202020202020202020202020202020202020202020202020202"
      gas-usage:
        limit: '<1000000'
        duration: 1 day
      action: 'allow'
```

---

**2. Limit Gas Usage per Address and Module**

In this example, the configuration restricts the daily gas usage for a specific address when calling a designated module. The sender at address `0x0101...` is limited to a maximum daily usage of `10000000` gas units when accessing package `0x0202...`. For all other interactions, gas usage is blocked.

```yaml
access-controller:
access-policy: deny-all
rules:
  - sender-address: "0x0101010101010101010101010101010101010101010101010101010101010101"
    gas-usage:
      limit: '>1000000'
      duration: 1 day
    action: 'deny'

  - sender-address: '*'
    action: 'allow'
```

## Learn More

For more information about how the rules are processed, please refer to [this link](https://docs.iota.org/operator/gas-station/architecture/features#access-controller).
