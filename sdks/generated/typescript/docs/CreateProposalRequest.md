# CreateProposalRequest

## Properties

| Name             | Type                    | Description                  | Notes                  |
| ---------------- | ----------------------- | ---------------------------- | ---------------------- |
| **title**        | **string**              | Proposal title               | [default to undefined] |
| **description**  | **string**              | Proposal description         | [default to undefined] |
| **actions**      | **Array&lt;object&gt;** | Actions to execute if passed | [default to undefined] |
| **votingPeriod** | **number**              | Voting period in seconds     | [default to undefined] |

## Example

```typescript
import { CreateProposalRequest } from '@wrfcoin/sdk-typescript';

const instance: CreateProposalRequest = {
  title,
  description,
  actions,
  votingPeriod,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
