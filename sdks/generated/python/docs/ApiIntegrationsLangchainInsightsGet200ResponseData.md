# ApiIntegrationsLangchainInsightsGet200ResponseData

## Properties

| Name            | Type                                              | Description | Notes      |
| --------------- | ------------------------------------------------- | ----------- | ---------- |
| **insights**    | [**List[LangChainInsight]**](LangChainInsight.md) |             | [optional] |
| **timestamp**   | **datetime**                                      |             | [optional] |
| **data_source** | **str**                                           |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_integrations_langchain_insights_get200_response_data import ApiIntegrationsLangchainInsightsGet200ResponseData

# TODO update the JSON string below
json = "{}"
# create an instance of ApiIntegrationsLangchainInsightsGet200ResponseData from a JSON string
api_integrations_langchain_insights_get200_response_data_instance = ApiIntegrationsLangchainInsightsGet200ResponseData.from_json(json)
# print the JSON string representation of the object
print(ApiIntegrationsLangchainInsightsGet200ResponseData.to_json())

# convert the object into a dict
api_integrations_langchain_insights_get200_response_data_dict = api_integrations_langchain_insights_get200_response_data_instance.to_dict()
# create an instance of ApiIntegrationsLangchainInsightsGet200ResponseData from a dict
api_integrations_langchain_insights_get200_response_data_from_dict = ApiIntegrationsLangchainInsightsGet200ResponseData.from_dict(api_integrations_langchain_insights_get200_response_data_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
