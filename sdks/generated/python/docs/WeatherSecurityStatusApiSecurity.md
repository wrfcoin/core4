# WeatherSecurityStatusApiSecurity

## Properties

| Name                  | Type     | Description | Notes      |
| --------------------- | -------- | ----------- | ---------- |
| **rate_limiting**     | **bool** |             | [optional] |
| **ddos_protection**   | **bool** |             | [optional] |
| **ssl_grade**         | **str**  |             | [optional] |
| **certificate_valid** | **bool** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.weather_security_status_api_security import WeatherSecurityStatusApiSecurity

# TODO update the JSON string below
json = "{}"
# create an instance of WeatherSecurityStatusApiSecurity from a JSON string
weather_security_status_api_security_instance = WeatherSecurityStatusApiSecurity.from_json(json)
# print the JSON string representation of the object
print(WeatherSecurityStatusApiSecurity.to_json())

# convert the object into a dict
weather_security_status_api_security_dict = weather_security_status_api_security_instance.to_dict()
# create an instance of WeatherSecurityStatusApiSecurity from a dict
weather_security_status_api_security_from_dict = WeatherSecurityStatusApiSecurity.from_dict(weather_security_status_api_security_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
