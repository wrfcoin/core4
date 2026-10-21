# HealthGet200Response

## Properties

| Name          | Type                                | Description | Notes      |
| ------------- | ----------------------------------- | ----------- | ---------- |
| **success**   | **bool**                            |             | [optional] |
| **data**      | [**HealthStatus**](HealthStatus.md) |             | [optional] |
| **message**   | **str**                             |             | [optional] |
| **timestamp** | **datetime**                        |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_get200_response import HealthGet200Response

# TODO update the JSON string below
json = "{}"
# create an instance of HealthGet200Response from a JSON string
health_get200_response_instance = HealthGet200Response.from_json(json)
# print the JSON string representation of the object
print(HealthGet200Response.to_json())

# convert the object into a dict
health_get200_response_dict = health_get200_response_instance.to_dict()
# create an instance of HealthGet200Response from a dict
health_get200_response_from_dict = HealthGet200Response.from_dict(health_get200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
