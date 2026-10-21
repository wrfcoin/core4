# \PerformanceApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                       | HTTP request                           | Description                      |
| -------------------------------------------------------------------------------------------- | -------------------------------------- | -------------------------------- |
| [**api_performance_metrics_get**](PerformanceApi.md#api_performance_metrics_get)             | **GET** /api/performance/metrics       | Get performance metrics          |
| [**api_performance_optimization_post**](PerformanceApi.md#api_performance_optimization_post) | **POST** /api/performance/optimization | Trigger performance optimization |

## api_performance_metrics_get

> models::ApiPerformanceMetricsGet200Response api_performance_metrics_get() Get performance metrics

Retrieve comprehensive performance metrics

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::ApiPerformanceMetricsGet200Response**](_api_performance_metrics_get_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_performance_optimization_post

> models::ApiPerformanceOptimizationPost200Response
> api_performance_optimization_post(api_performance_optimization_post_request) Trigger performance
> optimization

Initiate performance optimization procedures

### Parameters

| Name                                          | Type                                                                                  | Description | Required   | Notes |
| --------------------------------------------- | ------------------------------------------------------------------------------------- | ----------- | ---------- | ----- |
| **api_performance_optimization_post_request** | [**ApiPerformanceOptimizationPostRequest**](ApiPerformanceOptimizationPostRequest.md) |             | [required] |

### Return type

[**models::ApiPerformanceOptimizationPost200Response**](_api_performance_optimization_post_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
