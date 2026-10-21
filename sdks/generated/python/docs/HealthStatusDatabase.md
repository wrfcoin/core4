# HealthStatusDatabase

## Properties

| Name                  | Type                                                                                | Description | Notes      |
| --------------------- | ----------------------------------------------------------------------------------- | ----------- | ---------- |
| **status**            | **str**                                                                             |             | [optional] |
| **connection_pool**   | [**HealthStatusDatabaseConnectionPool**](HealthStatusDatabaseConnectionPool.md)     |             | [optional] |
| **query_performance** | [**HealthStatusDatabaseQueryPerformance**](HealthStatusDatabaseQueryPerformance.md) |             | [optional] |
| **storage**           | [**HealthStatusDatabaseStorage**](HealthStatusDatabaseStorage.md)                   |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_database import HealthStatusDatabase

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusDatabase from a JSON string
health_status_database_instance = HealthStatusDatabase.from_json(json)
# print the JSON string representation of the object
print(HealthStatusDatabase.to_json())

# convert the object into a dict
health_status_database_dict = health_status_database_instance.to_dict()
# create an instance of HealthStatusDatabase from a dict
health_status_database_from_dict = HealthStatusDatabase.from_dict(health_status_database_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
