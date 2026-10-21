# WeatherSubmissionResponseDataSensorsInner

## Properties

| Name       | Type      | Description | Notes      |
| ---------- | --------- | ----------- | ---------- |
| **type**   | **str**   |             | [optional] |
| **value**  | **float** |             | [optional] |
| **unit**   | **str**   |             | [optional] |
| **status** | **str**   |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_submission_response_data_sensors_inner import WeatherSubmissionResponseDataSensorsInner

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherSubmissionResponseDataSensorsInner from a JSON string
weather_submission_response_data_sensors_inner_instance = WeatherSubmissionResponseDataSensorsInner.from_json(json)
# print the JSON string representation of the object
print(WeatherSubmissionResponseDataSensorsInner.to_json())

# convert the object into a dict
weather_submission_response_data_sensors_inner_dict = weather_submission_response_data_sensors_inner_instance.to_dict()
# create an instance of WeatherSubmissionResponseDataSensorsInner from a dict
weather_submission_response_data_sensors_inner_from_dict = WeatherSubmissionResponseDataSensorsInner.from_dict(weather_submission_response_data_sensors_inner_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
