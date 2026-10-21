# HealthStatusDatabaseConnectionPool

## Properties

| Name       | Type    | Description | Notes      |
| ---------- | ------- | ----------- | ---------- |
| **total**  | **int** |             | [optional] |
| **active** | **int** |             | [optional] |
| **idle**   | **int** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_database_connection_pool import HealthStatusDatabaseConnectionPool

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusDatabaseConnectionPool from a JSON string
health_status_database_connection_pool_instance = HealthStatusDatabaseConnectionPool.from_json(json)
# print the JSON string representation of the object
print(HealthStatusDatabaseConnectionPool.to_json())

# convert the object into a dict
health_status_database_connection_pool_dict = health_status_database_connection_pool_instance.to_dict()
# create an instance of HealthStatusDatabaseConnectionPool from a dict
health_status_database_connection_pool_from_dict = HealthStatusDatabaseConnectionPool.from_dict(health_status_database_connection_pool_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
