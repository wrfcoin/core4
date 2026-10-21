# NetworkAnalytics

## Properties

| Name            | Type                                                              | Description | Notes      |
| --------------- | ----------------------------------------------------------------- | ----------- | ---------- |
| **overview**    | [**NetworkAnalyticsOverview**](NetworkAnalyticsOverview.md)       |             | [optional] |
| **performance** | [**NetworkAnalyticsPerformance**](NetworkAnalyticsPerformance.md) |             | [optional] |
| **security**    | [**NetworkAnalyticsSecurity**](NetworkAnalyticsSecurity.md)       |             | [optional] |
| **cross_chain** | [**NetworkAnalyticsCrossChain**](NetworkAnalyticsCrossChain.md)   |             | [optional] |
| **weather**     | [**NetworkAnalyticsWeather**](NetworkAnalyticsWeather.md)         |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.network_analytics import NetworkAnalytics

# TODO update the JSON string below
json = "{}"
# create an instance of NetworkAnalytics from a JSON string
network_analytics_instance = NetworkAnalytics.from_json(json)
# print the JSON string representation of the object
print(NetworkAnalytics.to_json())

# convert the object into a dict
network_analytics_dict = network_analytics_instance.to_dict()
# create an instance of NetworkAnalytics from a dict
network_analytics_from_dict = NetworkAnalytics.from_dict(network_analytics_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
