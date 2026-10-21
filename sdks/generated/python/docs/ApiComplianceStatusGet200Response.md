# ApiComplianceStatusGet200Response

## Properties

| Name          | Type                                        | Description | Notes      |
| ------------- | ------------------------------------------- | ----------- | ---------- |
| **success**   | **bool**                                    |             | [optional] |
| **data**      | [**ComplianceStatus**](ComplianceStatus.md) |             | [optional] |
| **message**   | **str**                                     |             | [optional] |
| **timestamp** | **datetime**                                |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_compliance_status_get200_response import ApiComplianceStatusGet200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiComplianceStatusGet200Response from a JSON string
api_compliance_status_get200_response_instance = ApiComplianceStatusGet200Response.from_json(json)
# print the JSON string representation of the object
print(ApiComplianceStatusGet200Response.to_json())

# convert the object into a dict
api_compliance_status_get200_response_dict = api_compliance_status_get200_response_instance.to_dict()
# create an instance of ApiComplianceStatusGet200Response from a dict
api_compliance_status_get200_response_from_dict = ApiComplianceStatusGet200Response.from_dict(api_compliance_status_get200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
