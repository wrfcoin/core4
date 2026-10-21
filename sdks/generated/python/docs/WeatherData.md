# WeatherData

## Properties

| Name              | Type                                            | Description | Notes      |
| ----------------- | ----------------------------------------------- | ----------- | ---------- |
| **id**            | **str**                                         |             | [optional] |
| **station_id**    | **str**                                         |             | [optional] |
| **timestamp**     | **int**                                         |             | [optional] |
| **location**      | [**Location**](Location.md)                     |             | [optional] |
| **measurement**   | [**WeatherMeasurement**](WeatherMeasurement.md) |             | [optional] |
| **quality_score** | **float**                                       |             | [optional] |
| **reward_amount** | **str**                                         |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_data import WeatherData

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherData from a JSON string
weather_data_instance = WeatherData.from_json(json)
# print the JSON string representation of the object
print(WeatherData.to_json())

# convert the object into a dict
weather_data_dict = weather_data_instance.to_dict()
# create an instance of WeatherData from a dict
weather_data_from_dict = WeatherData.from_dict(weather_data_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
