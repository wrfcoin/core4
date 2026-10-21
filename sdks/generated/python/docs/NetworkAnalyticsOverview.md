# NetworkAnalyticsOverview

## Properties

| Name                    | Type      | Description | Notes      |
| ----------------------- | --------- | ----------- | ---------- |
| **total_users**         | **int**   |             | [optional] |
| **active_users24h**     | **int**   |             | [optional] |
| **total_transactions**  | **int**   |             | [optional] |
| **total_value_locked**  | **float** |             | [optional] |
| **network_utilization** | **float** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.network_analytics_overview import NetworkAnalyticsOverview

# TODO update the JSON string below
json = "{}"
# create an instance of NetworkAnalyticsOverview from a JSON string
network_analytics_overview_instance = NetworkAnalyticsOverview.from_json(json)
# print the JSON string representation of the object
print(NetworkAnalyticsOverview.to_json())

# convert the object into a dict
network_analytics_overview_dict = network_analytics_overview_instance.to_dict()
# create an instance of NetworkAnalyticsOverview from a dict
network_analytics_overview_from_dict = NetworkAnalyticsOverview.from_dict(network_analytics_overview_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
