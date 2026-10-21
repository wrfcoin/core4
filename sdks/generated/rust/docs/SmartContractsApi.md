# \SmartContractsApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                      | HTTP request                         | Description                    |
| ------------------------------------------------------------------------------------------- | ------------------------------------ | ------------------------------ |
| [**api_contracts_call_post**](SmartContractsApi.md#api_contracts_call_post)                 | **POST** /api/contracts/call         | Call contract function         |
| [**api_contracts_deploy_post**](SmartContractsApi.md#api_contracts_deploy_post)             | **POST** /api/contracts/deploy       | Deploy smart contract          |
| [**api_contracts_estimate_gas_post**](SmartContractsApi.md#api_contracts_estimate_gas_post) | **POST** /api/contracts/estimate-gas | Estimate gas for contract call |
| [**api_contracts_query_post**](SmartContractsApi.md#api_contracts_query_post)               | **POST** /api/contracts/query        | Query contract state           |

## api_contracts_call_post

> models::ContractCallResponse api_contracts_call_post(contract_call_request) Call contract function

Execute a smart contract function

### Parameters

| Name                      | Type                                              | Description | Required   | Notes |
| ------------------------- | ------------------------------------------------- | ----------- | ---------- | ----- |
| **contract_call_request** | [**ContractCallRequest**](ContractCallRequest.md) |             | [required] |

### Return type

[**models::ContractCallResponse**](ContractCallResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_contracts_deploy_post

> models::ContractDeployResponse api_contracts_deploy_post(contract_deploy_request) Deploy smart
> contract

Deploy a new smart contract to the blockchain

### Parameters

| Name                        | Type                                                  | Description | Required   | Notes |
| --------------------------- | ----------------------------------------------------- | ----------- | ---------- | ----- |
| **contract_deploy_request** | [**ContractDeployRequest**](ContractDeployRequest.md) |             | [required] |

### Return type

[**models::ContractDeployResponse**](ContractDeployResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_contracts_estimate_gas_post

> models::ApiContractsEstimateGasPost200Response
> api_contracts_estimate_gas_post(contract_call_request) Estimate gas for contract call

### Parameters

| Name                      | Type                                              | Description | Required   | Notes |
| ------------------------- | ------------------------------------------------- | ----------- | ---------- | ----- |
| **contract_call_request** | [**ContractCallRequest**](ContractCallRequest.md) |             | [required] |

### Return type

[**models::ApiContractsEstimateGasPost200Response**](_api_contracts_estimate_gas_post_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_contracts_query_post

> models::ContractQueryResponse api_contracts_query_post(contract_query_request) Query contract
> state

Query smart contract state without modifying it

### Parameters

| Name                       | Type                                                | Description | Required   | Notes |
| -------------------------- | --------------------------------------------------- | ----------- | ---------- | ----- |
| **contract_query_request** | [**ContractQueryRequest**](ContractQueryRequest.md) |             | [required] |

### Return type

[**models::ContractQueryResponse**](ContractQueryResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
