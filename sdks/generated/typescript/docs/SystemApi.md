# SystemApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                            | HTTP request        | Description         |
| --------------------------------- | ------------------- | ------------------- |
| [**apiHealthGet**](#apihealthget) | **GET** /api/health | API health check    |
| [**healthGet**](#healthget)       | **GET** /health     | System health check |

# **apiHealthGet**

> HealthGet200Response apiHealthGet()

Get comprehensive API health status

### Example

```typescript
import { SystemApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new SystemApi(configuration);

const { status, data } = await apiInstance.apiHealthGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**HealthGet200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                              | Response headers |
| ----------- | ---------------------------------------- | ---------------- |
| **200**     | API health status retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **healthGet**

> HealthGet200Response healthGet()

Get comprehensive system health status

### Example

```typescript
import { SystemApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new SystemApi(configuration);

const { status, data } = await apiInstance.healthGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**HealthGet200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                 | Response headers |
| ----------- | ------------------------------------------- | ---------------- |
| **200**     | System health status retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
