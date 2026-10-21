# MiningApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                      | HTTP request                | Description        |
| ----------------------------------------------------------- | --------------------------- | ------------------ |
| [**apiMiningRewardsGet**](MiningApi.md#apiMiningRewardsGet) | **GET** /api/mining/rewards | Get mining rewards |

<a id="apiMiningRewardsGet"></a>

# **apiMiningRewardsGet**

> ApiMiningRewardsGet200Response apiMiningRewardsGet(address, type, limit)

Get mining rewards

Retrieve mining rewards for a specific address

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.MiningApi;

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

    MiningApi apiInstance = new MiningApi(defaultClient);
    String address = "address_example"; // String | Wallet address to get rewards for
    String type = "weather_data"; // String | Type of mining reward
    Integer limit = 10; // Integer | Number of rewards to return
    try {
      ApiMiningRewardsGet200Response result = apiInstance.apiMiningRewardsGet(address, type, limit);
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling MiningApi#apiMiningRewardsGet");
      System.err.println("Status code: " + e.getCode());
      System.err.println("Reason: " + e.getResponseBody());
      System.err.println("Response headers: " + e.getResponseHeaders());
      e.printStackTrace();
    }
  }
}
```

### Parameters

| Name        | Type        | Description                       | Notes                                                         |
| ----------- | ----------- | --------------------------------- | ------------------------------------------------------------- |
| **address** | **String**  | Wallet address to get rewards for | [optional]                                                    |
| **type**    | **String**  | Type of mining reward             | [optional] [enum: weather_data, compute, validation, staking] |
| **limit**   | **Integer** | Number of rewards to return       | [optional] [default to 10]                                    |

### Return type

[**ApiMiningRewardsGet200Response**](ApiMiningRewardsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                           | Response headers |
| ----------- | ------------------------------------- | ---------------- |
| **200**     | Mining rewards retrieved successfully | -                |
