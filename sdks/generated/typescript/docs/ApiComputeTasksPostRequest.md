# ApiComputeTasksPostRequest

## Properties

| Name                     | Type                                                                                                    | Description          | Notes                             |
| ------------------------ | ------------------------------------------------------------------------------------------------------- | -------------------- | --------------------------------- |
| **taskType**             | **string**                                                                                              | Type of compute task | [default to undefined]            |
| **priority**             | **string**                                                                                              | Task priority level  | [optional] [default to undefined] |
| **resourceRequirements** | [**ApiComputeTasksPostRequestResourceRequirements**](ApiComputeTasksPostRequestResourceRequirements.md) |                      | [optional] [default to undefined] |

## Example

```typescript
import { ApiComputeTasksPostRequest } from '@wrfcoin/sdk-typescript';

const instance: ApiComputeTasksPostRequest = {
  taskType,
  priority,
  resourceRequirements,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
