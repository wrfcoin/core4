# wrfcoin_sdk.IntegrationsApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                                    | HTTP request                                 | Description                      |
| --------------------------------------------------------------------------------------------------------- | -------------------------------------------- | -------------------------------- |
| [**api_integrations_langchain_enable_post**](IntegrationsApi.md#api_integrations_langchain_enable_post)   | **POST** /api/integrations/langchain/enable  | Enable LangChain integration     |
| [**api_integrations_langchain_insights_get**](IntegrationsApi.md#api_integrations_langchain_insights_get) | **GET** /api/integrations/langchain/insights | Get LangChain insights           |
| [**api_integrations_langchain_status_get**](IntegrationsApi.md#api_integrations_langchain_status_get)     | **GET** /api/integrations/langchain/status   | Get LangChain integration status |

# **api_integrations_langchain_enable_post**

> ApiIntegrationsLangchainEnablePost200Response api_integrations_langchain_enable_post()

Enable LangChain integration

Enable LangChain analytics integration

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_integrations_langchain_enable_post200_response import ApiIntegrationsLangchainEnablePost200Response
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
    api_instance = wrfcoin_sdk.IntegrationsApi(api_client)

    try:
        # Enable LangChain integration
        api_response = api_instance.api_integrations_langchain_enable_post()
        print("The response of IntegrationsApi->api_integrations_langchain_enable_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling IntegrationsApi->api_integrations_langchain_enable_post: %s\n" % e)
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**ApiIntegrationsLangchainEnablePost200Response**](ApiIntegrationsLangchainEnablePost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                | Response headers |
| ----------- | ------------------------------------------ | ---------------- |
| **200**     | LangChain integration enabled successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_integrations_langchain_insights_get**

> ApiIntegrationsLangchainInsightsGet200Response api_integrations_langchain_insights_get()

Get LangChain insights

Retrieve AI-powered insights from LangChain analytics

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_integrations_langchain_insights_get200_response import ApiIntegrationsLangchainInsightsGet200Response
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
    api_instance = wrfcoin_sdk.IntegrationsApi(api_client)

    try:
        # Get LangChain insights
        api_response = api_instance.api_integrations_langchain_insights_get()
        print("The response of IntegrationsApi->api_integrations_langchain_insights_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling IntegrationsApi->api_integrations_langchain_insights_get: %s\n" % e)
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**ApiIntegrationsLangchainInsightsGet200Response**](ApiIntegrationsLangchainInsightsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                               | Response headers |
| ----------- | ----------------------------------------- | ---------------- |
| **200**     | LangChain insights retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_integrations_langchain_status_get**

> ApiIntegrationsLangchainStatusGet200Response api_integrations_langchain_status_get()

Get LangChain integration status

Retrieve status of LangChain analytics integration

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_integrations_langchain_status_get200_response import ApiIntegrationsLangchainStatusGet200Response
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
    api_instance = wrfcoin_sdk.IntegrationsApi(api_client)

    try:
        # Get LangChain integration status
        api_response = api_instance.api_integrations_langchain_status_get()
        print("The response of IntegrationsApi->api_integrations_langchain_status_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling IntegrationsApi->api_integrations_langchain_status_get: %s\n" % e)
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**ApiIntegrationsLangchainStatusGet200Response**](ApiIntegrationsLangchainStatusGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                         | Response headers |
| ----------- | --------------------------------------------------- | ---------------- |
| **200**     | LangChain integration status retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
