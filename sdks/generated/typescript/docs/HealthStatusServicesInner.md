# HealthStatusServicesInner

## Properties

| Name             | Type                    | Description | Notes                             |
| ---------------- | ----------------------- | ----------- | --------------------------------- |
| **name**         | **string**              |             | [optional] [default to undefined] |
| **status**       | **string**              |             | [optional] [default to undefined] |
| **responseTime** | **number**              |             | [optional] [default to undefined] |
| **errorRate**    | **number**              |             | [optional] [default to undefined] |
| **lastCheck**    | **string**              |             | [optional] [default to undefined] |
| **dependencies** | **Array&lt;string&gt;** |             | [optional] [default to undefined] |

## Example

```typescript
import { HealthStatusServicesInner } from '@wrfcoin/sdk-typescript';

const instance: HealthStatusServicesInner = {
  name,
  status,
  responseTime,
  errorRate,
  lastCheck,
  dependencies,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
