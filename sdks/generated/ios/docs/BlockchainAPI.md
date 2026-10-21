# BlockchainAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                        | HTTP request                                  | Description             |
| --------------------------------------------------------------------------------------------- | --------------------------------------------- | ----------------------- |
| [**apiBlockchainBlocksBlockHashGet**](BlockchainAPI.md#apiblockchainblocksblockhashget)       | **GET** /api/blockchain/blocks/{blockHash}    | Get block by hash       |
| [**apiBlockchainBlocksGet**](BlockchainAPI.md#apiblockchainblocksget)                         | **GET** /api/blockchain/blocks                | Get blockchain blocks   |
| [**apiBlockchainStatusGet**](BlockchainAPI.md#apiblockchainstatusget)                         | **GET** /api/blockchain/status                | Get blockchain status   |
| [**apiBlockchainTransactionsTxHashGet**](BlockchainAPI.md#apiblockchaintransactionstxhashget) | **GET** /api/blockchain/transactions/{txHash} | Get transaction by hash |

# **apiBlockchainBlocksBlockHashGet**

```swift
    open class func apiBlockchainBlocksBlockHashGet(blockHash: String, completion: @escaping (_ data: Block?, _ error: Error?) -> Void)
```

Get block by hash

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let blockHash = "blockHash_example" // String |

// Get block by hash
BlockchainAPI.apiBlockchainBlocksBlockHashGet(blockHash: blockHash) { (response, error) in
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

| Name          | Type       | Description | Notes |
| ------------- | ---------- | ----------- | ----- |
| **blockHash** | **String** |             |

### Return type

[**Block**](Block.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiBlockchainBlocksGet**

```swift
    open class func apiBlockchainBlocksGet(limit: Int? = nil, offset: Int? = nil, order: Order_apiBlockchainBlocksGet? = nil, completion: @escaping (_ data: ApiBlockchainBlocksGet200Response?, _ error: Error?) -> Void)
```

Get blockchain blocks

Retrieve a list of blockchain blocks with pagination

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let limit = 987 // Int | Number of blocks to return (optional) (default to 10)
let offset = 987 // Int | Number of blocks to skip (optional) (default to 0)
let order = "order_example" // String | Sort order (optional) (default to .desc)

// Get blockchain blocks
BlockchainAPI.apiBlockchainBlocksGet(limit: limit, offset: offset, order: order) { (response, error) in
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

| Name       | Type       | Description                | Notes                         |
| ---------- | ---------- | -------------------------- | ----------------------------- |
| **limit**  | **Int**    | Number of blocks to return | [optional] [default to 10]    |
| **offset** | **Int**    | Number of blocks to skip   | [optional] [default to 0]     |
| **order**  | **String** | Sort order                 | [optional] [default to .desc] |

### Return type

[**ApiBlockchainBlocksGet200Response**](ApiBlockchainBlocksGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiBlockchainStatusGet**

```swift
    open class func apiBlockchainStatusGet(completion: @escaping (_ data: BlockchainStatus?, _ error: Error?) -> Void)
```

Get blockchain status

Returns current blockchain status including block height, network health, and consensus state

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// Get blockchain status
BlockchainAPI.apiBlockchainStatusGet() { (response, error) in
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

[**BlockchainStatus**](BlockchainStatus.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiBlockchainTransactionsTxHashGet**

```swift
    open class func apiBlockchainTransactionsTxHashGet(txHash: String, completion: @escaping (_ data: Transaction?, _ error: Error?) -> Void)
```

Get transaction by hash

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let txHash = "txHash_example" // String |

// Get transaction by hash
BlockchainAPI.apiBlockchainTransactionsTxHashGet(txHash: txHash) { (response, error) in
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

| Name       | Type       | Description | Notes |
| ---------- | ---------- | ----------- | ----- |
| **txHash** | **String** |             |

### Return type

[**Transaction**](Transaction.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
