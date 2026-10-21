# SystemAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                        | HTTP request        | Description         |
| --------------------------------------------- | ------------------- | ------------------- |
| [**apiHealthGet**](SystemAPI.md#apihealthget) | **GET** /api/health | API health check    |
| [**healthGet**](SystemAPI.md#healthget)       | **GET** /health     | System health check |

# **apiHealthGet**

```swift
    open class func apiHealthGet(completion: @escaping (_ data: HealthGet200Response?, _ error: Error?) -> Void)
```

API health check

Get comprehensive API health status

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// API health check
SystemAPI.apiHealthGet() { (response, error) in
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

[**HealthGet200Response**](HealthGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **healthGet**

```swift
    open class func healthGet(completion: @escaping (_ data: HealthGet200Response?, _ error: Error?) -> Void)
```

System health check

Get comprehensive system health status

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// System health check
SystemAPI.healthGet() { (response, error) in
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

[**HealthGet200Response**](HealthGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
