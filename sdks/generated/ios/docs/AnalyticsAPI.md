# AnalyticsAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                 | HTTP request           | Description           |
| ------------------------------------------------------ | ---------------------- | --------------------- |
| [**apiAnalyticsGet**](AnalyticsAPI.md#apianalyticsget) | **GET** /api/analytics | Get network analytics |

# **apiAnalyticsGet**

```swift
    open class func apiAnalyticsGet(completion: @escaping (_ data: ApiAnalyticsGet200Response?, _ error: Error?) -> Void)
```

Get network analytics

Retrieve comprehensive network analytics and metrics

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// Get network analytics
AnalyticsAPI.apiAnalyticsGet() { (response, error) in
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

[**ApiAnalyticsGet200Response**](ApiAnalyticsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
