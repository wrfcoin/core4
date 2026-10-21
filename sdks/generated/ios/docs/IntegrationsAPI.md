# IntegrationsAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                            | HTTP request                                 | Description                      |
| ------------------------------------------------------------------------------------------------- | -------------------------------------------- | -------------------------------- |
| [**apiIntegrationsLangchainEnablePost**](IntegrationsAPI.md#apiintegrationslangchainenablepost)   | **POST** /api/integrations/langchain/enable  | Enable LangChain integration     |
| [**apiIntegrationsLangchainInsightsGet**](IntegrationsAPI.md#apiintegrationslangchaininsightsget) | **GET** /api/integrations/langchain/insights | Get LangChain insights           |
| [**apiIntegrationsLangchainStatusGet**](IntegrationsAPI.md#apiintegrationslangchainstatusget)     | **GET** /api/integrations/langchain/status   | Get LangChain integration status |

# **apiIntegrationsLangchainEnablePost**

```swift
    open class func apiIntegrationsLangchainEnablePost(completion: @escaping (_ data: ApiIntegrationsLangchainEnablePost200Response?, _ error: Error?) -> Void)
```

Enable LangChain integration

Enable LangChain analytics integration

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// Enable LangChain integration
IntegrationsAPI.apiIntegrationsLangchainEnablePost() { (response, error) in
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

[**ApiIntegrationsLangchainEnablePost200Response**](ApiIntegrationsLangchainEnablePost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiIntegrationsLangchainInsightsGet**

```swift
    open class func apiIntegrationsLangchainInsightsGet(completion: @escaping (_ data: ApiIntegrationsLangchainInsightsGet200Response?, _ error: Error?) -> Void)
```

Get LangChain insights

Retrieve AI-powered insights from LangChain analytics

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// Get LangChain insights
IntegrationsAPI.apiIntegrationsLangchainInsightsGet() { (response, error) in
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

[**ApiIntegrationsLangchainInsightsGet200Response**](ApiIntegrationsLangchainInsightsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiIntegrationsLangchainStatusGet**

```swift
    open class func apiIntegrationsLangchainStatusGet(completion: @escaping (_ data: ApiIntegrationsLangchainStatusGet200Response?, _ error: Error?) -> Void)
```

Get LangChain integration status

Retrieve status of LangChain analytics integration

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// Get LangChain integration status
IntegrationsAPI.apiIntegrationsLangchainStatusGet() { (response, error) in
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

[**ApiIntegrationsLangchainStatusGet200Response**](ApiIntegrationsLangchainStatusGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
