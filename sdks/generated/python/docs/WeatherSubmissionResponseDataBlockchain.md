# WeatherSubmissionResponseDataBlockchain

## Properties

| Name                 | Type    | Description                                 | Notes      |
| -------------------- | ------- | ------------------------------------------- | ---------- |
| **block_number**     | **int** | Block number where transaction was included | [optional] |
| **transaction_hash** | **str** | Transaction hash                            | [optional] |
| **gas_used**         | **int** | Gas used for transaction                    | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_submission_response_data_blockchain import WeatherSubmissionResponseDataBlockchain

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherSubmissionResponseDataBlockchain from a JSON string
weather_submission_response_data_blockchain_instance = WeatherSubmissionResponseDataBlockchain.from_json(json)
# print the JSON string representation of the object
print(WeatherSubmissionResponseDataBlockchain.to_json())

# convert the object into a dict
weather_submission_response_data_blockchain_dict = weather_submission_response_data_blockchain_instance.to_dict()
# create an instance of WeatherSubmissionResponseDataBlockchain from a dict
weather_submission_response_data_blockchain_from_dict = WeatherSubmissionResponseDataBlockchain.from_dict(weather_submission_response_data_blockchain_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
