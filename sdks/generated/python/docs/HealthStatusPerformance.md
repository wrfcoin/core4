# HealthStatusPerformance

## Properties

| Name        | Type                                                                    | Description | Notes      |
| ----------- | ----------------------------------------------------------------------- | ----------- | ---------- |
| **cpu**     | [**HealthStatusPerformanceCpu**](HealthStatusPerformanceCpu.md)         |             | [optional] |
| **memory**  | [**HealthStatusDatabaseStorage**](HealthStatusDatabaseStorage.md)       |             | [optional] |
| **disk**    | [**HealthStatusDatabaseStorage**](HealthStatusDatabaseStorage.md)       |             | [optional] |
| **network** | [**HealthStatusPerformanceNetwork**](HealthStatusPerformanceNetwork.md) |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_performance import HealthStatusPerformance

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusPerformance from a JSON string
health_status_performance_instance = HealthStatusPerformance.from_json(json)
# print the JSON string representation of the object
print(HealthStatusPerformance.to_json())

# convert the object into a dict
health_status_performance_dict = health_status_performance_instance.to_dict()
# create an instance of HealthStatusPerformance from a dict
health_status_performance_from_dict = HealthStatusPerformance.from_dict(health_status_performance_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
