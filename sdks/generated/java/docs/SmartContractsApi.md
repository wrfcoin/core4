# SmartContractsApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                              | HTTP request                         | Description                    |
| ----------------------------------------------------------------------------------- | ------------------------------------ | ------------------------------ |
| [**apiContractsCallPost**](SmartContractsApi.md#apiContractsCallPost)               | **POST** /api/contracts/call         | Call contract function         |
| [**apiContractsDeployPost**](SmartContractsApi.md#apiContractsDeployPost)           | **POST** /api/contracts/deploy       | Deploy smart contract          |
| [**apiContractsEstimateGasPost**](SmartContractsApi.md#apiContractsEstimateGasPost) | **POST** /api/contracts/estimate-gas | Estimate gas for contract call |
| [**apiContractsQueryPost**](SmartContractsApi.md#apiContractsQueryPost)             | **POST** /api/contracts/query        | Query contract state           |

## apiContractsCallPost

> ContractCallResponse apiContractsCallPost(contractCallRequest)

Call contract function

Execute a smart contract function

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.SmartContractsApi;

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

        SmartContractsApi apiInstance = new SmartContractsApi(defaultClient);
        ContractCallRequest contractCallRequest = new ContractCallRequest(); // ContractCallRequest |
        try {
            ContractCallResponse result = apiInstance.apiContractsCallPost(contractCallRequest);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling SmartContractsApi#apiContractsCallPost");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name                    | Type                                              | Description | Notes |
| ----------------------- | ------------------------------------------------- | ----------- | ----- |
| **contractCallRequest** | [**ContractCallRequest**](ContractCallRequest.md) |             |       |

### Return type

[**ContractCallResponse**](ContractCallResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                             | Response headers |
| ----------- | --------------------------------------- | ---------------- |
| **200**     | Contract function executed successfully | -                |

## apiContractsDeployPost

> ContractDeployResponse apiContractsDeployPost(contractDeployRequest)

Deploy smart contract

Deploy a new smart contract to the blockchain

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.SmartContractsApi;

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

        SmartContractsApi apiInstance = new SmartContractsApi(defaultClient);
        ContractDeployRequest contractDeployRequest = new ContractDeployRequest(); // ContractDeployRequest |
        try {
            ContractDeployResponse result = apiInstance.apiContractsDeployPost(contractDeployRequest);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling SmartContractsApi#apiContractsDeployPost");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name                      | Type                                                  | Description | Notes |
| ------------------------- | ----------------------------------------------------- | ----------- | ----- |
| **contractDeployRequest** | [**ContractDeployRequest**](ContractDeployRequest.md) |             |       |

### Return type

[**ContractDeployResponse**](ContractDeployResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | Contract deployed successfully      | -                |
| **400**     | Invalid contract code or parameters | -                |

## apiContractsEstimateGasPost

> ApiContractsEstimateGasPost200Response apiContractsEstimateGasPost(contractCallRequest)

Estimate gas for contract call

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.SmartContractsApi;

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

        SmartContractsApi apiInstance = new SmartContractsApi(defaultClient);
        ContractCallRequest contractCallRequest = new ContractCallRequest(); // ContractCallRequest |
        try {
            ApiContractsEstimateGasPost200Response result = apiInstance.apiContractsEstimateGasPost(contractCallRequest);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling SmartContractsApi#apiContractsEstimateGasPost");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name                    | Type                                              | Description | Notes |
| ----------------------- | ------------------------------------------------- | ----------- | ----- |
| **contractCallRequest** | [**ContractCallRequest**](ContractCallRequest.md) |             |       |

### Return type

[**ApiContractsEstimateGasPost200Response**](ApiContractsEstimateGasPost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description             | Response headers |
| ----------- | ----------------------- | ---------------- |
| **200**     | Gas estimate calculated | -                |

## apiContractsQueryPost

> ContractQueryResponse apiContractsQueryPost(contractQueryRequest)

Query contract state

Query smart contract state without modifying it

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.SmartContractsApi;

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

        SmartContractsApi apiInstance = new SmartContractsApi(defaultClient);
        ContractQueryRequest contractQueryRequest = new ContractQueryRequest(); // ContractQueryRequest |
        try {
            ContractQueryResponse result = apiInstance.apiContractsQueryPost(contractQueryRequest);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling SmartContractsApi#apiContractsQueryPost");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name                     | Type                                                | Description | Notes |
| ------------------------ | --------------------------------------------------- | ----------- | ----- |
| **contractQueryRequest** | [**ContractQueryRequest**](ContractQueryRequest.md) |             |       |

### Return type

[**ContractQueryResponse**](ContractQueryResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | Contract state queried successfully | -                |
