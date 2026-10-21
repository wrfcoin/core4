# SmartContractsApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                          | HTTP request                         | Description                    |
| --------------------------------------------------------------- | ------------------------------------ | ------------------------------ |
| [**apiContractsCallPost**](#apicontractscallpost)               | **POST** /api/contracts/call         | Call contract function         |
| [**apiContractsDeployPost**](#apicontractsdeploypost)           | **POST** /api/contracts/deploy       | Deploy smart contract          |
| [**apiContractsEstimateGasPost**](#apicontractsestimategaspost) | **POST** /api/contracts/estimate-gas | Estimate gas for contract call |
| [**apiContractsQueryPost**](#apicontractsquerypost)             | **POST** /api/contracts/query        | Query contract state           |

# **apiContractsCallPost**

> ContractCallResponse apiContractsCallPost(contractCallRequest)

Execute a smart contract function

### Example

```typescript
import { SmartContractsApi, Configuration, ContractCallRequest } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new SmartContractsApi(configuration);

let contractCallRequest: ContractCallRequest; //

const { status, data } = await apiInstance.apiContractsCallPost(contractCallRequest);
```

### Parameters

| Name                    | Type                    | Description | Notes |
| ----------------------- | ----------------------- | ----------- | ----- |
| **contractCallRequest** | **ContractCallRequest** |             |       |

### Return type

**ContractCallResponse**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                             | Response headers |
| ----------- | --------------------------------------- | ---------------- |
| **200**     | Contract function executed successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiContractsDeployPost**

> ContractDeployResponse apiContractsDeployPost(contractDeployRequest)

Deploy a new smart contract to the blockchain

### Example

```typescript
import { SmartContractsApi, Configuration, ContractDeployRequest } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new SmartContractsApi(configuration);

let contractDeployRequest: ContractDeployRequest; //

const { status, data } = await apiInstance.apiContractsDeployPost(contractDeployRequest);
```

### Parameters

| Name                      | Type                      | Description | Notes |
| ------------------------- | ------------------------- | ----------- | ----- |
| **contractDeployRequest** | **ContractDeployRequest** |             |       |

### Return type

**ContractDeployResponse**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | Contract deployed successfully      | -                |
| **400**     | Invalid contract code or parameters | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiContractsEstimateGasPost**

> ApiContractsEstimateGasPost200Response apiContractsEstimateGasPost(contractCallRequest)

### Example

```typescript
import { SmartContractsApi, Configuration, ContractCallRequest } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new SmartContractsApi(configuration);

let contractCallRequest: ContractCallRequest; //

const { status, data } = await apiInstance.apiContractsEstimateGasPost(contractCallRequest);
```

### Parameters

| Name                    | Type                    | Description | Notes |
| ----------------------- | ----------------------- | ----------- | ----- |
| **contractCallRequest** | **ContractCallRequest** |             |       |

### Return type

**ApiContractsEstimateGasPost200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description             | Response headers |
| ----------- | ----------------------- | ---------------- |
| **200**     | Gas estimate calculated | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiContractsQueryPost**

> ContractQueryResponse apiContractsQueryPost(contractQueryRequest)

Query smart contract state without modifying it

### Example

```typescript
import { SmartContractsApi, Configuration, ContractQueryRequest } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new SmartContractsApi(configuration);

let contractQueryRequest: ContractQueryRequest; //

const { status, data } = await apiInstance.apiContractsQueryPost(contractQueryRequest);
```

### Parameters

| Name                     | Type                     | Description | Notes |
| ------------------------ | ------------------------ | ----------- | ----- |
| **contractQueryRequest** | **ContractQueryRequest** |             |       |

### Return type

**ContractQueryResponse**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | Contract state queried successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
