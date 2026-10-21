# VoteResponse

## Properties

| Name                | Type       | Description           | Notes                             |
| ------------------- | ---------- | --------------------- | --------------------------------- |
| **transactionHash** | **string** | Vote transaction hash | [optional] [default to undefined] |
| **votingPower**     | **string** | Voting power used     | [optional] [default to undefined] |
| **timestamp**       | **number** | Vote timestamp        | [optional] [default to undefined] |

## Example

```typescript
import { VoteResponse } from '@wrfcoin/sdk-typescript';

const instance: VoteResponse = {
  transactionHash,
  votingPower,
  timestamp,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
