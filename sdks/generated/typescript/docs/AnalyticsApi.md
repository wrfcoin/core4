# AnalyticsApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                  | HTTP request           | Description           |
| --------------------------------------- | ---------------------- | --------------------- |
| [**apiAnalyticsGet**](#apianalyticsget) | **GET** /api/analytics | Get network analytics |

# **apiAnalyticsGet**

> ApiAnalyticsGet200Response apiAnalyticsGet()

Retrieve comprehensive network analytics and metrics

### Example

```typescript
import { AnalyticsApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new AnalyticsApi(configuration);

const { status, data } = await apiInstance.apiAnalyticsGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**ApiAnalyticsGet200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                              | Response headers |
| ----------- | ---------------------------------------- | ---------------- |
| **200**     | Network analytics retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
