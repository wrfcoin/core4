# HealthStatusDatabaseStorage

## Properties

| Name            | Type      | Description | Notes      |
| --------------- | --------- | ----------- | ---------- |
| **used**        | **int**   |             | [optional] |
| **available**   | **int**   |             | [optional] |
| **utilization** | **float** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_database_storage import HealthStatusDatabaseStorage

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusDatabaseStorage from a JSON string
health_status_database_storage_instance = HealthStatusDatabaseStorage.from_json(json)
# print the JSON string representation of the object
print(HealthStatusDatabaseStorage.to_json())

# convert the object into a dict
health_status_database_storage_dict = health_status_database_storage_instance.to_dict()
# create an instance of HealthStatusDatabaseStorage from a dict
health_status_database_storage_from_dict = HealthStatusDatabaseStorage.from_dict(health_status_database_storage_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
