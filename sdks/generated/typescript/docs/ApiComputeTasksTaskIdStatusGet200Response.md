# ApiComputeTasksTaskIdStatusGet200Response

## Properties

| Name                    | Type                                                                                                                    | Description | Notes                             |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------- | ----------- | --------------------------------- |
| **taskId**              | **string**                                                                                                              |             | [optional] [default to undefined] |
| **status**              | **string**                                                                                                              |             | [optional] [default to undefined] |
| **progress**            | **number**                                                                                                              |             | [optional] [default to undefined] |
| **provider**            | **string**                                                                                                              |             | [optional] [default to undefined] |
| **estimatedCompletion** | **string**                                                                                                              |             | [optional] [default to undefined] |
| **resourceUsage**       | [**ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage**](ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage.md) |             | [optional] [default to undefined] |
| **cost**                | **string**                                                                                                              |             | [optional] [default to undefined] |

## Example

```typescript
import { ApiComputeTasksTaskIdStatusGet200Response } from '@wrfcoin/sdk-typescript';

const instance: ApiComputeTasksTaskIdStatusGet200Response = {
  taskId,
  status,
  progress,
  provider,
  estimatedCompletion,
  resourceUsage,
  cost,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
