# ApiComputeTasksPostRequestResourceRequirements

## Properties

| Name        | Type        | Description        | Notes                             |
| ----------- | ----------- | ------------------ | --------------------------------- |
| **cpu**     | **number**  | CPU cores required | [optional] [default to undefined] |
| **memory**  | **number**  | Memory in GB       | [optional] [default to undefined] |
| **storage** | **number**  | Storage in GB      | [optional] [default to undefined] |
| **gpu**     | **boolean** | GPU requirement    | [optional] [default to undefined] |

## Example

```typescript
import { ApiComputeTasksPostRequestResourceRequirements } from '@wrfcoin/sdk-typescript';

const instance: ApiComputeTasksPostRequestResourceRequirements = {
  cpu,
  memory,
  storage,
  gpu,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
