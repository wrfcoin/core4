# ApiAimlInferencePostRequest

## Properties

| Name           | Type       | Description              | Notes      |
| -------------- | ---------- | ------------------------ | ---------- |
| **model_id**   | **str**    | Model identifier         |
| **input**      | **object** | Input data for inference |
| **parameters** | **object** | Model parameters         | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_aiml_inference_post_request import ApiAimlInferencePostRequest

# TODO update the JSON string below
json = "{}"
# create an instance of ApiAimlInferencePostRequest from a JSON string
api_aiml_inference_post_request_instance = ApiAimlInferencePostRequest.from_json(json)
# print the JSON string representation of the object
print(ApiAimlInferencePostRequest.to_json())

# convert the object into a dict
api_aiml_inference_post_request_dict = api_aiml_inference_post_request_instance.to_dict()
# create an instance of ApiAimlInferencePostRequest from a dict
api_aiml_inference_post_request_from_dict = ApiAimlInferencePostRequest.from_dict(api_aiml_inference_post_request_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
