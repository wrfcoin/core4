# ApiSecurityStatsGet200Response

## Properties

| Name          | Type                                  | Description | Notes      |
| ------------- | ------------------------------------- | ----------- | ---------- |
| **success**   | **bool**                              |             | [optional] |
| **data**      | [**SecurityStats**](SecurityStats.md) |             | [optional] |
| **message**   | **str**                               |             | [optional] |
| **timestamp** | **datetime**                          |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_security_stats_get200_response import ApiSecurityStatsGet200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiSecurityStatsGet200Response from a JSON string
api_security_stats_get200_response_instance = ApiSecurityStatsGet200Response.from_json(json)
# print the JSON string representation of the object
print(ApiSecurityStatsGet200Response.to_json())

# convert the object into a dict
api_security_stats_get200_response_dict = api_security_stats_get200_response_instance.to_dict()
# create an instance of ApiSecurityStatsGet200Response from a dict
api_security_stats_get200_response_from_dict = ApiSecurityStatsGet200Response.from_dict(api_security_stats_get200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
