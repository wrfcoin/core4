# ApiIntegrationsLangchainStatusGet200Response

## Properties

| Name          | Type                                      | Description | Notes      |
| ------------- | ----------------------------------------- | ----------- | ---------- |
| **success**   | **bool**                                  |             | [optional] |
| **data**      | [**LangChainStatus**](LangChainStatus.md) |             | [optional] |
| **timestamp** | **datetime**                              |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_integrations_langchain_status_get200_response import ApiIntegrationsLangchainStatusGet200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiIntegrationsLangchainStatusGet200Response from a JSON string
api_integrations_langchain_status_get200_response_instance = ApiIntegrationsLangchainStatusGet200Response.from_json(json)
# print the JSON string representation of the object
print(ApiIntegrationsLangchainStatusGet200Response.to_json())

# convert the object into a dict
api_integrations_langchain_status_get200_response_dict = api_integrations_langchain_status_get200_response_instance.to_dict()
# create an instance of ApiIntegrationsLangchainStatusGet200Response from a dict
api_integrations_langchain_status_get200_response_from_dict = ApiIntegrationsLangchainStatusGet200Response.from_dict(api_integrations_langchain_status_get200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
