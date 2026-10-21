# ApiComputeProvidersGet200Response

## Properties

| Name          | Type                                                                                                            | Description | Notes      |
| ------------- | --------------------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| **success**   | **bool**                                                                                                        |             | [optional] |
| **providers** | [**List[ApiComputeProvidersGet200ResponseProvidersInner]**](ApiComputeProvidersGet200ResponseProvidersInner.md) |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_compute_providers_get200_response import ApiComputeProvidersGet200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiComputeProvidersGet200Response from a JSON string
api_compute_providers_get200_response_instance = ApiComputeProvidersGet200Response.from_json(json)
# print the JSON string representation of the object
print(ApiComputeProvidersGet200Response.to_json())

# convert the object into a dict
api_compute_providers_get200_response_dict = api_compute_providers_get200_response_instance.to_dict()
# create an instance of ApiComputeProvidersGet200Response from a dict
api_compute_providers_get200_response_from_dict = ApiComputeProvidersGet200Response.from_dict(api_compute_providers_get200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
