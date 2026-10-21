# WeatherSecurityStatus

## Properties

| Name                       | Type                                                                              | Description | Notes      |
| -------------------------- | --------------------------------------------------------------------------------- | ----------- | ---------- |
| **station_security**       | **str**                                                                           |             | [optional] |
| **data_integrity**         | **float**                                                                         |             | [optional] |
| **transmission_encrypted** | **bool**                                                                          |             | [optional] |
| **last_security_check**    | **datetime**                                                                      |             | [optional] |
| **active_stations**        | **int**                                                                           |             | [optional] |
| **secure_stations**        | **int**                                                                           |             | [optional] |
| **flagged_stations**       | **int**                                                                           |             | [optional] |
| **authentication_status**  | **str**                                                                           |             | [optional] |
| **api_security**           | [**WeatherSecurityStatusApiSecurity**](WeatherSecurityStatusApiSecurity.md)       |             | [optional] |
| **data_validation**        | [**WeatherSecurityStatusDataValidation**](WeatherSecurityStatusDataValidation.md) |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_security_status import WeatherSecurityStatus

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherSecurityStatus from a JSON string
weather_security_status_instance = WeatherSecurityStatus.from_json(json)
# print the JSON string representation of the object
print(WeatherSecurityStatus.to_json())

# convert the object into a dict
weather_security_status_dict = weather_security_status_instance.to_dict()
# create an instance of WeatherSecurityStatus from a dict
weather_security_status_from_dict = WeatherSecurityStatus.from_dict(weather_security_status_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
