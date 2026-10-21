# ApiPerformanceOptimizationPost200Response

## Properties

| Name                      | Type                                                                                                                                  | Description | Notes      |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| **success**               | **bool**                                                                                                                              |             | [optional] |
| **optimization_id**       | **str**                                                                                                                               |             | [optional] |
| **status**                | **str**                                                                                                                               |             | [optional] |
| **estimated_duration**    | **str**                                                                                                                               |             | [optional] |
| **expected_improvements** | [**ApiPerformanceOptimizationPost200ResponseExpectedImprovements**](ApiPerformanceOptimizationPost200ResponseExpectedImprovements.md) |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_performance_optimization_post200_response import ApiPerformanceOptimizationPost200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiPerformanceOptimizationPost200Response from a JSON string
api_performance_optimization_post200_response_instance = ApiPerformanceOptimizationPost200Response.from_json(json)
# print the JSON string representation of the object
print(ApiPerformanceOptimizationPost200Response.to_json())

# convert the object into a dict
api_performance_optimization_post200_response_dict = api_performance_optimization_post200_response_instance.to_dict()
# create an instance of ApiPerformanceOptimizationPost200Response from a dict
api_performance_optimization_post200_response_from_dict = ApiPerformanceOptimizationPost200Response.from_dict(api_performance_optimization_post200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
