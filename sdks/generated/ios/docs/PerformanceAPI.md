# PerformanceAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                 | HTTP request                           | Description                      |
| -------------------------------------------------------------------------------------- | -------------------------------------- | -------------------------------- |
| [**apiPerformanceMetricsGet**](PerformanceAPI.md#apiperformancemetricsget)             | **GET** /api/performance/metrics       | Get performance metrics          |
| [**apiPerformanceOptimizationPost**](PerformanceAPI.md#apiperformanceoptimizationpost) | **POST** /api/performance/optimization | Trigger performance optimization |

# **apiPerformanceMetricsGet**

```swift
    open class func apiPerformanceMetricsGet(completion: @escaping (_ data: ApiPerformanceMetricsGet200Response?, _ error: Error?) -> Void)
```

Get performance metrics

Retrieve comprehensive performance metrics

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// Get performance metrics
PerformanceAPI.apiPerformanceMetricsGet() { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**ApiPerformanceMetricsGet200Response**](ApiPerformanceMetricsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiPerformanceOptimizationPost**

```swift
    open class func apiPerformanceOptimizationPost(apiPerformanceOptimizationPostRequest: ApiPerformanceOptimizationPostRequest, completion: @escaping (_ data: ApiPerformanceOptimizationPost200Response?, _ error: Error?) -> Void)
```

Trigger performance optimization

Initiate performance optimization procedures

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let apiPerformanceOptimizationPostRequest = _api_performance_optimization_post_request(target: "target_example", level: "level_example", duration: 123) // ApiPerformanceOptimizationPostRequest |

// Trigger performance optimization
PerformanceAPI.apiPerformanceOptimizationPost(apiPerformanceOptimizationPostRequest: apiPerformanceOptimizationPostRequest) { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
```

### Parameters

| Name                                      | Type                                                                                  | Description | Notes |
| ----------------------------------------- | ------------------------------------------------------------------------------------- | ----------- | ----- |
| **apiPerformanceOptimizationPostRequest** | [**ApiPerformanceOptimizationPostRequest**](ApiPerformanceOptimizationPostRequest.md) |             |

### Return type

[**ApiPerformanceOptimizationPost200Response**](ApiPerformanceOptimizationPost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
