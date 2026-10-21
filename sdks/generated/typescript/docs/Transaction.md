# Transaction

## Properties

| Name                 | Type       | Description                       | Notes                             |
| -------------------- | ---------- | --------------------------------- | --------------------------------- |
| **hash**             | **string** | Transaction hash                  | [optional] [default to undefined] |
| **from**             | **string** | Sender address                    | [optional] [default to undefined] |
| **to**               | **string** | Recipient address                 | [optional] [default to undefined] |
| **value**            | **string** | Transaction value in wei          | [optional] [default to undefined] |
| **gas**              | **number** | Gas limit                         | [optional] [default to undefined] |
| **gasPrice**         | **string** | Gas price in wei                  | [optional] [default to undefined] |
| **gasUsed**          | **number** | Gas used                          | [optional] [default to undefined] |
| **nonce**            | **number** | Transaction nonce                 | [optional] [default to undefined] |
| **blockHash**        | **string** | Block hash containing transaction | [optional] [default to undefined] |
| **blockNumber**      | **number** | Block number                      | [optional] [default to undefined] |
| **transactionIndex** | **number** | Transaction index in block        | [optional] [default to undefined] |
| **status**           | **string** |                                   | [optional] [default to undefined] |
| **timestamp**        | **number** | Transaction timestamp             | [optional] [default to undefined] |

## Example

```typescript
import { Transaction } from '@wrfcoin/sdk-typescript';

const instance: Transaction = {
  hash,
  from,
  to,
  value,
  gas,
  gasPrice,
  gasUsed,
  nonce,
  blockHash,
  blockNumber,
  transactionIndex,
  status,
  timestamp,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
