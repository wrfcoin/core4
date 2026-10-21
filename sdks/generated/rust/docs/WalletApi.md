# \WalletApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                  | HTTP request                           | Description                           |
| --------------------------------------------------------------------------------------- | -------------------------------------- | ------------------------------------- |
| [**api_wallet_generate_mnemonic_post**](WalletApi.md#api_wallet_generate_mnemonic_post) | **POST** /api/wallet/generate-mnemonic | Generate mnemonic phrase              |
| [**api_wallet_hardware_devices_get**](WalletApi.md#api_wallet_hardware_devices_get)     | **GET** /api/wallet/hardware/devices   | List connected hardware wallets       |
| [**api_wallet_hardware_sign_post**](WalletApi.md#api_wallet_hardware_sign_post)         | **POST** /api/wallet/hardware/sign     | Sign transaction with hardware wallet |
| [**api_wallet_validate_mnemonic_post**](WalletApi.md#api_wallet_validate_mnemonic_post) | **POST** /api/wallet/validate-mnemonic | Validate mnemonic phrase              |

## api_wallet_generate_mnemonic_post

> models::ApiWalletGenerateMnemonicPost200Response api_wallet_generate_mnemonic_post() Generate
> mnemonic phrase

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::ApiWalletGenerateMnemonicPost200Response**](_api_wallet_generate_mnemonic_post_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_wallet_hardware_devices_get

> Vec<models::HardwareDevice> api_wallet_hardware_devices_get() List connected hardware wallets

### Parameters

This endpoint does not need any parameter.

### Return type

[**Vec<models::HardwareDevice>**](HardwareDevice.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_wallet_hardware_sign_post

> models::HardwareSignResponse api_wallet_hardware_sign_post(hardware_sign_request) Sign transaction
> with hardware wallet

### Parameters

| Name                      | Type                                              | Description | Required   | Notes |
| ------------------------- | ------------------------------------------------- | ----------- | ---------- | ----- |
| **hardware_sign_request** | [**HardwareSignRequest**](HardwareSignRequest.md) |             | [required] |

### Return type

[**models::HardwareSignResponse**](HardwareSignResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_wallet_validate_mnemonic_post

> models::ApiWalletValidateMnemonicPost200Response
> api_wallet_validate_mnemonic_post(api_wallet_validate_mnemonic_post_request) Validate mnemonic
> phrase

### Parameters

| Name                                          | Type                                                                                | Description | Required   | Notes |
| --------------------------------------------- | ----------------------------------------------------------------------------------- | ----------- | ---------- | ----- |
| **api_wallet_validate_mnemonic_post_request** | [**ApiWalletValidateMnemonicPostRequest**](ApiWalletValidateMnemonicPostRequest.md) |             | [required] |

### Return type

[**models::ApiWalletValidateMnemonicPost200Response**](_api_wallet_validate_mnemonic_post_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
