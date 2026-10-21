# \BlockchainApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                                  | HTTP request                                  | Description             |
| ------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ----------------------- |
| [**api_blockchain_blocks_block_hash_get**](BlockchainApi.md#api_blockchain_blocks_block_hash_get)       | **GET** /api/blockchain/blocks/{blockHash}    | Get block by hash       |
| [**api_blockchain_blocks_get**](BlockchainApi.md#api_blockchain_blocks_get)                             | **GET** /api/blockchain/blocks                | Get blockchain blocks   |
| [**api_blockchain_status_get**](BlockchainApi.md#api_blockchain_status_get)                             | **GET** /api/blockchain/status                | Get blockchain status   |
| [**api_blockchain_transactions_tx_hash_get**](BlockchainApi.md#api_blockchain_transactions_tx_hash_get) | **GET** /api/blockchain/transactions/{txHash} | Get transaction by hash |

## api_blockchain_blocks_block_hash_get

> models::Block api_blockchain_blocks_block_hash_get(block_hash) Get block by hash

### Parameters

| Name           | Type       | Description | Required   | Notes |
| -------------- | ---------- | ----------- | ---------- | ----- |
| **block_hash** | **String** |             | [required] |

### Return type

[**models::Block**](Block.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_blockchain_blocks_get

> models::ApiBlockchainBlocksGet200Response api_blockchain_blocks_get(limit, offset, order) Get
> blockchain blocks

Retrieve a list of blockchain blocks with pagination

### Parameters

| Name       | Type               | Description                | Required | Notes             |
| ---------- | ------------------ | -------------------------- | -------- | ----------------- |
| **limit**  | Option<**i32**>    | Number of blocks to return |          | [default to 10]   |
| **offset** | Option<**i32**>    | Number of blocks to skip   |          | [default to 0]    |
| **order**  | Option<**String**> | Sort order                 |          | [default to desc] |

### Return type

[**models::ApiBlockchainBlocksGet200Response**](_api_blockchain_blocks_get_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_blockchain_status_get

> models::BlockchainStatus api_blockchain_status_get() Get blockchain status

Returns current blockchain status including block height, network health, and consensus state

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::BlockchainStatus**](BlockchainStatus.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_blockchain_transactions_tx_hash_get

> models::Transaction api_blockchain_transactions_tx_hash_get(tx_hash) Get transaction by hash

### Parameters

| Name        | Type       | Description | Required   | Notes |
| ----------- | ---------- | ----------- | ---------- | ----- |
| **tx_hash** | **String** |             | [required] |

### Return type

[**models::Transaction**](Transaction.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
