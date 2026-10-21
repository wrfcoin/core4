# WeatherSubmissionResponseDataQuality

## Properties

| Name          | Type          | Description | Notes      |
| ------------- | ------------- | ----------- | ---------- |
| **score**     | **float**     |             | [optional] |
| **issues**    | **List[str]** |             | [optional] |
| **validated** | **bool**      |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_submission_response_data_quality import WeatherSubmissionResponseDataQuality

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherSubmissionResponseDataQuality from a JSON string
weather_submission_response_data_quality_instance = WeatherSubmissionResponseDataQuality.from_json(json)
# print the JSON string representation of the object
print(WeatherSubmissionResponseDataQuality.to_json())

# convert the object into a dict
weather_submission_response_data_quality_dict = weather_submission_response_data_quality_instance.to_dict()
# create an instance of WeatherSubmissionResponseDataQuality from a dict
weather_submission_response_data_quality_from_dict = WeatherSubmissionResponseDataQuality.from_dict(weather_submission_response_data_quality_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
