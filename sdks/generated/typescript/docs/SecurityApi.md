# SecurityApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                          | HTTP request                | Description             |
| ----------------------------------------------- | --------------------------- | ----------------------- |
| [**apiSecurityStatsGet**](#apisecuritystatsget) | **GET** /api/security/stats | Get security statistics |

# **apiSecurityStatsGet**

> ApiSecurityStatsGet200Response apiSecurityStatsGet()

Retrieve security statistics and events

### Example

```typescript
import { SecurityApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new SecurityApi(configuration);

const { status, data } = await apiInstance.apiSecurityStatsGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**ApiSecurityStatsGet200Response**

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
