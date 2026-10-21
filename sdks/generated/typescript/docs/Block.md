# Block

## Properties

| Name                 | Type       | Description                     | Notes                             |
| -------------------- | ---------- | ------------------------------- | --------------------------------- |
| **hash**             | **string** | Block hash                      | [optional] [default to undefined] |
| **index**            | **number** | Block number                    | [optional] [default to undefined] |
| **previousHash**     | **string** | Previous block hash             | [optional] [default to undefined] |
| **timestamp**        | **number** | Block timestamp                 | [optional] [default to undefined] |
| **merkleRoot**       | **string** | Merkle root of transactions     | [optional] [default to undefined] |
| **nonce**            | **number** | Mining nonce                    | [optional] [default to undefined] |
| **difficulty**       | **number** | Block difficulty                | [optional] [default to undefined] |
| **transactionCount** | **number** | Number of transactions in block | [optional] [default to undefined] |
| **size**             | **number** | Block size in bytes             | [optional] [default to undefined] |
| **gasUsed**          | **number** | Total gas used in block         | [optional] [default to undefined] |
| **gasLimit**         | **number** | Block gas limit                 | [optional] [default to undefined] |

## Example

```typescript
import { Block } from '@wrfcoin/sdk-typescript';

const instance: Block = {
  hash,
  index,
  previousHash,
  timestamp,
  merkleRoot,
  nonce,
  difficulty,
  transactionCount,
  size,
  gasUsed,
  gasLimit,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
