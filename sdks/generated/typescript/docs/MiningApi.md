# MiningApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                          | HTTP request                | Description        |
| ----------------------------------------------- | --------------------------- | ------------------ |
| [**apiMiningRewardsGet**](#apiminingrewardsget) | **GET** /api/mining/rewards | Get mining rewards |

# **apiMiningRewardsGet**

> ApiMiningRewardsGet200Response apiMiningRewardsGet()

Retrieve mining rewards for a specific address

### Example

```typescript
import { MiningApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new MiningApi(configuration);

let address: string; //Wallet address to get rewards for (optional) (default to undefined)
let type: 'weather_data' | 'compute' | 'validation' | 'staking'; //Type of mining reward (optional) (default to undefined)
let limit: number; //Number of rewards to return (optional) (default to 10)

const { status, data } = await apiInstance.apiMiningRewardsGet(address, type, limit);
```

### Parameters

| Name        | Type                        | Description                       | Notes                            |
| ----------- | --------------------------- | --------------------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | --------------------- | -------------------------------- |
| **address** | [**string**]                | Wallet address to get rewards for | (optional) defaults to undefined |
| **type**    | [\*\*&#39;weather_data&#39; | &#39;compute&#39;                 | &#39;validation&#39;             | &#39;staking&#39;**]**Array<&#39;weather_data&#39; &#124; &#39;compute&#39; &#124; &#39;validation&#39; &#124; &#39;staking&#39;>\*\* | Type of mining reward | (optional) defaults to undefined |
| **limit**   | [**number**]                | Number of rewards to return       | (optional) defaults to 10        |

### Return type

**ApiMiningRewardsGet200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                           | Response headers |
| ----------- | ------------------------------------- | ---------------- |
| **200**     | Mining rewards retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
