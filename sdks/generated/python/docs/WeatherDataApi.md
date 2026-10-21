# wrfcoin_sdk.WeatherDataApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                   | HTTP request                         | Description                 |
| ---------------------------------------------------------------------------------------- | ------------------------------------ | --------------------------- |
| [**api_weather_data_get**](WeatherDataApi.md#api_weather_data_get)                       | **GET** /api/weather/data            | Get weather data            |
| [**api_weather_security_status_get**](WeatherDataApi.md#api_weather_security_status_get) | **GET** /api/weather/security-status | Get weather security status |
| [**api_weather_submit_post**](WeatherDataApi.md#api_weather_submit_post)                 | **POST** /api/weather/submit         | Submit weather data         |

# **api_weather_data_get**

> List[WeatherData] api_weather_data_get(station_id=station_id, start_date=start_date,
> end_date=end_date, data_type=data_type)

Get weather data

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.weather_data import WeatherData
from wrfcoin_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to https://api.wrfcoin.com/v1
# See configuration.py for a list of all supported configuration parameters.
configuration = wrfcoin_sdk.Configuration(
    host = "https://api.wrfcoin.com/v1"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure API key authorization: ApiKeyAuth
configuration.api_key['ApiKeyAuth'] = os.environ["API_KEY"]

# Uncomment below to setup prefix (e.g. Bearer) for API key, if needed
# configuration.api_key_prefix['ApiKeyAuth'] = 'Bearer'

# Configure Bearer authorization (JWT): BearerAuth
configuration = wrfcoin_sdk.Configuration(
    access_token = os.environ["BEARER_TOKEN"]
)

# Enter a context with an instance of the API client
with wrfcoin_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = wrfcoin_sdk.WeatherDataApi(api_client)
    station_id = 'station_id_example' # str |  (optional)
    start_date = '2013-10-20T19:20:30+01:00' # datetime |  (optional)
    end_date = '2013-10-20T19:20:30+01:00' # datetime |  (optional)
    data_type = 'data_type_example' # str |  (optional)

    try:
        # Get weather data
        api_response = api_instance.api_weather_data_get(station_id=station_id, start_date=start_date, end_date=end_date, data_type=data_type)
        print("The response of WeatherDataApi->api_weather_data_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling WeatherDataApi->api_weather_data_get: %s\n" % e)
```

### Parameters

| Name           | Type         | Description | Notes      |
| -------------- | ------------ | ----------- | ---------- |
| **station_id** | **str**      |             | [optional] |
| **start_date** | **datetime** |             | [optional] |
| **end_date**   | **datetime** |             | [optional] |
| **data_type**  | **str**      |             | [optional] |

### Return type

[**List[WeatherData]**](WeatherData.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | Weather data retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_weather_security_status_get**

> WeatherSecurityStatus api_weather_security_status_get()

Get weather security status

Retrieve security status of weather stations and data integrity

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.weather_security_status import WeatherSecurityStatus
from wrfcoin_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to https://api.wrfcoin.com/v1
# See configuration.py for a list of all supported configuration parameters.
configuration = wrfcoin_sdk.Configuration(
    host = "https://api.wrfcoin.com/v1"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure API key authorization: ApiKeyAuth
configuration.api_key['ApiKeyAuth'] = os.environ["API_KEY"]

# Uncomment below to setup prefix (e.g. Bearer) for API key, if needed
# configuration.api_key_prefix['ApiKeyAuth'] = 'Bearer'

# Configure Bearer authorization (JWT): BearerAuth
configuration = wrfcoin_sdk.Configuration(
    access_token = os.environ["BEARER_TOKEN"]
)

# Enter a context with an instance of the API client
with wrfcoin_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = wrfcoin_sdk.WeatherDataApi(api_client)

    try:
        # Get weather security status
        api_response = api_instance.api_weather_security_status_get()
        print("The response of WeatherDataApi->api_weather_security_status_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling WeatherDataApi->api_weather_security_status_get: %s\n" % e)
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**WeatherSecurityStatus**](WeatherSecurityStatus.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                    | Response headers |
| ----------- | ---------------------------------------------- | ---------------- |
| **200**     | Weather security status retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_weather_submit_post**

> WeatherSubmissionResponse api_weather_submit_post(weather_data_submission)

Submit weather data

Submit environmental data from weather stations

### Example

- Api Key Authentication (ApiKeyAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.weather_data_submission import WeatherDataSubmission
from wrfcoin_sdk.models.weather_submission_response import WeatherSubmissionResponse
from wrfcoin_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to https://api.wrfcoin.com/v1
# See configuration.py for a list of all supported configuration parameters.
configuration = wrfcoin_sdk.Configuration(
    host = "https://api.wrfcoin.com/v1"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure API key authorization: ApiKeyAuth
configuration.api_key['ApiKeyAuth'] = os.environ["API_KEY"]

# Uncomment below to setup prefix (e.g. Bearer) for API key, if needed
# configuration.api_key_prefix['ApiKeyAuth'] = 'Bearer'

# Enter a context with an instance of the API client
with wrfcoin_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = wrfcoin_sdk.WeatherDataApi(api_client)
    weather_data_submission = {"stationId":"station_001","sensorData":[{"type":"temperature","value":23.5,"unit":"celsius","status":"active"},{"type":"humidity","value":65.2,"unit":"percent","status":"active"}],"location":{"latitude":40.7128,"longitude":-74.006,"altitude":10},"quality":{"score":95,"issues":[]}} # WeatherDataSubmission |

    try:
        # Submit weather data
        api_response = api_instance.api_weather_submit_post(weather_data_submission)
        print("The response of WeatherDataApi->api_weather_submit_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling WeatherDataApi->api_weather_submit_post: %s\n" % e)
```

### Parameters

| Name                        | Type                                                  | Description | Notes |
| --------------------------- | ----------------------------------------------------- | ----------- | ----- |
| **weather_data_submission** | [**WeatherDataSubmission**](WeatherDataSubmission.md) |             |

### Return type

[**WeatherSubmissionResponse**](WeatherSubmissionResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | Weather data submitted successfully | -                |
| **400**     | Invalid weather data                | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
