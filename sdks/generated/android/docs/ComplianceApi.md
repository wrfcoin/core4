# ComplianceApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                | HTTP request                   | Description           |
| --------------------------------------------------------------------- | ------------------------------ | --------------------- |
| [**apiComplianceStatusGet**](ComplianceApi.md#apiComplianceStatusGet) | **GET** /api/compliance/status | Get compliance status |

<a id="apiComplianceStatusGet"></a>

# **apiComplianceStatusGet**

> ApiComplianceStatusGet200Response apiComplianceStatusGet()

Get compliance status

Retrieve compliance status and certifications

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.ComplianceApi;

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

    ComplianceApi apiInstance = new ComplianceApi(defaultClient);
    try {
      ApiComplianceStatusGet200Response result = apiInstance.apiComplianceStatusGet();
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling ComplianceApi#apiComplianceStatusGet");
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

[**ApiComplianceStatusGet200Response**](ApiComplianceStatusGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                              | Response headers |
| ----------- | ---------------------------------------- | ---------------- |
| **200**     | Compliance status retrieved successfully | -                |
