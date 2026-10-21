# wrfcoin_sdk.SecurityApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                              | HTTP request                | Description             |
| ------------------------------------------------------------------- | --------------------------- | ----------------------- |
| [**api_security_stats_get**](SecurityApi.md#api_security_stats_get) | **GET** /api/security/stats | Get security statistics |

# **api_security_stats_get**

> ApiSecurityStatsGet200Response api_security_stats_get()

Get security statistics

Retrieve security statistics and events

### Example

- Api Key Authentication (ApiKeyAuth):
- Bearer (JWT) Authentication (BearerAuth):

```python
import wrfcoin_sdk
from wrfcoin_sdk.models.api_security_stats_get200_response import ApiSecurityStatsGet200Response
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
    api_instance = wrfcoin_sdk.SecurityApi(api_client)

    try:
        # Get security statistics
        api_response = api_instance.api_security_stats_get()
        print("The response of SecurityApi->api_security_stats_get:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling SecurityApi->api_security_stats_get: %s\n" % e)
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**ApiSecurityStatsGet200Response**](ApiSecurityStatsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                | Response headers |
| ----------- | ------------------------------------------ | ---------------- |
| **200**     | Security statistics retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
