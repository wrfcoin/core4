# AiMlApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                      | HTTP request                 | Description                 |
| ----------------------------------------------------------- | ---------------------------- | --------------------------- |
| [**apiAimlInferencePost**](AiMlApi.md#apiAimlInferencePost) | **POST** /api/aiml/inference | Run AI/ML inference         |
| [**apiAimlModelsGet**](AiMlApi.md#apiAimlModelsGet)         | **GET** /api/aiml/models     | List available AI/ML models |

## apiAimlInferencePost

> ApiAimlInferencePost200Response apiAimlInferencePost(apiAimlInferencePostRequest)

Run AI/ML inference

Execute AI/ML inference on the compute network

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.AiMlApi;

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

        AiMlApi apiInstance = new AiMlApi(defaultClient);
        ApiAimlInferencePostRequest apiAimlInferencePostRequest = new ApiAimlInferencePostRequest(); // ApiAimlInferencePostRequest |
        try {
            ApiAimlInferencePost200Response result = apiInstance.apiAimlInferencePost(apiAimlInferencePostRequest);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling AiMlApi#apiAimlInferencePost");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name                            | Type                                                              | Description | Notes |
| ------------------------------- | ----------------------------------------------------------------- | ----------- | ----- |
| **apiAimlInferencePostRequest** | [**ApiAimlInferencePostRequest**](ApiAimlInferencePostRequest.md) |             |       |

### Return type

[**ApiAimlInferencePost200Response**](ApiAimlInferencePost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                      | Response headers |
| ----------- | -------------------------------- | ---------------- |
| **200**     | Inference completed successfully | -                |

## apiAimlModelsGet

> ApiAimlModelsGet200Response apiAimlModelsGet()

List available AI/ML models

Retrieve list of available AI/ML models for computation

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.AiMlApi;

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

        AiMlApi apiInstance = new AiMlApi(defaultClient);
        try {
            ApiAimlModelsGet200Response result = apiInstance.apiAimlModelsGet();
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling AiMlApi#apiAimlModelsGet");
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

[**ApiAimlModelsGet200Response**](ApiAimlModelsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | AI/ML models retrieved successfully | -                |
