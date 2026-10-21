# SecurityAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                        | HTTP request                | Description             |
| ------------------------------------------------------------- | --------------------------- | ----------------------- |
| [**apiSecurityStatsGet**](SecurityAPI.md#apisecuritystatsget) | **GET** /api/security/stats | Get security statistics |

# **apiSecurityStatsGet**

```swift
    open class func apiSecurityStatsGet(completion: @escaping (_ data: ApiSecurityStatsGet200Response?, _ error: Error?) -> Void)
```

Get security statistics

Retrieve security statistics and events

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// Get security statistics
SecurityAPI.apiSecurityStatsGet() { (response, error) in
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

[**ApiSecurityStatsGet200Response**](ApiSecurityStatsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
