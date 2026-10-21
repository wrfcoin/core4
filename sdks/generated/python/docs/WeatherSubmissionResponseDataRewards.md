# WeatherSubmissionResponseDataRewards

## Properties

| Name                | Type      | Description            | Notes      |
| ------------------- | --------- | ---------------------- | ---------- |
| **base_amount**     | **float** | Base reward amount     | [optional] |
| **quality_bonus**   | **float** | Quality bonus amount   | [optional] |
| **proximity_bonus** | **float** | Proximity bonus amount | [optional] |
| **total_amount**    | **float** | Total reward amount    | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_submission_response_data_rewards import WeatherSubmissionResponseDataRewards

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherSubmissionResponseDataRewards from a JSON string
weather_submission_response_data_rewards_instance = WeatherSubmissionResponseDataRewards.from_json(json)
# print the JSON string representation of the object
print(WeatherSubmissionResponseDataRewards.to_json())

# convert the object into a dict
weather_submission_response_data_rewards_dict = weather_submission_response_data_rewards_instance.to_dict()
# create an instance of WeatherSubmissionResponseDataRewards from a dict
weather_submission_response_data_rewards_from_dict = WeatherSubmissionResponseDataRewards.from_dict(weather_submission_response_data_rewards_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
