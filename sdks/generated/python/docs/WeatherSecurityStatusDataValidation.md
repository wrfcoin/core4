# WeatherSecurityStatusDataValidation

## Properties

| Name                    | Type     | Description | Notes      |
| ----------------------- | -------- | ----------- | ---------- |
| **sensor_checks**       | **bool** |             | [optional] |
| **anomaly_detection**   | **bool** |             | [optional] |
| **data_sanitization**   | **bool** |             | [optional] |
| **backup_verification** | **bool** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_security_status_data_validation import WeatherSecurityStatusDataValidation

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherSecurityStatusDataValidation from a JSON string
weather_security_status_data_validation_instance = WeatherSecurityStatusDataValidation.from_json(json)
# print the JSON string representation of the object
print(WeatherSecurityStatusDataValidation.to_json())

# convert the object into a dict
weather_security_status_data_validation_dict = weather_security_status_data_validation_instance.to_dict()
# create an instance of WeatherSecurityStatusDataValidation from a dict
weather_security_status_data_validation_from_dict = WeatherSecurityStatusDataValidation.from_dict(weather_security_status_data_validation_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
