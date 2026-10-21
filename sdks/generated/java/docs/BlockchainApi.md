# BlockchainApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                        | HTTP request                                  | Description             |
| --------------------------------------------------------------------------------------------- | --------------------------------------------- | ----------------------- |
| [**apiBlockchainBlocksBlockHashGet**](BlockchainApi.md#apiBlockchainBlocksBlockHashGet)       | **GET** /api/blockchain/blocks/{blockHash}    | Get block by hash       |
| [**apiBlockchainBlocksGet**](BlockchainApi.md#apiBlockchainBlocksGet)                         | **GET** /api/blockchain/blocks                | Get blockchain blocks   |
| [**apiBlockchainStatusGet**](BlockchainApi.md#apiBlockchainStatusGet)                         | **GET** /api/blockchain/status                | Get blockchain status   |
| [**apiBlockchainTransactionsTxHashGet**](BlockchainApi.md#apiBlockchainTransactionsTxHashGet) | **GET** /api/blockchain/transactions/{txHash} | Get transaction by hash |

## apiBlockchainBlocksBlockHashGet

> Block apiBlockchainBlocksBlockHashGet(blockHash)

Get block by hash

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.BlockchainApi;

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

        BlockchainApi apiInstance = new BlockchainApi(defaultClient);
        String blockHash = "blockHash_example"; // String |
        try {
            Block result = apiInstance.apiBlockchainBlocksBlockHashGet(blockHash);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling BlockchainApi#apiBlockchainBlocksBlockHashGet");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name          | Type       | Description | Notes |
| ------------- | ---------- | ----------- | ----- |
| **blockHash** | **String** |             |       |

### Return type

[**Block**](Block.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                  | Response headers |
| ----------- | ---------------------------- | ---------------- |
| **200**     | Block retrieved successfully | -                |
| **404**     | Block not found              | -                |

## apiBlockchainBlocksGet

> ApiBlockchainBlocksGet200Response apiBlockchainBlocksGet(limit, offset, order)

Get blockchain blocks

Retrieve a list of blockchain blocks with pagination

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.BlockchainApi;

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

        BlockchainApi apiInstance = new BlockchainApi(defaultClient);
        Integer limit = 10; // Integer | Number of blocks to return
        Integer offset = 0; // Integer | Number of blocks to skip
        String order = "asc"; // String | Sort order
        try {
            ApiBlockchainBlocksGet200Response result = apiInstance.apiBlockchainBlocksGet(limit, offset, order);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling BlockchainApi#apiBlockchainBlocksGet");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name       | Type        | Description                | Notes                                          |
| ---------- | ----------- | -------------------------- | ---------------------------------------------- |
| **limit**  | **Integer** | Number of blocks to return | [optional] [default to 10]                     |
| **offset** | **Integer** | Number of blocks to skip   | [optional] [default to 0]                      |
| **order**  | **String**  | Sort order                 | [optional] [default to desc] [enum: asc, desc] |

### Return type

[**ApiBlockchainBlocksGet200Response**](ApiBlockchainBlocksGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                   | Response headers |
| ----------- | ----------------------------- | ---------------- |
| **200**     | Blocks retrieved successfully | -                |

## apiBlockchainStatusGet

> BlockchainStatus apiBlockchainStatusGet()

Get blockchain status

Returns current blockchain status including block height, network health, and consensus state

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.BlockchainApi;

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

        BlockchainApi apiInstance = new BlockchainApi(defaultClient);
        try {
            BlockchainStatus result = apiInstance.apiBlockchainStatusGet();
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling BlockchainApi#apiBlockchainStatusGet");
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

[**BlockchainStatus**](BlockchainStatus.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                              | Response headers |
| ----------- | ---------------------------------------- | ---------------- |
| **200**     | Blockchain status retrieved successfully | -                |
| **500**     | Internal server error                    | -                |

## apiBlockchainTransactionsTxHashGet

> Transaction apiBlockchainTransactionsTxHashGet(txHash)

Get transaction by hash

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.BlockchainApi;

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

        BlockchainApi apiInstance = new BlockchainApi(defaultClient);
        String txHash = "txHash_example"; // String |
        try {
            Transaction result = apiInstance.apiBlockchainTransactionsTxHashGet(txHash);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling BlockchainApi#apiBlockchainTransactionsTxHashGet");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name       | Type       | Description | Notes |
| ---------- | ---------- | ----------- | ----- |
| **txHash** | **String** |             |       |

### Return type

[**Transaction**](Transaction.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                        | Response headers |
| ----------- | ---------------------------------- | ---------------- |
| **200**     | Transaction retrieved successfully | -                |
