# ComplianceAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                | HTTP request                   | Description           |
| --------------------------------------------------------------------- | ------------------------------ | --------------------- |
| [**apiComplianceStatusGet**](ComplianceAPI.md#apicompliancestatusget) | **GET** /api/compliance/status | Get compliance status |

# **apiComplianceStatusGet**

```swift
    open class func apiComplianceStatusGet(completion: @escaping (_ data: ApiComplianceStatusGet200Response?, _ error: Error?) -> Void)
```

Get compliance status

Retrieve compliance status and certifications

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// Get compliance status
ComplianceAPI.apiComplianceStatusGet() { (response, error) in
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

[**ApiComplianceStatusGet200Response**](ApiComplianceStatusGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
