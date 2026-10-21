# WeatherMeasurement

## Properties

| Name            | Type      | Description             | Notes      |
| --------------- | --------- | ----------------------- | ---------- |
| **type**        | **str**   |                         |
| **value**       | **float** | Measurement value       |
| **unit**        | **str**   | Unit of measurement     |
| **uncertainty** | **float** | Measurement uncertainty | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_measurement import WeatherMeasurement

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherMeasurement from a JSON string
weather_measurement_instance = WeatherMeasurement.from_json(json)
# print the JSON string representation of the object
print(WeatherMeasurement.to_json())

# convert the object into a dict
weather_measurement_dict = weather_measurement_instance.to_dict()
# create an instance of WeatherMeasurement from a dict
weather_measurement_from_dict = WeatherMeasurement.from_dict(weather_measurement_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
