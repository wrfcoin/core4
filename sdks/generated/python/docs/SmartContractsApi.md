# wrfcoin_sdk.SmartContractsApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                      | HTTP request                         | Description                    |
| ------------------------------------------------------------------------------------------- | ------------------------------------ | ------------------------------ |
| [**api_contracts_call_post**](SmartContractsApi.md#api_contracts_call_post)                 | **POST** /api/contracts/call         | Call contract function         |
| [**api_contracts_deploy_post**](SmartContractsApi.md#api_contracts_deploy_post)             | **POST** /api/contracts/deploy       | Deploy smart contract          |
| [**api_contracts_estimate_gas_post**](SmartContractsApi.md#api_contracts_estimate_gas_post) | **POST** /api/contracts/estimate-gas | Estimate gas for contract call |
| [**api_contracts_query_post**](SmartContractsApi.md#api_contracts_query_post)               | **POST** /api/contracts/query        | Query contract state           |

# **api_contracts_call_post**

> ContractCallResponse api_contracts_call_post(contract_call_request)

Call contract function

Execute a smart contract function

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.contract_call_request import ContractCallRequest
from wrfcoin_sdk.models.contract_call_response import ContractCallResponse
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
    api_instance = wrfcoin_sdk.SmartContractsApi(api_client)
    contract_call_request = wrfcoin_sdk.ContractCallRequest() # ContractCallRequest |

    try:
        # Call contract function
        api_response = api_instance.api_contracts_call_post(contract_call_request)
        print("The response of SmartContractsApi->api_contracts_call_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling SmartContractsApi->api_contracts_call_post: %s\n" % e)
```

### Parameters

| Name                      | Type                                              | Description | Notes |
| ------------------------- | ------------------------------------------------- | ----------- | ----- |
| **contract_call_request** | [**ContractCallRequest**](ContractCallRequest.md) |             |

### Return type

[**ContractCallResponse**](ContractCallResponse.md)

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

# **api_contracts_deploy_post**

> ContractDeployResponse api_contracts_deploy_post(contract_deploy_request)

Deploy smart contract

Deploy a new smart contract to the blockchain

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.contract_deploy_request import ContractDeployRequest
from wrfcoin_sdk.models.contract_deploy_response import ContractDeployResponse
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
    api_instance = wrfcoin_sdk.SmartContractsApi(api_client)
    contract_deploy_request = wrfcoin_sdk.ContractDeployRequest() # ContractDeployRequest |

    try:
        # Deploy smart contract
        api_response = api_instance.api_contracts_deploy_post(contract_deploy_request)
        print("The response of SmartContractsApi->api_contracts_deploy_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling SmartContractsApi->api_contracts_deploy_post: %s\n" % e)
```

### Parameters

| Name                        | Type                                                  | Description | Notes |
| --------------------------- | ----------------------------------------------------- | ----------- | ----- |
| **contract_deploy_request** | [**ContractDeployRequest**](ContractDeployRequest.md) |             |

### Return type

[**ContractDeployResponse**](ContractDeployResponse.md)

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

# **api_contracts_estimate_gas_post**

> ApiContractsEstimateGasPost200Response api_contracts_estimate_gas_post(contract_call_request)

Estimate gas for contract call

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_contracts_estimate_gas_post200_response import ApiContractsEstimateGasPost200Response
from wrfcoin_sdk.models.contract_call_request import ContractCallRequest
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
    api_instance = wrfcoin_sdk.SmartContractsApi(api_client)
    contract_call_request = wrfcoin_sdk.ContractCallRequest() # ContractCallRequest |

    try:
        # Estimate gas for contract call
        api_response = api_instance.api_contracts_estimate_gas_post(contract_call_request)
        print("The response of SmartContractsApi->api_contracts_estimate_gas_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling SmartContractsApi->api_contracts_estimate_gas_post: %s\n" % e)
```

### Parameters

| Name                      | Type                                              | Description | Notes |
| ------------------------- | ------------------------------------------------- | ----------- | ----- |
| **contract_call_request** | [**ContractCallRequest**](ContractCallRequest.md) |             |

### Return type

[**ApiContractsEstimateGasPost200Response**](ApiContractsEstimateGasPost200Response.md)

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

# **api_contracts_query_post**

> ContractQueryResponse api_contracts_query_post(contract_query_request)

Query contract state

Query smart contract state without modifying it

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.contract_query_request import ContractQueryRequest
from wrfcoin_sdk.models.contract_query_response import ContractQueryResponse
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
    api_instance = wrfcoin_sdk.SmartContractsApi(api_client)
    contract_query_request = wrfcoin_sdk.ContractQueryRequest() # ContractQueryRequest |

    try:
        # Query contract state
        api_response = api_instance.api_contracts_query_post(contract_query_request)
        print("The response of SmartContractsApi->api_contracts_query_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling SmartContractsApi->api_contracts_query_post: %s\n" % e)
```

### Parameters

| Name                       | Type                                                | Description | Notes |
| -------------------------- | --------------------------------------------------- | ----------- | ----- |
| **contract_query_request** | [**ContractQueryRequest**](ContractQueryRequest.md) |             |

### Return type

[**ContractQueryResponse**](ContractQueryResponse.md)

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
