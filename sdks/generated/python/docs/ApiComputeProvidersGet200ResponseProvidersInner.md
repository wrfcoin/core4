# ApiComputeProvidersGet200ResponseProvidersInner

## Properties

| Name               | Type                                                                                                                        | Description | Notes      |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| **name**           | **str**                                                                                                                     |             | [optional] |
| **status**         | **str**                                                                                                                     |             | [optional] |
| **availability**   | **float**                                                                                                                   |             | [optional] |
| **price_per_hour** | **str**                                                                                                                     |             | [optional] |
| **capabilities**   | **List[str]**                                                                                                               |             | [optional] |
| **resources**      | [**ApiComputeProvidersGet200ResponseProvidersInnerResources**](ApiComputeProvidersGet200ResponseProvidersInnerResources.md) |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_compute_providers_get200_response_providers_inner import ApiComputeProvidersGet200ResponseProvidersInner

# TODO update the JSON string below
json = "{}"
# create an instance of ApiComputeProvidersGet200ResponseProvidersInner from a JSON string
api_compute_providers_get200_response_providers_inner_instance = ApiComputeProvidersGet200ResponseProvidersInner.from_json(json)
# print the JSON string representation of the object
print(ApiComputeProvidersGet200ResponseProvidersInner.to_json())

# convert the object into a dict
api_compute_providers_get200_response_providers_inner_dict = api_compute_providers_get200_response_providers_inner_instance.to_dict()
# create an instance of ApiComputeProvidersGet200ResponseProvidersInner from a dict
api_compute_providers_get200_response_providers_inner_from_dict = ApiComputeProvidersGet200ResponseProvidersInner.from_dict(api_compute_providers_get200_response_providers_inner_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
