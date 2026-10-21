# HealthStatus

## Properties

| Name            | Type                                                                       | Description | Notes                             |
| --------------- | -------------------------------------------------------------------------- | ----------- | --------------------------------- |
| **status**      | **string**                                                                 |             | [optional] [default to undefined] |
| **timestamp**   | **string**                                                                 |             | [optional] [default to undefined] |
| **uptime**      | **number**                                                                 |             | [optional] [default to undefined] |
| **version**     | **string**                                                                 |             | [optional] [default to undefined] |
| **environment** | **string**                                                                 |             | [optional] [default to undefined] |
| **services**    | [**Array&lt;HealthStatusServicesInner&gt;**](HealthStatusServicesInner.md) |             | [optional] [default to undefined] |
| **database**    | [**HealthStatusDatabase**](HealthStatusDatabase.md)                        |             | [optional] [default to undefined] |
| **blockchain**  | [**HealthStatusBlockchain**](HealthStatusBlockchain.md)                    |             | [optional] [default to undefined] |
| **performance** | [**HealthStatusPerformance**](HealthStatusPerformance.md)                  |             | [optional] [default to undefined] |
| **security**    | [**HealthStatusSecurity**](HealthStatusSecurity.md)                        |             | [optional] [default to undefined] |

## Example

```typescript
import { HealthStatus } from '@wrfcoin/sdk-typescript';

const instance: HealthStatus = {
  status,
  timestamp,
  uptime,
  version,
  environment,
  services,
  database,
  blockchain,
  performance,
  security,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
