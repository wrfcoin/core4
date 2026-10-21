# wrfcoin_sdk.ComputeApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                         | HTTP request                               | Description             |
| ---------------------------------------------------------------------------------------------- | ------------------------------------------ | ----------------------- |
| [**api_compute_providers_get**](ComputeApi.md#api_compute_providers_get)                       | **GET** /api/compute/providers             | List compute providers  |
| [**api_compute_tasks_post**](ComputeApi.md#api_compute_tasks_post)                             | **POST** /api/compute/tasks                | Submit compute task     |
| [**api_compute_tasks_task_id_status_get**](ComputeApi.md#api_compute_tasks_task_id_status_get) | **GET** /api/compute/tasks/{taskId}/status | Get compute task status |

# **api_compute_providers_get**

> ApiComputeProvidersGet200Response api_compute_providers_get()

List compute providers

Retrieve list of available compute providers

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_compute_providers_get200_response import ApiComputeProvidersGet200Response
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
    api_instance = wrfcoin_sdk.ComputeApi(api_client)

    try:
        # List compute providers
        api_response = api_instance.api_compute_providers_get()
        print("The response of ComputeApi->api_compute_providers_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling ComputeApi->api_compute_providers_get: %s\n" % e)
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**ApiComputeProvidersGet200Response**](ApiComputeProvidersGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                              | Response headers |
| ----------- | ---------------------------------------- | ---------------- |
| **200**     | Compute providers retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_compute_tasks_post**

> ApiComputeTasksPost200Response api_compute_tasks_post(api_compute_tasks_post_request)

Submit compute task

Submit a distributed computing task to the network

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_compute_tasks_post200_response import ApiComputeTasksPost200Response
from wrfcoin_sdk.models.api_compute_tasks_post_request import ApiComputeTasksPostRequest
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
    api_instance = wrfcoin_sdk.ComputeApi(api_client)
    api_compute_tasks_post_request = {"taskType":"machine-learning","priority":"high","resourceRequirements":{"cpu":8,"memory":32,"storage":100,"gpu":true}} # ApiComputeTasksPostRequest |

    try:
        # Submit compute task
        api_response = api_instance.api_compute_tasks_post(api_compute_tasks_post_request)
        print("The response of ComputeApi->api_compute_tasks_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling ComputeApi->api_compute_tasks_post: %s\n" % e)
```

### Parameters

| Name                               | Type                                                            | Description | Notes |
| ---------------------------------- | --------------------------------------------------------------- | ----------- | ----- |
| **api_compute_tasks_post_request** | [**ApiComputeTasksPostRequest**](ApiComputeTasksPostRequest.md) |             |

### Return type

[**ApiComputeTasksPost200Response**](ApiComputeTasksPost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                 | Response headers |
| ----------- | --------------------------- | ---------------- |
| **200**     | Task submitted successfully | -                |
| **400**     | Invalid task request        | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_compute_tasks_task_id_status_get**

> ApiComputeTasksTaskIdStatusGet200Response api_compute_tasks_task_id_status_get(task_id)

Get compute task status

Retrieve the status of a specific compute task

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_compute_tasks_task_id_status_get200_response import ApiComputeTasksTaskIdStatusGet200Response
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
    api_instance = wrfcoin_sdk.ComputeApi(api_client)
    task_id = 'task_id_example' # str | Task identifier

    try:
        # Get compute task status
        api_response = api_instance.api_compute_tasks_task_id_status_get(task_id)
        print("The response of ComputeApi->api_compute_tasks_task_id_status_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling ComputeApi->api_compute_tasks_task_id_status_get: %s\n" % e)
```

### Parameters

| Name        | Type    | Description     | Notes |
| ----------- | ------- | --------------- | ----- |
| **task_id** | **str** | Task identifier |

### Return type

[**ApiComputeTasksTaskIdStatusGet200Response**](ApiComputeTasksTaskIdStatusGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                        | Response headers |
| ----------- | ---------------------------------- | ---------------- |
| **200**     | Task status retrieved successfully | -                |
| **404**     | Task not found                     | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
