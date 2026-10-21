# ComputeApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                             | HTTP request                               | Description             |
| ---------------------------------------------------------------------------------- | ------------------------------------------ | ----------------------- |
| [**apiComputeProvidersGet**](ComputeApi.md#apiComputeProvidersGet)                 | **GET** /api/compute/providers             | List compute providers  |
| [**apiComputeTasksPost**](ComputeApi.md#apiComputeTasksPost)                       | **POST** /api/compute/tasks                | Submit compute task     |
| [**apiComputeTasksTaskIdStatusGet**](ComputeApi.md#apiComputeTasksTaskIdStatusGet) | **GET** /api/compute/tasks/{taskId}/status | Get compute task status |

## apiComputeProvidersGet

> ApiComputeProvidersGet200Response apiComputeProvidersGet()

List compute providers

Retrieve list of available compute providers

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.ComputeApi;

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

        ComputeApi apiInstance = new ComputeApi(defaultClient);
        try {
            ApiComputeProvidersGet200Response result = apiInstance.apiComputeProvidersGet();
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling ComputeApi#apiComputeProvidersGet");
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

[**ApiComputeProvidersGet200Response**](ApiComputeProvidersGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                              | Response headers |
| ----------- | ---------------------------------------- | ---------------- |
| **200**     | Compute providers retrieved successfully | -                |

## apiComputeTasksPost

> ApiComputeTasksPost200Response apiComputeTasksPost(apiComputeTasksPostRequest)

Submit compute task

Submit a distributed computing task to the network

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.ComputeApi;

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

        ComputeApi apiInstance = new ComputeApi(defaultClient);
        ApiComputeTasksPostRequest apiComputeTasksPostRequest = new ApiComputeTasksPostRequest(); // ApiComputeTasksPostRequest |
        try {
            ApiComputeTasksPost200Response result = apiInstance.apiComputeTasksPost(apiComputeTasksPostRequest);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling ComputeApi#apiComputeTasksPost");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name                           | Type                                                            | Description | Notes |
| ------------------------------ | --------------------------------------------------------------- | ----------- | ----- |
| **apiComputeTasksPostRequest** | [**ApiComputeTasksPostRequest**](ApiComputeTasksPostRequest.md) |             |       |

### Return type

[**ApiComputeTasksPost200Response**](ApiComputeTasksPost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                 | Response headers |
| ----------- | --------------------------- | ---------------- |
| **200**     | Task submitted successfully | -                |
| **400**     | Invalid task request        | -                |

## apiComputeTasksTaskIdStatusGet

> ApiComputeTasksTaskIdStatusGet200Response apiComputeTasksTaskIdStatusGet(taskId)

Get compute task status

Retrieve the status of a specific compute task

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.ComputeApi;

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

        ComputeApi apiInstance = new ComputeApi(defaultClient);
        String taskId = "taskId_example"; // String | Task identifier
        try {
            ApiComputeTasksTaskIdStatusGet200Response result = apiInstance.apiComputeTasksTaskIdStatusGet(taskId);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling ComputeApi#apiComputeTasksTaskIdStatusGet");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name       | Type       | Description     | Notes |
| ---------- | ---------- | --------------- | ----- |
| **taskId** | **String** | Task identifier |       |

### Return type

[**ApiComputeTasksTaskIdStatusGet200Response**](ApiComputeTasksTaskIdStatusGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                        | Response headers |
| ----------- | ---------------------------------- | ---------------- |
| **200**     | Task status retrieved successfully | -                |
| **404**     | Task not found                     | -                |
