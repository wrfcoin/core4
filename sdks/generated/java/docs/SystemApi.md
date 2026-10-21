# SystemApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                        | HTTP request        | Description         |
| --------------------------------------------- | ------------------- | ------------------- |
| [**apiHealthGet**](SystemApi.md#apiHealthGet) | **GET** /api/health | API health check    |
| [**healthGet**](SystemApi.md#healthGet)       | **GET** /health     | System health check |

## apiHealthGet

> HealthGet200Response apiHealthGet()

API health check

Get comprehensive API health status

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.SystemApi;

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

        SystemApi apiInstance = new SystemApi(defaultClient);
        try {
            HealthGet200Response result = apiInstance.apiHealthGet();
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling SystemApi#apiHealthGet");
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

[**HealthGet200Response**](HealthGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                              | Response headers |
| ----------- | ---------------------------------------- | ---------------- |
| **200**     | API health status retrieved successfully | -                |

## healthGet

> HealthGet200Response healthGet()

System health check

Get comprehensive system health status

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.SystemApi;

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

        SystemApi apiInstance = new SystemApi(defaultClient);
        try {
            HealthGet200Response result = apiInstance.healthGet();
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling SystemApi#healthGet");
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

[**HealthGet200Response**](HealthGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                 | Response headers |
| ----------- | ------------------------------------------- | ---------------- |
| **200**     | System health status retrieved successfully | -                |
