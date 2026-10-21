# MiningAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                      | HTTP request                | Description        |
| ----------------------------------------------------------- | --------------------------- | ------------------ |
| [**apiMiningRewardsGet**](MiningAPI.md#apiminingrewardsget) | **GET** /api/mining/rewards | Get mining rewards |

# **apiMiningRewardsGet**

```swift
    open class func apiMiningRewardsGet(address: String? = nil, type: ModelType_apiMiningRewardsGet? = nil, limit: Int? = nil, completion: @escaping (_ data: ApiMiningRewardsGet200Response?, _ error: Error?) -> Void)
```

Get mining rewards

Retrieve mining rewards for a specific address

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let address = "address_example" // String | Wallet address to get rewards for (optional)
let type = "type_example" // String | Type of mining reward (optional)
let limit = 987 // Int | Number of rewards to return (optional) (default to 10)

// Get mining rewards
MiningAPI.apiMiningRewardsGet(address: address, type: type, limit: limit) { (response, error) in
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

| Name        | Type       | Description                       | Notes                      |
| ----------- | ---------- | --------------------------------- | -------------------------- |
| **address** | **String** | Wallet address to get rewards for | [optional]                 |
| **type**    | **String** | Type of mining reward             | [optional]                 |
| **limit**   | **Int**    | Number of rewards to return       | [optional] [default to 10] |

### Return type

[**ApiMiningRewardsGet200Response**](ApiMiningRewardsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
