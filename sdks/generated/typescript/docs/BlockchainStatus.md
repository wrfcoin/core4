# BlockchainStatus

## Properties

| Name                | Type       | Description                   | Notes                             |
| ------------------- | ---------- | ----------------------------- | --------------------------------- |
| **blockHeight**     | **number** | Current block height          | [optional] [default to undefined] |
| **lastBlockHash**   | **string** | Hash of the most recent block | [optional] [default to undefined] |
| **networkHashRate** | **number** | Network hash rate in H/s      | [optional] [default to undefined] |
| **difficulty**      | **number** | Current mining difficulty     | [optional] [default to undefined] |
| **consensusState**  | **string** |                               | [optional] [default to undefined] |
| **peerCount**       | **number** | Number of connected peers     | [optional] [default to undefined] |
| **syncStatus**      | **string** |                               | [optional] [default to undefined] |
| **uptimeSeconds**   | **number** | Node uptime in seconds        | [optional] [default to undefined] |

## Example

```typescript
import { BlockchainStatus } from '@wrfcoin/sdk-typescript';

const instance: BlockchainStatus = {
  blockHeight,
  lastBlockHash,
  networkHashRate,
  difficulty,
  consensusState,
  peerCount,
  syncStatus,
  uptimeSeconds,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
