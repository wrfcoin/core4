# ApiComputeProvidersGet200ResponseProvidersInnerResources

## Properties

| Name                 | Type     | Description | Notes      |
| -------------------- | -------- | ----------- | ---------- |
| **total_cpu**        | **int**  |             | [optional] |
| **available_cpu**    | **int**  |             | [optional] |
| **total_memory**     | **int**  |             | [optional] |
| **available_memory** | **int**  |             | [optional] |
| **gpu_support**      | **bool** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_compute_providers_get200_response_providers_inner_resources import ApiComputeProvidersGet200ResponseProvidersInnerResources

# TODO update the JSON string below
json = "{}"
# create an instance of ApiComputeProvidersGet200ResponseProvidersInnerResources from a JSON string
api_compute_providers_get200_response_providers_inner_resources_instance = ApiComputeProvidersGet200ResponseProvidersInnerResources.from_json(json)
# print the JSON string representation of the object
print(ApiComputeProvidersGet200ResponseProvidersInnerResources.to_json())

# convert the object into a dict
api_compute_providers_get200_response_providers_inner_resources_dict = api_compute_providers_get200_response_providers_inner_resources_instance.to_dict()
# create an instance of ApiComputeProvidersGet200ResponseProvidersInnerResources from a dict
api_compute_providers_get200_response_providers_inner_resources_from_dict = ApiComputeProvidersGet200ResponseProvidersInnerResources.from_dict(api_compute_providers_get200_response_providers_inner_resources_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
