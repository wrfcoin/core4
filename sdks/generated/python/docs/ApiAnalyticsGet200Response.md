# ApiAnalyticsGet200Response

## Properties

| Name          | Type                                        | Description | Notes      |
| ------------- | ------------------------------------------- | ----------- | ---------- |
| **success**   | **bool**                                    |             | [optional] |
| **data**      | [**NetworkAnalytics**](NetworkAnalytics.md) |             | [optional] |
| **message**   | **str**                                     |             | [optional] |
| **timestamp** | **datetime**                                |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_analytics_get200_response import ApiAnalyticsGet200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiAnalyticsGet200Response from a JSON string
api_analytics_get200_response_instance = ApiAnalyticsGet200Response.from_json(json)
# print the JSON string representation of the object
print(ApiAnalyticsGet200Response.to_json())

# convert the object into a dict
api_analytics_get200_response_dict = api_analytics_get200_response_instance.to_dict()
# create an instance of ApiAnalyticsGet200Response from a dict
api_analytics_get200_response_from_dict = ApiAnalyticsGet200Response.from_dict(api_analytics_get200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
