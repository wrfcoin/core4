# \MiningApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                            | HTTP request                | Description        |
| ----------------------------------------------------------------- | --------------------------- | ------------------ |
| [**api_mining_rewards_get**](MiningApi.md#api_mining_rewards_get) | **GET** /api/mining/rewards | Get mining rewards |

## api_mining_rewards_get

> models::ApiMiningRewardsGet200Response api_mining_rewards_get(address, r#type, limit) Get mining
> rewards

Retrieve mining rewards for a specific address

### Parameters

| Name        | Type               | Description                       | Required | Notes           |
| ----------- | ------------------ | --------------------------------- | -------- | --------------- |
| **address** | Option<**String**> | Wallet address to get rewards for |          |
| **r#type**  | Option<**String**> | Type of mining reward             |          |
| **limit**   | Option<**i32**>    | Number of rewards to return       |          | [default to 10] |

### Return type

[**models::ApiMiningRewardsGet200Response**](_api_mining_rewards_get_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
