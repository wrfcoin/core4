# VoteRequest

## Properties

| Name            | Type       | Description            | Notes                             |
| --------------- | ---------- | ---------------------- | --------------------------------- |
| **vote**        | **string** |                        | [default to undefined]            |
| **votingPower** | **string** | Voting power to use    | [optional] [default to undefined] |
| **reason**      | **string** | Optional voting reason | [optional] [default to undefined] |

## Example

```typescript
import { VoteRequest } from '@wrfcoin/sdk-typescript';

const instance: VoteRequest = {
  vote,
  votingPower,
  reason,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
