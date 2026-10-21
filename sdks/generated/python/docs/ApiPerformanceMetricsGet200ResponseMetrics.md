# ApiPerformanceMetricsGet200ResponseMetrics

## Properties

| Name           | Type                                                                                                                | Description             | Notes      |
| -------------- | ------------------------------------------------------------------------------------------------------------------- | ----------------------- | ---------- |
| **tps**        | **float**                                                                                                           | Transactions per second | [optional] |
| **latency**    | [**ApiPerformanceMetricsGet200ResponseMetricsLatency**](ApiPerformanceMetricsGet200ResponseMetricsLatency.md)       |                         | [optional] |
| **throughput** | [**ApiPerformanceMetricsGet200ResponseMetricsThroughput**](ApiPerformanceMetricsGet200ResponseMetricsThroughput.md) |                         | [optional] |
| **error_rate** | **float**                                                                                                           |                         | [optional] |
| **uptime**     | **float**                                                                                                           |                         | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_performance_metrics_get200_response_metrics import ApiPerformanceMetricsGet200ResponseMetrics

# TODO update the JSON string below
json = "{}"
# create an instance of ApiPerformanceMetricsGet200ResponseMetrics from a JSON string
api_performance_metrics_get200_response_metrics_instance = ApiPerformanceMetricsGet200ResponseMetrics.from_json(json)
# print the JSON string representation of the object
print(ApiPerformanceMetricsGet200ResponseMetrics.to_json())

# convert the object into a dict
api_performance_metrics_get200_response_metrics_dict = api_performance_metrics_get200_response_metrics_instance.to_dict()
# create an instance of ApiPerformanceMetricsGet200ResponseMetrics from a dict
api_performance_metrics_get200_response_metrics_from_dict = ApiPerformanceMetricsGet200ResponseMetrics.from_dict(api_performance_metrics_get200_response_metrics_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
