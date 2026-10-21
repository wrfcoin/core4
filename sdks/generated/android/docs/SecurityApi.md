# SecurityApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                        | HTTP request                | Description             |
| ------------------------------------------------------------- | --------------------------- | ----------------------- |
| [**apiSecurityStatsGet**](SecurityApi.md#apiSecurityStatsGet) | **GET** /api/security/stats | Get security statistics |

<a id="apiSecurityStatsGet"></a>

# **apiSecurityStatsGet**

> ApiSecurityStatsGet200Response apiSecurityStatsGet()

Get security statistics

Retrieve security statistics and events

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.SecurityApi;

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

    SecurityApi apiInstance = new SecurityApi(defaultClient);
    try {
      ApiSecurityStatsGet200Response result = apiInstance.apiSecurityStatsGet();
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling SecurityApi#apiSecurityStatsGet");
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

[**ApiSecurityStatsGet200Response**](ApiSecurityStatsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                | Response headers |
| ----------- | ------------------------------------------ | ---------------- |
| **200**     | Security statistics retrieved successfully | -                |
