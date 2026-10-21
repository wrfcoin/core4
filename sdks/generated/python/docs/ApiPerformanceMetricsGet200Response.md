# ApiPerformanceMetricsGet200Response

## Properties

| Name          | Type                                                                                            | Description | Notes      |
| ------------- | ----------------------------------------------------------------------------------------------- | ----------- | ---------- |
| **success**   | **bool**                                                                                        |             | [optional] |
| **metrics**   | [**ApiPerformanceMetricsGet200ResponseMetrics**](ApiPerformanceMetricsGet200ResponseMetrics.md) |             | [optional] |
| **timestamp** | **datetime**                                                                                    |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_performance_metrics_get200_response import ApiPerformanceMetricsGet200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiPerformanceMetricsGet200Response from a JSON string
api_performance_metrics_get200_response_instance = ApiPerformanceMetricsGet200Response.from_json(json)
# print the JSON string representation of the object
print(ApiPerformanceMetricsGet200Response.to_json())

# convert the object into a dict
api_performance_metrics_get200_response_dict = api_performance_metrics_get200_response_instance.to_dict()
# create an instance of ApiPerformanceMetricsGet200Response from a dict
api_performance_metrics_get200_response_from_dict = ApiPerformanceMetricsGet200Response.from_dict(api_performance_metrics_get200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
