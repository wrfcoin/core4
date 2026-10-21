# ApiAimlModelsGet200Response

## Properties

| Name        | Type                                                                                          | Description | Notes      |
| ----------- | --------------------------------------------------------------------------------------------- | ----------- | ---------- |
| **success** | **bool**                                                                                      |             | [optional] |
| **models**  | [**List[ApiAimlModelsGet200ResponseModelsInner]**](ApiAimlModelsGet200ResponseModelsInner.md) |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_aiml_models_get200_response import ApiAimlModelsGet200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiAimlModelsGet200Response from a JSON string
api_aiml_models_get200_response_instance = ApiAimlModelsGet200Response.from_json(json)
# print the JSON string representation of the object
print(ApiAimlModelsGet200Response.to_json())

# convert the object into a dict
api_aiml_models_get200_response_dict = api_aiml_models_get200_response_instance.to_dict()
# create an instance of ApiAimlModelsGet200Response from a dict
api_aiml_models_get200_response_from_dict = ApiAimlModelsGet200Response.from_dict(api_aiml_models_get200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
