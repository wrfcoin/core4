# NetworkAnalyticsWeather

## Properties

| Name                       | Type      | Description | Notes      |
| -------------------------- | --------- | ----------- | ---------- |
| **active_stations**        | **int**   |             | [optional] |
| **data_submissions24h**    | **int**   |             | [optional] |
| **average_quality**        | **float** |             | [optional] |
| **rewards_distributed24h** | **float** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.network_analytics_weather import NetworkAnalyticsWeather

# TODO update the JSON string below
json = "{}"
# create an instance of NetworkAnalyticsWeather from a JSON string
network_analytics_weather_instance = NetworkAnalyticsWeather.from_json(json)
# print the JSON string representation of the object
print(NetworkAnalyticsWeather.to_json())

# convert the object into a dict
network_analytics_weather_dict = network_analytics_weather_instance.to_dict()
# create an instance of NetworkAnalyticsWeather from a dict
network_analytics_weather_from_dict = NetworkAnalyticsWeather.from_dict(network_analytics_weather_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
