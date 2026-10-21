# HealthStatusPerformanceCpu

## Properties

| Name             | Type            | Description | Notes      |
| ---------------- | --------------- | ----------- | ---------- |
| **usage**        | **float**       |             | [optional] |
| **load_average** | **List[float]** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_performance_cpu import HealthStatusPerformanceCpu

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusPerformanceCpu from a JSON string
health_status_performance_cpu_instance = HealthStatusPerformanceCpu.from_json(json)
# print the JSON string representation of the object
print(HealthStatusPerformanceCpu.to_json())

# convert the object into a dict
health_status_performance_cpu_dict = health_status_performance_cpu_instance.to_dict()
# create an instance of HealthStatusPerformanceCpu from a dict
health_status_performance_cpu_from_dict = HealthStatusPerformanceCpu.from_dict(health_status_performance_cpu_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
