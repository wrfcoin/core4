# WeatherDataSubmissionSensorDataInner

## Properties

| Name       | Type      | Description          | Notes      |
| ---------- | --------- | -------------------- | ---------- |
| **type**   | **str**   |                      | [optional] |
| **value**  | **float** | Sensor reading value | [optional] |
| **unit**   | **str**   | Unit of measurement  | [optional] |
| **status** | **str**   |                      | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_data_submission_sensor_data_inner import WeatherDataSubmissionSensorDataInner

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherDataSubmissionSensorDataInner from a JSON string
weather_data_submission_sensor_data_inner_instance = WeatherDataSubmissionSensorDataInner.from_json(json)
# print the JSON string representation of the object
print(WeatherDataSubmissionSensorDataInner.to_json())

# convert the object into a dict
weather_data_submission_sensor_data_inner_dict = weather_data_submission_sensor_data_inner_instance.to_dict()
# create an instance of WeatherDataSubmissionSensorDataInner from a dict
weather_data_submission_sensor_data_inner_from_dict = WeatherDataSubmissionSensorDataInner.from_dict(weather_data_submission_sensor_data_inner_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
