# \WeatherDataApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                   | HTTP request                         | Description                 |
| ---------------------------------------------------------------------------------------- | ------------------------------------ | --------------------------- |
| [**api_weather_data_get**](WeatherDataApi.md#api_weather_data_get)                       | **GET** /api/weather/data            | Get weather data            |
| [**api_weather_security_status_get**](WeatherDataApi.md#api_weather_security_status_get) | **GET** /api/weather/security-status | Get weather security status |
| [**api_weather_submit_post**](WeatherDataApi.md#api_weather_submit_post)                 | **POST** /api/weather/submit         | Submit weather data         |

## api_weather_data_get

> Vec<models::WeatherData> api_weather_data_get(station_id, start_date, end_date, data_type) Get
> weather data

### Parameters

| Name           | Type               | Description | Required | Notes |
| -------------- | ------------------ | ----------- | -------- | ----- |
| **station_id** | Option<**String**> |             |          |
| **start_date** | Option<**String**> |             |          |
| **end_date**   | Option<**String**> |             |          |
| **data_type**  | Option<**String**> |             |          |

### Return type

[**Vec<models::WeatherData>**](WeatherData.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_weather_security_status_get

> models::WeatherSecurityStatus api_weather_security_status_get() Get weather security status

Retrieve security status of weather stations and data integrity

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::WeatherSecurityStatus**](WeatherSecurityStatus.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_weather_submit_post

> models::WeatherSubmissionResponse api_weather_submit_post(weather_data_submission) Submit weather
> data

Submit environmental data from weather stations

### Parameters

| Name                        | Type                                                  | Description | Required   | Notes |
| --------------------------- | ----------------------------------------------------- | ----------- | ---------- | ----- |
| **weather_data_submission** | [**WeatherDataSubmission**](WeatherDataSubmission.md) |             | [required] |

### Return type

[**models::WeatherSubmissionResponse**](WeatherSubmissionResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
