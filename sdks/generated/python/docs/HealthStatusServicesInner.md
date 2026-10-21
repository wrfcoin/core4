# HealthStatusServicesInner

## Properties

| Name              | Type          | Description | Notes      |
| ----------------- | ------------- | ----------- | ---------- |
| **name**          | **str**       |             | [optional] |
| **status**        | **str**       |             | [optional] |
| **response_time** | **float**     |             | [optional] |
| **error_rate**    | **float**     |             | [optional] |
| **last_check**    | **datetime**  |             | [optional] |
| **dependencies**  | **List[str]** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_services_inner import HealthStatusServicesInner

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusServicesInner from a JSON string
health_status_services_inner_instance = HealthStatusServicesInner.from_json(json)
# print the JSON string representation of the object
print(HealthStatusServicesInner.to_json())

# convert the object into a dict
health_status_services_inner_dict = health_status_services_inner_instance.to_dict()
# create an instance of HealthStatusServicesInner from a dict
health_status_services_inner_from_dict = HealthStatusServicesInner.from_dict(health_status_services_inner_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
