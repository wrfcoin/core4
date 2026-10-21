# wrfcoin_sdk.PerformanceApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                       | HTTP request                           | Description                      |
| -------------------------------------------------------------------------------------------- | -------------------------------------- | -------------------------------- |
| [**api_performance_metrics_get**](PerformanceApi.md#api_performance_metrics_get)             | **GET** /api/performance/metrics       | Get performance metrics          |
| [**api_performance_optimization_post**](PerformanceApi.md#api_performance_optimization_post) | **POST** /api/performance/optimization | Trigger performance optimization |

# **api_performance_metrics_get**

> ApiPerformanceMetricsGet200Response api_performance_metrics_get()

Get performance metrics

Retrieve comprehensive performance metrics

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_performance_metrics_get200_response import ApiPerformanceMetricsGet200Response
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
    api_instance = wrfcoin_sdk.PerformanceApi(api_client)

    try:
        # Get performance metrics
        api_response = api_instance.api_performance_metrics_get()
        print("The response of PerformanceApi->api_performance_metrics_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling PerformanceApi->api_performance_metrics_get: %s\n" % e)
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**ApiPerformanceMetricsGet200Response**](ApiPerformanceMetricsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                | Response headers |
| ----------- | ------------------------------------------ | ---------------- |
| **200**     | Performance metrics retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_performance_optimization_post**

> ApiPerformanceOptimizationPost200Response
> api_performance_optimization_post(api_performance_optimization_post_request)

Trigger performance optimization

Initiate performance optimization procedures

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_performance_optimization_post200_response import ApiPerformanceOptimizationPost200Response
from wrfcoin_sdk.models.api_performance_optimization_post_request import ApiPerformanceOptimizationPostRequest
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
    api_instance = wrfcoin_sdk.PerformanceApi(api_client)
    api_performance_optimization_post_request = wrfcoin_sdk.ApiPerformanceOptimizationPostRequest() # ApiPerformanceOptimizationPostRequest |

    try:
        # Trigger performance optimization
        api_response = api_instance.api_performance_optimization_post(api_performance_optimization_post_request)
        print("The response of PerformanceApi->api_performance_optimization_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling PerformanceApi->api_performance_optimization_post: %s\n" % e)
```

### Parameters

| Name                                          | Type                                                                                  | Description | Notes |
| --------------------------------------------- | ------------------------------------------------------------------------------------- | ----------- | ----- |
| **api_performance_optimization_post_request** | [**ApiPerformanceOptimizationPostRequest**](ApiPerformanceOptimizationPostRequest.md) |             |

### Return type

[**ApiPerformanceOptimizationPost200Response**](ApiPerformanceOptimizationPost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | Optimization initiated successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
