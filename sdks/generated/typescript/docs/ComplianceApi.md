# ComplianceApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                | HTTP request                   | Description           |
| ----------------------------------------------------- | ------------------------------ | --------------------- |
| [**apiComplianceStatusGet**](#apicompliancestatusget) | **GET** /api/compliance/status | Get compliance status |

# **apiComplianceStatusGet**

> ApiComplianceStatusGet200Response apiComplianceStatusGet()

Retrieve compliance status and certifications

### Example

```typescript
import { ComplianceApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new ComplianceApi(configuration);

const { status, data } = await apiInstance.apiComplianceStatusGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**ApiComplianceStatusGet200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                              | Response headers |
| ----------- | ---------------------------------------- | ---------------- |
| **200**     | Compliance status retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
