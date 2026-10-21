# HealthStatusPerformance

## Properties

| Name        | Type                                                                    | Description | Notes                             |
| ----------- | ----------------------------------------------------------------------- | ----------- | --------------------------------- |
| **cpu**     | [**HealthStatusPerformanceCpu**](HealthStatusPerformanceCpu.md)         |             | [optional] [default to undefined] |
| **memory**  | [**HealthStatusDatabaseStorage**](HealthStatusDatabaseStorage.md)       |             | [optional] [default to undefined] |
| **disk**    | [**HealthStatusDatabaseStorage**](HealthStatusDatabaseStorage.md)       |             | [optional] [default to undefined] |
| **network** | [**HealthStatusPerformanceNetwork**](HealthStatusPerformanceNetwork.md) |             | [optional] [default to undefined] |

## Example

```typescript
import { HealthStatusPerformance } from '@wrfcoin/sdk-typescript';

const instance: HealthStatusPerformance = {
  cpu,
  memory,
  disk,
  network,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
