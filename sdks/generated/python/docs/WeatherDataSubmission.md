# WeatherDataSubmission

## Properties

| Name             | Type                                                                                      | Description                | Notes      |
| ---------------- | ----------------------------------------------------------------------------------------- | -------------------------- | ---------- |
| **station_id**   | **str**                                                                                   | Weather station identifier |
| **timestamp**    | **int**                                                                                   | Data collection timestamp  | [optional] |
| **location**     | [**Location**](Location.md)                                                               |                            | [optional] |
| **measurements** | [**List[WeatherMeasurement]**](WeatherMeasurement.md)                                     |                            | [optional] |
| **signature**    | **str**                                                                                   | Data integrity signature   | [optional] |
| **sensor_data**  | [**List[WeatherDataSubmissionSensorDataInner]**](WeatherDataSubmissionSensorDataInner.md) | Array of sensor readings   |
| **quality**      | [**WeatherDataSubmissionQuality**](WeatherDataSubmissionQuality.md)                       |                            | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_data_submission import WeatherDataSubmission

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherDataSubmission from a JSON string
weather_data_submission_instance = WeatherDataSubmission.from_json(json)
# print the JSON string representation of the object
print(WeatherDataSubmission.to_json())

# convert the object into a dict
weather_data_submission_dict = weather_data_submission_instance.to_dict()
# create an instance of WeatherDataSubmission from a dict
weather_data_submission_from_dict = WeatherDataSubmission.from_dict(weather_data_submission_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
