# HealthStatusPerformanceNetwork

## Properties

| Name            | Type    | Description | Notes      |
| --------------- | ------- | ----------- | ---------- |
| **bytes_in**    | **int** |             | [optional] |
| **bytes_out**   | **int** |             | [optional] |
| **packets_in**  | **int** |             | [optional] |
| **packets_out** | **int** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_performance_network import HealthStatusPerformanceNetwork

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusPerformanceNetwork from a JSON string
health_status_performance_network_instance = HealthStatusPerformanceNetwork.from_json(json)
# print the JSON string representation of the object
print(HealthStatusPerformanceNetwork.to_json())

# convert the object into a dict
health_status_performance_network_dict = health_status_performance_network_instance.to_dict()
# create an instance of HealthStatusPerformanceNetwork from a dict
health_status_performance_network_from_dict = HealthStatusPerformanceNetwork.from_dict(health_status_performance_network_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
