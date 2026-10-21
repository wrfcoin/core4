# BlockchainApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                        | HTTP request                                  | Description             |
| ----------------------------------------------------------------------------- | --------------------------------------------- | ----------------------- |
| [**apiBlockchainBlocksBlockHashGet**](#apiblockchainblocksblockhashget)       | **GET** /api/blockchain/blocks/{blockHash}    | Get block by hash       |
| [**apiBlockchainBlocksGet**](#apiblockchainblocksget)                         | **GET** /api/blockchain/blocks                | Get blockchain blocks   |
| [**apiBlockchainStatusGet**](#apiblockchainstatusget)                         | **GET** /api/blockchain/status                | Get blockchain status   |
| [**apiBlockchainTransactionsTxHashGet**](#apiblockchaintransactionstxhashget) | **GET** /api/blockchain/transactions/{txHash} | Get transaction by hash |

# **apiBlockchainBlocksBlockHashGet**

> Block apiBlockchainBlocksBlockHashGet()

### Example

```typescript
import { BlockchainApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new BlockchainApi(configuration);

let blockHash: string; // (default to undefined)

const { status, data } = await apiInstance.apiBlockchainBlocksBlockHashGet(blockHash);
```

### Parameters

| Name          | Type         | Description | Notes                 |
| ------------- | ------------ | ----------- | --------------------- |
| **blockHash** | [**string**] |             | defaults to undefined |

### Return type

**Block**

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

# **apiBlockchainBlocksGet**

> ApiBlockchainBlocksGet200Response apiBlockchainBlocksGet()

Retrieve a list of blockchain blocks with pagination

### Example

```typescript
import { BlockchainApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new BlockchainApi(configuration);

let limit: number; //Number of blocks to return (optional) (default to 10)
let offset: number; //Number of blocks to skip (optional) (default to 0)
let order: 'asc' | 'desc'; //Sort order (optional) (default to 'desc')

const { status, data } = await apiInstance.apiBlockchainBlocksGet(limit, offset, order);
```

### Parameters

| Name       | Type               | Description                                                       | Notes                     |
| ---------- | ------------------ | ----------------------------------------------------------------- | ------------------------- | ----------------------------- |
| **limit**  | [**number**]       | Number of blocks to return                                        | (optional) defaults to 10 |
| **offset** | [**number**]       | Number of blocks to skip                                          | (optional) defaults to 0  |
| **order**  | [\*\*&#39;asc&#39; | &#39;desc&#39;**]**Array<&#39;asc&#39; &#124; &#39;desc&#39;>\*\* | Sort order                | (optional) defaults to 'desc' |

### Return type

**ApiBlockchainBlocksGet200Response**

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

# **apiBlockchainStatusGet**

> BlockchainStatus apiBlockchainStatusGet()

Returns current blockchain status including block height, network health, and consensus state

### Example

```typescript
import { BlockchainApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new BlockchainApi(configuration);

const { status, data } = await apiInstance.apiBlockchainStatusGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**BlockchainStatus**

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

# **apiBlockchainTransactionsTxHashGet**

> Transaction apiBlockchainTransactionsTxHashGet()

### Example

```typescript
import { BlockchainApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new BlockchainApi(configuration);

let txHash: string; // (default to undefined)

const { status, data } = await apiInstance.apiBlockchainTransactionsTxHashGet(txHash);
```

### Parameters

| Name       | Type         | Description | Notes                 |
| ---------- | ------------ | ----------- | --------------------- |
| **txHash** | [**string**] |             | defaults to undefined |

### Return type

**Transaction**

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
