# ApiAimlModelsGet200ResponseModelsInner

## Properties

| Name             | Type                                                                                                            | Description | Notes      |
| ---------------- | --------------------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| **id**           | **str**                                                                                                         |             | [optional] |
| **name**         | **str**                                                                                                         |             | [optional] |
| **description**  | **str**                                                                                                         |             | [optional] |
| **category**     | **str**                                                                                                         |             | [optional] |
| **version**      | **str**                                                                                                         |             | [optional] |
| **requirements** | [**ApiAimlModelsGet200ResponseModelsInnerRequirements**](ApiAimlModelsGet200ResponseModelsInnerRequirements.md) |             | [optional] |
| **pricing**      | [**ApiAimlModelsGet200ResponseModelsInnerPricing**](ApiAimlModelsGet200ResponseModelsInnerPricing.md)           |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_aiml_models_get200_response_models_inner import ApiAimlModelsGet200ResponseModelsInner

# TODO update the JSON string below
json = "{}"
# create an instance of ApiAimlModelsGet200ResponseModelsInner from a JSON string
api_aiml_models_get200_response_models_inner_instance = ApiAimlModelsGet200ResponseModelsInner.from_json(json)
# print the JSON string representation of the object
print(ApiAimlModelsGet200ResponseModelsInner.to_json())

# convert the object into a dict
api_aiml_models_get200_response_models_inner_dict = api_aiml_models_get200_response_models_inner_instance.to_dict()
# create an instance of ApiAimlModelsGet200ResponseModelsInner from a dict
api_aiml_models_get200_response_models_inner_from_dict = ApiAimlModelsGet200ResponseModelsInner.from_dict(api_aiml_models_get200_response_models_inner_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
