# wrfcoin_sdk.WalletApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                  | HTTP request                           | Description                           |
| --------------------------------------------------------------------------------------- | -------------------------------------- | ------------------------------------- |
| [**api_wallet_generate_mnemonic_post**](WalletApi.md#api_wallet_generate_mnemonic_post) | **POST** /api/wallet/generate-mnemonic | Generate mnemonic phrase              |
| [**api_wallet_hardware_devices_get**](WalletApi.md#api_wallet_hardware_devices_get)     | **GET** /api/wallet/hardware/devices   | List connected hardware wallets       |
| [**api_wallet_hardware_sign_post**](WalletApi.md#api_wallet_hardware_sign_post)         | **POST** /api/wallet/hardware/sign     | Sign transaction with hardware wallet |
| [**api_wallet_validate_mnemonic_post**](WalletApi.md#api_wallet_validate_mnemonic_post) | **POST** /api/wallet/validate-mnemonic | Validate mnemonic phrase              |

# **api_wallet_generate_mnemonic_post**

> ApiWalletGenerateMnemonicPost200Response api_wallet_generate_mnemonic_post()

Generate mnemonic phrase

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_wallet_generate_mnemonic_post200_response import ApiWalletGenerateMnemonicPost200Response
from wrfcoin_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to https://api.wrfcoin.com/v1
# See configuration.py for a list of all supported configuration parameters.
configuration = wrfcoin_sdk.Configuration(
    host = "https://api.wrfcoin.com/v1"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure API key authorization: ApiKeyAuth
configuration.api_key['ApiKeyAuth'] = os.environ["API_KEY"]

# Uncomment below to setup prefix (e.g. Bearer) for API key, if needed
# configuration.api_key_prefix['ApiKeyAuth'] = 'Bearer'

# Configure Bearer authorization (JWT): BearerAuth
configuration = wrfcoin_sdk.Configuration(
    access_token = os.environ["BEARER_TOKEN"]
)

# Enter a context with an instance of the API client
with wrfcoin_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = wrfcoin_sdk.WalletApi(api_client)

    try:
        # Generate mnemonic phrase
        api_response = api_instance.api_wallet_generate_mnemonic_post()
        print("The response of WalletApi->api_wallet_generate_mnemonic_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling WalletApi->api_wallet_generate_mnemonic_post: %s\n" % e)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_wallet_hardware_devices_get**

> List[HardwareDevice] api_wallet_hardware_devices_get()

List connected hardware wallets

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.hardware_device import HardwareDevice
from wrfcoin_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to https://api.wrfcoin.com/v1
# See configuration.py for a list of all supported configuration parameters.
configuration = wrfcoin_sdk.Configuration(
    host = "https://api.wrfcoin.com/v1"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure API key authorization: ApiKeyAuth
configuration.api_key['ApiKeyAuth'] = os.environ["API_KEY"]

# Uncomment below to setup prefix (e.g. Bearer) for API key, if needed
# configuration.api_key_prefix['ApiKeyAuth'] = 'Bearer'

# Configure Bearer authorization (JWT): BearerAuth
configuration = wrfcoin_sdk.Configuration(
    access_token = os.environ["BEARER_TOKEN"]
)

# Enter a context with an instance of the API client
with wrfcoin_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = wrfcoin_sdk.WalletApi(api_client)

    try:
        # List connected hardware wallets
        api_response = api_instance.api_wallet_hardware_devices_get()
        print("The response of WalletApi->api_wallet_hardware_devices_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling WalletApi->api_wallet_hardware_devices_get: %s\n" % e)
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**List[HardwareDevice]**](HardwareDevice.md)

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

# **api_wallet_hardware_sign_post**

> HardwareSignResponse api_wallet_hardware_sign_post(hardware_sign_request)

Sign transaction with hardware wallet

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.hardware_sign_request import HardwareSignRequest
from wrfcoin_sdk.models.hardware_sign_response import HardwareSignResponse
from wrfcoin_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to https://api.wrfcoin.com/v1
# See configuration.py for a list of all supported configuration parameters.
configuration = wrfcoin_sdk.Configuration(
    host = "https://api.wrfcoin.com/v1"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure API key authorization: ApiKeyAuth
configuration.api_key['ApiKeyAuth'] = os.environ["API_KEY"]

# Uncomment below to setup prefix (e.g. Bearer) for API key, if needed
# configuration.api_key_prefix['ApiKeyAuth'] = 'Bearer'

# Configure Bearer authorization (JWT): BearerAuth
configuration = wrfcoin_sdk.Configuration(
    access_token = os.environ["BEARER_TOKEN"]
)

# Enter a context with an instance of the API client
with wrfcoin_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = wrfcoin_sdk.WalletApi(api_client)
    hardware_sign_request = wrfcoin_sdk.HardwareSignRequest() # HardwareSignRequest |

    try:
        # Sign transaction with hardware wallet
        api_response = api_instance.api_wallet_hardware_sign_post(hardware_sign_request)
        print("The response of WalletApi->api_wallet_hardware_sign_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling WalletApi->api_wallet_hardware_sign_post: %s\n" % e)
```

### Parameters

| Name                      | Type                                              | Description | Notes |
| ------------------------- | ------------------------------------------------- | ----------- | ----- |
| **hardware_sign_request** | [**HardwareSignRequest**](HardwareSignRequest.md) |             |

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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_wallet_validate_mnemonic_post**

> ApiWalletValidateMnemonicPost200Response
> api_wallet_validate_mnemonic_post(api_wallet_validate_mnemonic_post_request)

Validate mnemonic phrase

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_wallet_validate_mnemonic_post200_response import ApiWalletValidateMnemonicPost200Response
from wrfcoin_sdk.models.api_wallet_validate_mnemonic_post_request import ApiWalletValidateMnemonicPostRequest
from wrfcoin_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to https://api.wrfcoin.com/v1
# See configuration.py for a list of all supported configuration parameters.
configuration = wrfcoin_sdk.Configuration(
    host = "https://api.wrfcoin.com/v1"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure API key authorization: ApiKeyAuth
configuration.api_key['ApiKeyAuth'] = os.environ["API_KEY"]

# Uncomment below to setup prefix (e.g. Bearer) for API key, if needed
# configuration.api_key_prefix['ApiKeyAuth'] = 'Bearer'

# Configure Bearer authorization (JWT): BearerAuth
configuration = wrfcoin_sdk.Configuration(
    access_token = os.environ["BEARER_TOKEN"]
)

# Enter a context with an instance of the API client
with wrfcoin_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = wrfcoin_sdk.WalletApi(api_client)
    api_wallet_validate_mnemonic_post_request = wrfcoin_sdk.ApiWalletValidateMnemonicPostRequest() # ApiWalletValidateMnemonicPostRequest |

    try:
        # Validate mnemonic phrase
        api_response = api_instance.api_wallet_validate_mnemonic_post(api_wallet_validate_mnemonic_post_request)
        print("The response of WalletApi->api_wallet_validate_mnemonic_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling WalletApi->api_wallet_validate_mnemonic_post: %s\n" % e)
```

### Parameters

| Name                                          | Type                                                                                | Description | Notes |
| --------------------------------------------- | ----------------------------------------------------------------------------------- | ----------- | ----- |
| **api_wallet_validate_mnemonic_post_request** | [**ApiWalletValidateMnemonicPostRequest**](ApiWalletValidateMnemonicPostRequest.md) |             |

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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
