# ApiPerformanceOptimizationPostRequest

## Properties

| Name         | Type    | Description                      | Notes      |
| ------------ | ------- | -------------------------------- | ---------- |
| **target**   | **str** |                                  | [optional] |
| **level**    | **str** |                                  | [optional] |
| **duration** | **int** | Optimization duration in minutes | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_performance_optimization_post_request import ApiPerformanceOptimizationPostRequest

# TODO update the JSON string below
json = "{}"
# create an instance of ApiPerformanceOptimizationPostRequest from a JSON string
api_performance_optimization_post_request_instance = ApiPerformanceOptimizationPostRequest.from_json(json)
# print the JSON string representation of the object
print(ApiPerformanceOptimizationPostRequest.to_json())

# convert the object into a dict
api_performance_optimization_post_request_dict = api_performance_optimization_post_request_instance.to_dict()
# create an instance of ApiPerformanceOptimizationPostRequest from a dict
api_performance_optimization_post_request_from_dict = ApiPerformanceOptimizationPostRequest.from_dict(api_performance_optimization_post_request_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
