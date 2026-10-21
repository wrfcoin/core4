# WeatherDataSubmissionQuality

## Properties

| Name       | Type          | Description             | Notes      |
| ---------- | ------------- | ----------------------- | ---------- |
| **score**  | **float**     | Data quality score      | [optional] |
| **issues** | **List[str]** | Quality issues detected | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_data_submission_quality import WeatherDataSubmissionQuality

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherDataSubmissionQuality from a JSON string
weather_data_submission_quality_instance = WeatherDataSubmissionQuality.from_json(json)
# print the JSON string representation of the object
print(WeatherDataSubmissionQuality.to_json())

# convert the object into a dict
weather_data_submission_quality_dict = weather_data_submission_quality_instance.to_dict()
# create an instance of WeatherDataSubmissionQuality from a dict
weather_data_submission_quality_from_dict = WeatherDataSubmissionQuality.from_dict(weather_data_submission_quality_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
