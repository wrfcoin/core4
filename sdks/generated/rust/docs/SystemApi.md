# \SystemApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                            | HTTP request        | Description         |
| ------------------------------------------------- | ------------------- | ------------------- |
| [**api_health_get**](SystemApi.md#api_health_get) | **GET** /api/health | API health check    |
| [**health_get**](SystemApi.md#health_get)         | **GET** /health     | System health check |

## api_health_get

> models::HealthGet200Response api_health_get() API health check

Get comprehensive API health status

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::HealthGet200Response**](_health_get_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## health_get

> models::HealthGet200Response health_get() System health check

Get comprehensive system health status

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::HealthGet200Response**](_health_get_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
