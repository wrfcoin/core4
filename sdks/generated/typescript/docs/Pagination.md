# Pagination

## Properties

| Name            | Type        | Description                      | Notes                             |
| --------------- | ----------- | -------------------------------- | --------------------------------- |
| **limit**       | **number**  | Items per page                   | [optional] [default to undefined] |
| **offset**      | **number**  | Items to skip                    | [optional] [default to undefined] |
| **total**       | **number**  | Total number of items            | [optional] [default to undefined] |
| **hasNext**     | **boolean** | Whether there are more items     | [optional] [default to undefined] |
| **hasPrevious** | **boolean** | Whether there are previous items | [optional] [default to undefined] |

## Example

```typescript
import { Pagination } from '@wrfcoin/sdk-typescript';

const instance: Pagination = {
  limit,
  offset,
  total,
  hasNext,
  hasPrevious,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
