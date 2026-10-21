# wrfcoin_sdk.BlockchainApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                                  | HTTP request                                  | Description             |
| ------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ----------------------- |
| [**api_blockchain_blocks_block_hash_get**](BlockchainApi.md#api_blockchain_blocks_block_hash_get)       | **GET** /api/blockchain/blocks/{blockHash}    | Get block by hash       |
| [**api_blockchain_blocks_get**](BlockchainApi.md#api_blockchain_blocks_get)                             | **GET** /api/blockchain/blocks                | Get blockchain blocks   |
| [**api_blockchain_status_get**](BlockchainApi.md#api_blockchain_status_get)                             | **GET** /api/blockchain/status                | Get blockchain status   |
| [**api_blockchain_transactions_tx_hash_get**](BlockchainApi.md#api_blockchain_transactions_tx_hash_get) | **GET** /api/blockchain/transactions/{txHash} | Get transaction by hash |

# **api_blockchain_blocks_block_hash_get**

> Block api_blockchain_blocks_block_hash_get(block_hash)

Get block by hash

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.block import Block
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
    api_instance = wrfcoin_sdk.BlockchainApi(api_client)
    block_hash = 'block_hash_example' # str |

    try:
        # Get block by hash
        api_response = api_instance.api_blockchain_blocks_block_hash_get(block_hash)
        print("The response of BlockchainApi->api_blockchain_blocks_block_hash_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling BlockchainApi->api_blockchain_blocks_block_hash_get: %s\n" % e)
```

### Parameters

| Name           | Type    | Description | Notes |
| -------------- | ------- | ----------- | ----- |
| **block_hash** | **str** |             |

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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_blockchain_blocks_get**

> ApiBlockchainBlocksGet200Response api_blockchain_blocks_get(limit=limit, offset=offset,
> order=order)

Get blockchain blocks

Retrieve a list of blockchain blocks with pagination

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_blockchain_blocks_get200_response import ApiBlockchainBlocksGet200Response
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
    api_instance = wrfcoin_sdk.BlockchainApi(api_client)
    limit = 10 # int | Number of blocks to return (optional) (default to 10)
    offset = 0 # int | Number of blocks to skip (optional) (default to 0)
    order = desc # str | Sort order (optional) (default to desc)

    try:
        # Get blockchain blocks
        api_response = api_instance.api_blockchain_blocks_get(limit=limit, offset=offset, order=order)
        print("The response of BlockchainApi->api_blockchain_blocks_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling BlockchainApi->api_blockchain_blocks_get: %s\n" % e)
```

### Parameters

| Name       | Type    | Description                | Notes                        |
| ---------- | ------- | -------------------------- | ---------------------------- |
| **limit**  | **int** | Number of blocks to return | [optional] [default to 10]   |
| **offset** | **int** | Number of blocks to skip   | [optional] [default to 0]    |
| **order**  | **str** | Sort order                 | [optional] [default to desc] |

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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_blockchain_status_get**

> BlockchainStatus api_blockchain_status_get()

Get blockchain status

Returns current blockchain status including block height, network health, and consensus state

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.blockchain_status import BlockchainStatus
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
    api_instance = wrfcoin_sdk.BlockchainApi(api_client)

    try:
        # Get blockchain status
        api_response = api_instance.api_blockchain_status_get()
        print("The response of BlockchainApi->api_blockchain_status_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling BlockchainApi->api_blockchain_status_get: %s\n" % e)
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_blockchain_transactions_tx_hash_get**

> Transaction api_blockchain_transactions_tx_hash_get(tx_hash)

Get transaction by hash

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.transaction import Transaction
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
    api_instance = wrfcoin_sdk.BlockchainApi(api_client)
    tx_hash = 'tx_hash_example' # str |

    try:
        # Get transaction by hash
        api_response = api_instance.api_blockchain_transactions_tx_hash_get(tx_hash)
        print("The response of BlockchainApi->api_blockchain_transactions_tx_hash_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling BlockchainApi->api_blockchain_transactions_tx_hash_get: %s\n" % e)
```

### Parameters

| Name        | Type    | Description | Notes |
| ----------- | ------- | ----------- | ----- |
| **tx_hash** | **str** |             |

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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
