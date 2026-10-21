# HealthStatusDatabase

## Properties

| Name                 | Type                                                                                | Description | Notes                             |
| -------------------- | ----------------------------------------------------------------------------------- | ----------- | --------------------------------- |
| **status**           | **string**                                                                          |             | [optional] [default to undefined] |
| **connectionPool**   | [**HealthStatusDatabaseConnectionPool**](HealthStatusDatabaseConnectionPool.md)     |             | [optional] [default to undefined] |
| **queryPerformance** | [**HealthStatusDatabaseQueryPerformance**](HealthStatusDatabaseQueryPerformance.md) |             | [optional] [default to undefined] |
| **storage**          | [**HealthStatusDatabaseStorage**](HealthStatusDatabaseStorage.md)                   |             | [optional] [default to undefined] |

## Example

```typescript
import { HealthStatusDatabase } from '@wrfcoin/sdk-typescript';

const instance: HealthStatusDatabase = {
  status,
  connectionPool,
  queryPerformance,
  storage,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
