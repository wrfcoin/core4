# wrfcoin_sdk.AIMLApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                            | HTTP request                 | Description                 |
| ----------------------------------------------------------------- | ---------------------------- | --------------------------- |
| [**api_aiml_inference_post**](AIMLApi.md#api_aiml_inference_post) | **POST** /api/aiml/inference | Run AI/ML inference         |
| [**api_aiml_models_get**](AIMLApi.md#api_aiml_models_get)         | **GET** /api/aiml/models     | List available AI/ML models |

# **api_aiml_inference_post**

> ApiAimlInferencePost200Response api_aiml_inference_post(api_aiml_inference_post_request)

Run AI/ML inference

Execute AI/ML inference on the compute network

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_aiml_inference_post200_response import ApiAimlInferencePost200Response
from wrfcoin_sdk.models.api_aiml_inference_post_request import ApiAimlInferencePostRequest
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
    api_instance = wrfcoin_sdk.AIMLApi(api_client)
    api_aiml_inference_post_request = wrfcoin_sdk.ApiAimlInferencePostRequest() # ApiAimlInferencePostRequest |

    try:
        # Run AI/ML inference
        api_response = api_instance.api_aiml_inference_post(api_aiml_inference_post_request)
        print("The response of AIMLApi->api_aiml_inference_post:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling AIMLApi->api_aiml_inference_post: %s\n" % e)
```

### Parameters

| Name                                | Type                                                              | Description | Notes |
| ----------------------------------- | ----------------------------------------------------------------- | ----------- | ----- |
| **api_aiml_inference_post_request** | [**ApiAimlInferencePostRequest**](ApiAimlInferencePostRequest.md) |             |

### Return type

[**ApiAimlInferencePost200Response**](ApiAimlInferencePost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                      | Response headers |
| ----------- | -------------------------------- | ---------------- |
| **200**     | Inference completed successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **api_aiml_models_get**

> ApiAimlModelsGet200Response api_aiml_models_get()

List available AI/ML models

Retrieve list of available AI/ML models for computation

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_aiml_models_get200_response import ApiAimlModelsGet200Response
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
    api_instance = wrfcoin_sdk.AIMLApi(api_client)

    try:
        # List available AI/ML models
        api_response = api_instance.api_aiml_models_get()
        print("The response of AIMLApi->api_aiml_models_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling AIMLApi->api_aiml_models_get: %s\n" % e)
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**ApiAimlModelsGet200Response**](ApiAimlModelsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | AI/ML models retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
