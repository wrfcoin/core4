# IntegrationsApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                            | HTTP request                                 | Description                      |
| ------------------------------------------------------------------------------------------------- | -------------------------------------------- | -------------------------------- |
| [**apiIntegrationsLangchainEnablePost**](IntegrationsApi.md#apiIntegrationsLangchainEnablePost)   | **POST** /api/integrations/langchain/enable  | Enable LangChain integration     |
| [**apiIntegrationsLangchainInsightsGet**](IntegrationsApi.md#apiIntegrationsLangchainInsightsGet) | **GET** /api/integrations/langchain/insights | Get LangChain insights           |
| [**apiIntegrationsLangchainStatusGet**](IntegrationsApi.md#apiIntegrationsLangchainStatusGet)     | **GET** /api/integrations/langchain/status   | Get LangChain integration status |

<a id="apiIntegrationsLangchainEnablePost"></a>

# **apiIntegrationsLangchainEnablePost**

> ApiIntegrationsLangchainEnablePost200Response apiIntegrationsLangchainEnablePost()

Enable LangChain integration

Enable LangChain analytics integration

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.IntegrationsApi;

public class Example {
  public static void main(String[] args) {
    ApiClient defaultClient = Configuration.getDefaultApiClient();
    defaultClient.setBasePath("https://api.wrfcoin.com/v1");

    // Configure API key authorization: ApiKeyAuth
    ApiKeyAuth ApiKeyAuth = (ApiKeyAuth) defaultClient.getAuthentication("ApiKeyAuth");
    ApiKeyAuth.setApiKey("YOUR API KEY");
    // Uncomment the following line to set a prefix for the API key, e.g. "Token" (defaults to null)
    //ApiKeyAuth.setApiKeyPrefix("Token");

    // Configure HTTP bearer authorization: BearerAuth
    HttpBearerAuth BearerAuth = (HttpBearerAuth) defaultClient.getAuthentication("BearerAuth");
    BearerAuth.setBearerToken("BEARER TOKEN");

    IntegrationsApi apiInstance = new IntegrationsApi(defaultClient);
    try {
      ApiIntegrationsLangchainEnablePost200Response result = apiInstance.apiIntegrationsLangchainEnablePost();
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling IntegrationsApi#apiIntegrationsLangchainEnablePost");
      System.err.println("Status code: " + e.getCode());
      System.err.println("Reason: " + e.getResponseBody());
      System.err.println("Response headers: " + e.getResponseHeaders());
      e.printStackTrace();
    }
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

### HTTP response details

| Status code | Description                                | Response headers |
| ----------- | ------------------------------------------ | ---------------- |
| **200**     | LangChain integration enabled successfully | -                |

<a id="apiIntegrationsLangchainInsightsGet"></a>

# **apiIntegrationsLangchainInsightsGet**

> ApiIntegrationsLangchainInsightsGet200Response apiIntegrationsLangchainInsightsGet()

Get LangChain insights

Retrieve AI-powered insights from LangChain analytics

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.IntegrationsApi;

public class Example {
  public static void main(String[] args) {
    ApiClient defaultClient = Configuration.getDefaultApiClient();
    defaultClient.setBasePath("https://api.wrfcoin.com/v1");

    // Configure API key authorization: ApiKeyAuth
    ApiKeyAuth ApiKeyAuth = (ApiKeyAuth) defaultClient.getAuthentication("ApiKeyAuth");
    ApiKeyAuth.setApiKey("YOUR API KEY");
    // Uncomment the following line to set a prefix for the API key, e.g. "Token" (defaults to null)
    //ApiKeyAuth.setApiKeyPrefix("Token");

    // Configure HTTP bearer authorization: BearerAuth
    HttpBearerAuth BearerAuth = (HttpBearerAuth) defaultClient.getAuthentication("BearerAuth");
    BearerAuth.setBearerToken("BEARER TOKEN");

    IntegrationsApi apiInstance = new IntegrationsApi(defaultClient);
    try {
      ApiIntegrationsLangchainInsightsGet200Response result = apiInstance.apiIntegrationsLangchainInsightsGet();
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling IntegrationsApi#apiIntegrationsLangchainInsightsGet");
      System.err.println("Status code: " + e.getCode());
      System.err.println("Reason: " + e.getResponseBody());
      System.err.println("Response headers: " + e.getResponseHeaders());
      e.printStackTrace();
    }
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

### HTTP response details

| Status code | Description                               | Response headers |
| ----------- | ----------------------------------------- | ---------------- |
| **200**     | LangChain insights retrieved successfully | -                |

<a id="apiIntegrationsLangchainStatusGet"></a>

# **apiIntegrationsLangchainStatusGet**

> ApiIntegrationsLangchainStatusGet200Response apiIntegrationsLangchainStatusGet()

Get LangChain integration status

Retrieve status of LangChain analytics integration

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.IntegrationsApi;

public class Example {
  public static void main(String[] args) {
    ApiClient defaultClient = Configuration.getDefaultApiClient();
    defaultClient.setBasePath("https://api.wrfcoin.com/v1");

    // Configure API key authorization: ApiKeyAuth
    ApiKeyAuth ApiKeyAuth = (ApiKeyAuth) defaultClient.getAuthentication("ApiKeyAuth");
    ApiKeyAuth.setApiKey("YOUR API KEY");
    // Uncomment the following line to set a prefix for the API key, e.g. "Token" (defaults to null)
    //ApiKeyAuth.setApiKeyPrefix("Token");

    // Configure HTTP bearer authorization: BearerAuth
    HttpBearerAuth BearerAuth = (HttpBearerAuth) defaultClient.getAuthentication("BearerAuth");
    BearerAuth.setBearerToken("BEARER TOKEN");

    IntegrationsApi apiInstance = new IntegrationsApi(defaultClient);
    try {
      ApiIntegrationsLangchainStatusGet200Response result = apiInstance.apiIntegrationsLangchainStatusGet();
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling IntegrationsApi#apiIntegrationsLangchainStatusGet");
      System.err.println("Status code: " + e.getCode());
      System.err.println("Reason: " + e.getResponseBody());
      System.err.println("Response headers: " + e.getResponseHeaders());
      e.printStackTrace();
    }
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

### HTTP response details

| Status code | Description                                         | Response headers |
| ----------- | --------------------------------------------------- | ---------------- |
| **200**     | LangChain integration status retrieved successfully | -                |
