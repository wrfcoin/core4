# ApiPerformanceMetricsGet200ResponseMetrics

## Properties

| Name           | Type                                                                                                                | Description             | Notes                             |
| -------------- | ------------------------------------------------------------------------------------------------------------------- | ----------------------- | --------------------------------- |
| **tps**        | **number**                                                                                                          | Transactions per second | [optional] [default to undefined] |
| **latency**    | [**ApiPerformanceMetricsGet200ResponseMetricsLatency**](ApiPerformanceMetricsGet200ResponseMetricsLatency.md)       |                         | [optional] [default to undefined] |
| **throughput** | [**ApiPerformanceMetricsGet200ResponseMetricsThroughput**](ApiPerformanceMetricsGet200ResponseMetricsThroughput.md) |                         | [optional] [default to undefined] |
| **errorRate**  | **number**                                                                                                          |                         | [optional] [default to undefined] |
| **uptime**     | **number**                                                                                                          |                         | [optional] [default to undefined] |

## Example

```typescript
import { ApiPerformanceMetricsGet200ResponseMetrics } from '@wrfcoin/sdk-typescript';

const instance: ApiPerformanceMetricsGet200ResponseMetrics = {
  tps,
  latency,
  throughput,
  errorRate,
  uptime,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
