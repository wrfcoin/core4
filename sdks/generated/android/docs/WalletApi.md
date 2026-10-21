# WalletApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                          | HTTP request                           | Description                           |
| ------------------------------------------------------------------------------- | -------------------------------------- | ------------------------------------- |
| [**apiWalletGenerateMnemonicPost**](WalletApi.md#apiWalletGenerateMnemonicPost) | **POST** /api/wallet/generate-mnemonic | Generate mnemonic phrase              |
| [**apiWalletHardwareDevicesGet**](WalletApi.md#apiWalletHardwareDevicesGet)     | **GET** /api/wallet/hardware/devices   | List connected hardware wallets       |
| [**apiWalletHardwareSignPost**](WalletApi.md#apiWalletHardwareSignPost)         | **POST** /api/wallet/hardware/sign     | Sign transaction with hardware wallet |
| [**apiWalletValidateMnemonicPost**](WalletApi.md#apiWalletValidateMnemonicPost) | **POST** /api/wallet/validate-mnemonic | Validate mnemonic phrase              |

<a id="apiWalletGenerateMnemonicPost"></a>

# **apiWalletGenerateMnemonicPost**

> ApiWalletGenerateMnemonicPost200Response apiWalletGenerateMnemonicPost()

Generate mnemonic phrase

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.WalletApi;

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

    WalletApi apiInstance = new WalletApi(defaultClient);
    try {
      ApiWalletGenerateMnemonicPost200Response result = apiInstance.apiWalletGenerateMnemonicPost();
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling WalletApi#apiWalletGenerateMnemonicPost");
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

[**ApiWalletGenerateMnemonicPost200Response**](ApiWalletGenerateMnemonicPost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                     | Response headers |
| ----------- | ------------------------------- | ---------------- |
| **200**     | Mnemonic generated successfully | -                |

<a id="apiWalletHardwareDevicesGet"></a>

# **apiWalletHardwareDevicesGet**

> List&lt;HardwareDevice&gt; apiWalletHardwareDevicesGet()

List connected hardware wallets

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.WalletApi;

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

    WalletApi apiInstance = new WalletApi(defaultClient);
    try {
      List<HardwareDevice> result = apiInstance.apiWalletHardwareDevicesGet();
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling WalletApi#apiWalletHardwareDevicesGet");
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

[**List&lt;HardwareDevice&gt;**](HardwareDevice.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description             | Response headers |
| ----------- | ----------------------- | ---------------- |
| **200**     | Hardware devices listed | -                |

<a id="apiWalletHardwareSignPost"></a>

# **apiWalletHardwareSignPost**

> HardwareSignResponse apiWalletHardwareSignPost(hardwareSignRequest)

Sign transaction with hardware wallet

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.WalletApi;

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

    WalletApi apiInstance = new WalletApi(defaultClient);
    HardwareSignRequest hardwareSignRequest = new HardwareSignRequest(); // HardwareSignRequest |
    try {
      HardwareSignResponse result = apiInstance.apiWalletHardwareSignPost(hardwareSignRequest);
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling WalletApi#apiWalletHardwareSignPost");
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
| **hardwareSignRequest** | [**HardwareSignRequest**](HardwareSignRequest.md) |             |       |

### Return type

[**HardwareSignResponse**](HardwareSignResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                     | Response headers |
| ----------- | ------------------------------- | ---------------- |
| **200**     | Transaction signed successfully | -                |

<a id="apiWalletValidateMnemonicPost"></a>

# **apiWalletValidateMnemonicPost**

> ApiWalletValidateMnemonicPost200Response
> apiWalletValidateMnemonicPost(apiWalletValidateMnemonicPostRequest)

Validate mnemonic phrase

### Example

```java
// Import classes:
import com.wrfcoin.android.sdk.ApiClient;
import com.wrfcoin.android.sdk.ApiException;
import com.wrfcoin.android.sdk.Configuration;
import com.wrfcoin.android.sdk.auth.*;
import com.wrfcoin.android.sdk.models.*;
import com.wrfcoin.android.sdk.api.WalletApi;

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

    WalletApi apiInstance = new WalletApi(defaultClient);
    ApiWalletValidateMnemonicPostRequest apiWalletValidateMnemonicPostRequest = new ApiWalletValidateMnemonicPostRequest(); // ApiWalletValidateMnemonicPostRequest |
    try {
      ApiWalletValidateMnemonicPost200Response result = apiInstance.apiWalletValidateMnemonicPost(apiWalletValidateMnemonicPostRequest);
      System.out.println(result);
    } catch (ApiException e) {
      System.err.println("Exception when calling WalletApi#apiWalletValidateMnemonicPost");
      System.err.println("Status code: " + e.getCode());
      System.err.println("Reason: " + e.getResponseBody());
      System.err.println("Response headers: " + e.getResponseHeaders());
      e.printStackTrace();
    }
  }
}
```

### Parameters

| Name                                     | Type                                                                                | Description | Notes |
| ---------------------------------------- | ----------------------------------------------------------------------------------- | ----------- | ----- |
| **apiWalletValidateMnemonicPostRequest** | [**ApiWalletValidateMnemonicPostRequest**](ApiWalletValidateMnemonicPostRequest.md) |             |       |

### Return type

[**ApiWalletValidateMnemonicPost200Response**](ApiWalletValidateMnemonicPost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                | Response headers |
| ----------- | -------------------------- | ---------------- |
| **200**     | Mnemonic validation result | -                |
