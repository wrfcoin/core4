# HealthStatusDatabaseQueryPerformance

## Properties

| Name             | Type      | Description | Notes      |
| ---------------- | --------- | ----------- | ---------- |
| **average_time** | **float** |             | [optional] |
| **slow_queries** | **int**   |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_database_query_performance import HealthStatusDatabaseQueryPerformance

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusDatabaseQueryPerformance from a JSON string
health_status_database_query_performance_instance = HealthStatusDatabaseQueryPerformance.from_json(json)
# print the JSON string representation of the object
print(HealthStatusDatabaseQueryPerformance.to_json())

# convert the object into a dict
health_status_database_query_performance_dict = health_status_database_query_performance_instance.to_dict()
# create an instance of HealthStatusDatabaseQueryPerformance from a dict
health_status_database_query_performance_from_dict = HealthStatusDatabaseQueryPerformance.from_dict(health_status_database_query_performance_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
