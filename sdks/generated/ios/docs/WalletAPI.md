# WalletAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                          | HTTP request                           | Description                           |
| ------------------------------------------------------------------------------- | -------------------------------------- | ------------------------------------- |
| [**apiWalletGenerateMnemonicPost**](WalletAPI.md#apiwalletgeneratemnemonicpost) | **POST** /api/wallet/generate-mnemonic | Generate mnemonic phrase              |
| [**apiWalletHardwareDevicesGet**](WalletAPI.md#apiwallethardwaredevicesget)     | **GET** /api/wallet/hardware/devices   | List connected hardware wallets       |
| [**apiWalletHardwareSignPost**](WalletAPI.md#apiwallethardwaresignpost)         | **POST** /api/wallet/hardware/sign     | Sign transaction with hardware wallet |
| [**apiWalletValidateMnemonicPost**](WalletAPI.md#apiwalletvalidatemnemonicpost) | **POST** /api/wallet/validate-mnemonic | Validate mnemonic phrase              |

# **apiWalletGenerateMnemonicPost**

```swift
    open class func apiWalletGenerateMnemonicPost(completion: @escaping (_ data: ApiWalletGenerateMnemonicPost200Response?, _ error: Error?) -> Void)
```

Generate mnemonic phrase

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// Generate mnemonic phrase
WalletAPI.apiWalletGenerateMnemonicPost() { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiWalletHardwareDevicesGet**

```swift
    open class func apiWalletHardwareDevicesGet(completion: @escaping (_ data: [HardwareDevice]?, _ error: Error?) -> Void)
```

List connected hardware wallets

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// List connected hardware wallets
WalletAPI.apiWalletHardwareDevicesGet() { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**[HardwareDevice]**](HardwareDevice.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiWalletHardwareSignPost**

```swift
    open class func apiWalletHardwareSignPost(hardwareSignRequest: HardwareSignRequest, completion: @escaping (_ data: HardwareSignResponse?, _ error: Error?) -> Void)
```

Sign transaction with hardware wallet

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let hardwareSignRequest = HardwareSignRequest(deviceId: "deviceId_example", transactionHash: "transactionHash_example", derivationPath: "derivationPath_example", displayOnDevice: false, requireConfirmation: false) // HardwareSignRequest |

// Sign transaction with hardware wallet
WalletAPI.apiWalletHardwareSignPost(hardwareSignRequest: hardwareSignRequest) { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
```

### Parameters

| Name                    | Type                                              | Description | Notes |
| ----------------------- | ------------------------------------------------- | ----------- | ----- |
| **hardwareSignRequest** | [**HardwareSignRequest**](HardwareSignRequest.md) |             |

### Return type

[**HardwareSignResponse**](HardwareSignResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiWalletValidateMnemonicPost**

```swift
    open class func apiWalletValidateMnemonicPost(apiWalletValidateMnemonicPostRequest: ApiWalletValidateMnemonicPostRequest, completion: @escaping (_ data: ApiWalletValidateMnemonicPost200Response?, _ error: Error?) -> Void)
```

Validate mnemonic phrase

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let apiWalletValidateMnemonicPostRequest = _api_wallet_validate_mnemonic_post_request(mnemonic: "mnemonic_example") // ApiWalletValidateMnemonicPostRequest |

// Validate mnemonic phrase
WalletAPI.apiWalletValidateMnemonicPost(apiWalletValidateMnemonicPostRequest: apiWalletValidateMnemonicPostRequest) { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
```

### Parameters

| Name                                     | Type                                                                                | Description | Notes |
| ---------------------------------------- | ----------------------------------------------------------------------------------- | ----------- | ----- |
| **apiWalletValidateMnemonicPostRequest** | [**ApiWalletValidateMnemonicPostRequest**](ApiWalletValidateMnemonicPostRequest.md) |             |

### Return type

[**ApiWalletValidateMnemonicPost200Response**](ApiWalletValidateMnemonicPost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
