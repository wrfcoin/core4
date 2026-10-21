# SmartContractsAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                              | HTTP request                         | Description                    |
| ----------------------------------------------------------------------------------- | ------------------------------------ | ------------------------------ |
| [**apiContractsCallPost**](SmartContractsAPI.md#apicontractscallpost)               | **POST** /api/contracts/call         | Call contract function         |
| [**apiContractsDeployPost**](SmartContractsAPI.md#apicontractsdeploypost)           | **POST** /api/contracts/deploy       | Deploy smart contract          |
| [**apiContractsEstimateGasPost**](SmartContractsAPI.md#apicontractsestimategaspost) | **POST** /api/contracts/estimate-gas | Estimate gas for contract call |
| [**apiContractsQueryPost**](SmartContractsAPI.md#apicontractsquerypost)             | **POST** /api/contracts/query        | Query contract state           |

# **apiContractsCallPost**

```swift
    open class func apiContractsCallPost(contractCallRequest: ContractCallRequest, completion: @escaping (_ data: ContractCallResponse?, _ error: Error?) -> Void)
```

Call contract function

Execute a smart contract function

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let contractCallRequest = ContractCallRequest(contractAddress: "contractAddress_example", functionName: "functionName_example", parameters: [123], gasLimit: 123, gasPrice: "gasPrice_example", value: "value_example") // ContractCallRequest |

// Call contract function
SmartContractsAPI.apiContractsCallPost(contractCallRequest: contractCallRequest) { (response, error) in
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
| **contractCallRequest** | [**ContractCallRequest**](ContractCallRequest.md) |             |

### Return type

[**ContractCallResponse**](ContractCallResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiContractsDeployPost**

```swift
    open class func apiContractsDeployPost(contractDeployRequest: ContractDeployRequest, completion: @escaping (_ data: ContractDeployResponse?, _ error: Error?) -> Void)
```

Deploy smart contract

Deploy a new smart contract to the blockchain

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let contractDeployRequest = ContractDeployRequest(contractCode: "contractCode_example", constructorArgs: ["constructorArgs_example"], gasLimit: 123, gasPrice: "gasPrice_example") // ContractDeployRequest |

// Deploy smart contract
SmartContractsAPI.apiContractsDeployPost(contractDeployRequest: contractDeployRequest) { (response, error) in
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

| Name                      | Type                                                  | Description | Notes |
| ------------------------- | ----------------------------------------------------- | ----------- | ----- |
| **contractDeployRequest** | [**ContractDeployRequest**](ContractDeployRequest.md) |             |

### Return type

[**ContractDeployResponse**](ContractDeployResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiContractsEstimateGasPost**

```swift
    open class func apiContractsEstimateGasPost(contractCallRequest: ContractCallRequest, completion: @escaping (_ data: ApiContractsEstimateGasPost200Response?, _ error: Error?) -> Void)
```

Estimate gas for contract call

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let contractCallRequest = ContractCallRequest(contractAddress: "contractAddress_example", functionName: "functionName_example", parameters: [123], gasLimit: 123, gasPrice: "gasPrice_example", value: "value_example") // ContractCallRequest |

// Estimate gas for contract call
SmartContractsAPI.apiContractsEstimateGasPost(contractCallRequest: contractCallRequest) { (response, error) in
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
| **contractCallRequest** | [**ContractCallRequest**](ContractCallRequest.md) |             |

### Return type

[**ApiContractsEstimateGasPost200Response**](ApiContractsEstimateGasPost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiContractsQueryPost**

```swift
    open class func apiContractsQueryPost(contractQueryRequest: ContractQueryRequest, completion: @escaping (_ data: ContractQueryResponse?, _ error: Error?) -> Void)
```

Query contract state

Query smart contract state without modifying it

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let contractQueryRequest = ContractQueryRequest(contractAddress: "contractAddress_example", functionName: "functionName_example", parameters: [123]) // ContractQueryRequest |

// Query contract state
SmartContractsAPI.apiContractsQueryPost(contractQueryRequest: contractQueryRequest) { (response, error) in
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

| Name                     | Type                                                | Description | Notes |
| ------------------------ | --------------------------------------------------- | ----------- | ----- |
| **contractQueryRequest** | [**ContractQueryRequest**](ContractQueryRequest.md) |             |

### Return type

[**ContractQueryResponse**](ContractQueryResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
