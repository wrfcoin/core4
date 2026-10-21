# ApiPerformanceOptimizationPost200Response

## Properties

| Name                     | Type                                                                                                                                  | Description | Notes                             |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------- | ----------- | --------------------------------- |
| **success**              | **boolean**                                                                                                                           |             | [optional] [default to undefined] |
| **optimizationId**       | **string**                                                                                                                            |             | [optional] [default to undefined] |
| **status**               | **string**                                                                                                                            |             | [optional] [default to undefined] |
| **estimatedDuration**    | **string**                                                                                                                            |             | [optional] [default to undefined] |
| **expectedImprovements** | [**ApiPerformanceOptimizationPost200ResponseExpectedImprovements**](ApiPerformanceOptimizationPost200ResponseExpectedImprovements.md) |             | [optional] [default to undefined] |

## Example

```typescript
import { ApiPerformanceOptimizationPost200Response } from '@wrfcoin/sdk-typescript';

const instance: ApiPerformanceOptimizationPost200Response = {
  success,
  optimizationId,
  status,
  estimatedDuration,
  expectedImprovements,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
