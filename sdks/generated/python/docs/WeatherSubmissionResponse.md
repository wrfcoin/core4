# WeatherSubmissionResponse

## Properties

| Name          | Type                                                                  | Description                           | Notes      |
| ------------- | --------------------------------------------------------------------- | ------------------------------------- | ---------- |
| **success**   | **bool**                                                              | Whether the submission was successful | [optional] |
| **data**      | [**WeatherSubmissionResponseData**](WeatherSubmissionResponseData.md) |                                       | [optional] |
| **message**   | **str**                                                               | Response message                      | [optional] |
| **timestamp** | **datetime**                                                          | Response timestamp                    | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_submission_response import WeatherSubmissionResponse

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherSubmissionResponse from a JSON string
weather_submission_response_instance = WeatherSubmissionResponse.from_json(json)
# print the JSON string representation of the object
print(WeatherSubmissionResponse.to_json())

# convert the object into a dict
weather_submission_response_dict = weather_submission_response_instance.to_dict()
# create an instance of WeatherSubmissionResponse from a dict
weather_submission_response_from_dict = WeatherSubmissionResponse.from_dict(weather_submission_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
