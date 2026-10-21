# WeatherSubmissionResponseData

## Properties

| Name           | Type                                                                                                | Description                | Notes      |
| -------------- | --------------------------------------------------------------------------------------------------- | -------------------------- | ---------- |
| **station_id** | **str**                                                                                             | Weather station identifier | [optional] |
| **timestamp**  | **datetime**                                                                                        | Submission timestamp       | [optional] |
| **location**   | [**Location**](Location.md)                                                                         |                            | [optional] |
| **sensors**    | [**List[WeatherSubmissionResponseDataSensorsInner]**](WeatherSubmissionResponseDataSensorsInner.md) |                            | [optional] |
| **quality**    | [**WeatherSubmissionResponseDataQuality**](WeatherSubmissionResponseDataQuality.md)                 |                            | [optional] |
| **rewards**    | [**WeatherSubmissionResponseDataRewards**](WeatherSubmissionResponseDataRewards.md)                 |                            | [optional] |
| **blockchain** | [**WeatherSubmissionResponseDataBlockchain**](WeatherSubmissionResponseDataBlockchain.md)           |                            | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_submission_response_data import WeatherSubmissionResponseData

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherSubmissionResponseData from a JSON string
weather_submission_response_data_instance = WeatherSubmissionResponseData.from_json(json)
# print the JSON string representation of the object
print(WeatherSubmissionResponseData.to_json())

# convert the object into a dict
weather_submission_response_data_dict = weather_submission_response_data_instance.to_dict()
# create an instance of WeatherSubmissionResponseData from a dict
weather_submission_response_data_from_dict = WeatherSubmissionResponseData.from_dict(weather_submission_response_data_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
