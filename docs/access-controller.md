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

### Access Controller Rule syntax

|  parameter                  | mandatory  | possible values                                                |
|-----------------------------| -----------|----------------------------------------------------------------|
| `sender-address`            |  yes       | `'0x0000...'`, `[0x0000.., 0x1111...]`, `'*'`                  |
| `gas-budget`                |  no        | `'=100'`, `'<100'`,  `'<=100'`, `'>100'`, `'>=100'`, `'!=100'` |
| `move-call-package-address` |  no        | `'0x0000...'`, `[0x0000..., 0x1111...]`, `'*'`                 |
| `action`                    |  yes       | `'allow'`,  `'deny'`                                           |

## Learn More

For more information about how the rules are processed, please refer to [this link](https://docs.iota.org/operator/gas-station/architecture/features#access-controller).
