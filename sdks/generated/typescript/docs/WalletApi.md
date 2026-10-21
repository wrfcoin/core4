# WalletApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                              | HTTP request                           | Description                           |
| ------------------------------------------------------------------- | -------------------------------------- | ------------------------------------- |
| [**apiWalletGenerateMnemonicPost**](#apiwalletgeneratemnemonicpost) | **POST** /api/wallet/generate-mnemonic | Generate mnemonic phrase              |
| [**apiWalletHardwareDevicesGet**](#apiwallethardwaredevicesget)     | **GET** /api/wallet/hardware/devices   | List connected hardware wallets       |
| [**apiWalletHardwareSignPost**](#apiwallethardwaresignpost)         | **POST** /api/wallet/hardware/sign     | Sign transaction with hardware wallet |
| [**apiWalletValidateMnemonicPost**](#apiwalletvalidatemnemonicpost) | **POST** /api/wallet/validate-mnemonic | Validate mnemonic phrase              |

# **apiWalletGenerateMnemonicPost**

> ApiWalletGenerateMnemonicPost200Response apiWalletGenerateMnemonicPost()

### Example

```typescript
import { WalletApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new WalletApi(configuration);

const { status, data } = await apiInstance.apiWalletGenerateMnemonicPost();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**ApiWalletGenerateMnemonicPost200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                     | Response headers |
| ----------- | ------------------------------- | ---------------- |
| **200**     | Mnemonic generated successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiWalletHardwareDevicesGet**

> Array<HardwareDevice> apiWalletHardwareDevicesGet()

### Example

```typescript
import { WalletApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new WalletApi(configuration);

const { status, data } = await apiInstance.apiWalletHardwareDevicesGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**Array<HardwareDevice>**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description             | Response headers |
| ----------- | ----------------------- | ---------------- |
| **200**     | Hardware devices listed | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiWalletHardwareSignPost**

> HardwareSignResponse apiWalletHardwareSignPost(hardwareSignRequest)

### Example

```typescript
import { WalletApi, Configuration, HardwareSignRequest } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new WalletApi(configuration);

let hardwareSignRequest: HardwareSignRequest; //

const { status, data } = await apiInstance.apiWalletHardwareSignPost(hardwareSignRequest);
```

### Parameters

| Name                    | Type                    | Description | Notes |
| ----------------------- | ----------------------- | ----------- | ----- |
| **hardwareSignRequest** | **HardwareSignRequest** |             |       |

### Return type

**HardwareSignResponse**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                     | Response headers |
| ----------- | ------------------------------- | ---------------- |
| **200**     | Transaction signed successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiWalletValidateMnemonicPost**

> ApiWalletValidateMnemonicPost200Response
> apiWalletValidateMnemonicPost(apiWalletValidateMnemonicPostRequest)

### Example

```typescript
import {
  WalletApi,
  Configuration,
  ApiWalletValidateMnemonicPostRequest,
} from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new WalletApi(configuration);

let apiWalletValidateMnemonicPostRequest: ApiWalletValidateMnemonicPostRequest; //

const { status, data } = await apiInstance.apiWalletValidateMnemonicPost(
  apiWalletValidateMnemonicPostRequest
);
```

### Parameters

| Name                                     | Type                                     | Description | Notes |
| ---------------------------------------- | ---------------------------------------- | ----------- | ----- |
| **apiWalletValidateMnemonicPostRequest** | **ApiWalletValidateMnemonicPostRequest** |             |       |

### Return type

**ApiWalletValidateMnemonicPost200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                | Response headers |
| ----------- | -------------------------- | ---------------- |
| **200**     | Mnemonic validation result | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
