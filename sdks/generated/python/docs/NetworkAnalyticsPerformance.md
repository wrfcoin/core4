# NetworkAnalyticsPerformance

## Properties

| Name                      | Type      | Description | Notes      |
| ------------------------- | --------- | ----------- | ---------- |
| **average_response_time** | **float** |             | [optional] |
| **throughput**            | **float** |             | [optional] |
| **error_rate**            | **float** |             | [optional] |
| **uptime**                | **float** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.network_analytics_performance import NetworkAnalyticsPerformance

# TODO update the JSON string below
json = "{}"
# create an instance of NetworkAnalyticsPerformance from a JSON string
network_analytics_performance_instance = NetworkAnalyticsPerformance.from_json(json)
# print the JSON string representation of the object
print(NetworkAnalyticsPerformance.to_json())

# convert the object into a dict
network_analytics_performance_dict = network_analytics_performance_instance.to_dict()
# create an instance of NetworkAnalyticsPerformance from a dict
network_analytics_performance_from_dict = NetworkAnalyticsPerformance.from_dict(network_analytics_performance_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
