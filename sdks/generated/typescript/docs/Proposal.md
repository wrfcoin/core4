# Proposal

## Properties

| Name              | Type       | Description                         | Notes                             |
| ----------------- | ---------- | ----------------------------------- | --------------------------------- |
| **id**            | **string** | Proposal ID                         | [optional] [default to undefined] |
| **title**         | **string** | Proposal title                      | [optional] [default to undefined] |
| **description**   | **string** | Proposal description                | [optional] [default to undefined] |
| **proposer**      | **string** | Proposer address                    | [optional] [default to undefined] |
| **status**        | **string** |                                     | [optional] [default to undefined] |
| **votesFor**      | **string** | Total votes in favor                | [optional] [default to undefined] |
| **votesAgainst**  | **string** | Total votes against                 | [optional] [default to undefined] |
| **votesAbstain**  | **string** | Total abstain votes                 | [optional] [default to undefined] |
| **startTime**     | **number** | Voting start timestamp              | [optional] [default to undefined] |
| **endTime**       | **number** | Voting end timestamp                | [optional] [default to undefined] |
| **executionTime** | **number** | Execution timestamp (if applicable) | [optional] [default to undefined] |
| **quorum**        | **string** | Required quorum                     | [optional] [default to undefined] |
| **threshold**     | **string** | Approval threshold                  | [optional] [default to undefined] |

## Example

```typescript
import { Proposal } from '@wrfcoin/sdk-typescript';

const instance: Proposal = {
  id,
  title,
  description,
  proposer,
  status,
  votesFor,
  votesAgainst,
  votesAbstain,
  startTime,
  endTime,
  executionTime,
  quorum,
  threshold,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
