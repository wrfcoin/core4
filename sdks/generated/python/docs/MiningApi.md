# wrfcoin_sdk.MiningApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                            | HTTP request                | Description        |
| ----------------------------------------------------------------- | --------------------------- | ------------------ |
| [**api_mining_rewards_get**](MiningApi.md#api_mining_rewards_get) | **GET** /api/mining/rewards | Get mining rewards |

# **api_mining_rewards_get**

> ApiMiningRewardsGet200Response api_mining_rewards_get(address=address, type=type, limit=limit)

Get mining rewards

Retrieve mining rewards for a specific address

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_mining_rewards_get200_response import ApiMiningRewardsGet200Response
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
    api_instance = wrfcoin_sdk.MiningApi(api_client)
    address = 'address_example' # str | Wallet address to get rewards for (optional)
    type = 'type_example' # str | Type of mining reward (optional)
    limit = 10 # int | Number of rewards to return (optional) (default to 10)

    try:
        # Get mining rewards
        api_response = api_instance.api_mining_rewards_get(address=address, type=type, limit=limit)
        print("The response of MiningApi->api_mining_rewards_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling MiningApi->api_mining_rewards_get: %s\n" % e)
```

### Parameters

| Name        | Type    | Description                       | Notes                      |
| ----------- | ------- | --------------------------------- | -------------------------- |
| **address** | **str** | Wallet address to get rewards for | [optional]                 |
| **type**    | **str** | Type of mining reward             | [optional]                 |
| **limit**   | **int** | Number of rewards to return       | [optional] [default to 10] |

### Return type

[**ApiMiningRewardsGet200Response**](ApiMiningRewardsGet200Response.md)

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
