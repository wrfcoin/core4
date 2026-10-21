# ApiAimlInferencePost200Response

## Properties

| Name                | Type       | Description      | Notes      |
| ------------------- | ---------- | ---------------- | ---------- |
| **success**         | **bool**   |                  | [optional] |
| **job_id**          | **str**    |                  | [optional] |
| **result**          | **object** | Inference result | [optional] |
| **processing_time** | **float**  |                  | [optional] |
| **cost**            | **str**    |                  | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_aiml_inference_post200_response import ApiAimlInferencePost200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiAimlInferencePost200Response from a JSON string
api_aiml_inference_post200_response_instance = ApiAimlInferencePost200Response.from_json(json)
# print the JSON string representation of the object
print(ApiAimlInferencePost200Response.to_json())

# convert the object into a dict
api_aiml_inference_post200_response_dict = api_aiml_inference_post200_response_instance.to_dict()
# create an instance of ApiAimlInferencePost200Response from a dict
api_aiml_inference_post200_response_from_dict = ApiAimlInferencePost200Response.from_dict(api_aiml_inference_post200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
