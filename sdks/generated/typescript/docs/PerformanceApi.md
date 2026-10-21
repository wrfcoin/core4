# PerformanceApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                | HTTP request                           | Description                      |
| --------------------------------------------------------------------- | -------------------------------------- | -------------------------------- |
| [**apiPerformanceMetricsGet**](#apiperformancemetricsget)             | **GET** /api/performance/metrics       | Get performance metrics          |
| [**apiPerformanceOptimizationPost**](#apiperformanceoptimizationpost) | **POST** /api/performance/optimization | Trigger performance optimization |

# **apiPerformanceMetricsGet**

> ApiPerformanceMetricsGet200Response apiPerformanceMetricsGet()

Retrieve comprehensive performance metrics

### Example

```typescript
import { PerformanceApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new PerformanceApi(configuration);

const { status, data } = await apiInstance.apiPerformanceMetricsGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**ApiPerformanceMetricsGet200Response**

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

# **apiPerformanceOptimizationPost**

> ApiPerformanceOptimizationPost200Response
> apiPerformanceOptimizationPost(apiPerformanceOptimizationPostRequest)

Initiate performance optimization procedures

### Example

```typescript
import {
  PerformanceApi,
  Configuration,
  ApiPerformanceOptimizationPostRequest,
} from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new PerformanceApi(configuration);

let apiPerformanceOptimizationPostRequest: ApiPerformanceOptimizationPostRequest; //

const { status, data } = await apiInstance.apiPerformanceOptimizationPost(
  apiPerformanceOptimizationPostRequest
);
```

### Parameters

| Name                                      | Type                                      | Description | Notes |
| ----------------------------------------- | ----------------------------------------- | ----------- | ----- |
| **apiPerformanceOptimizationPostRequest** | **ApiPerformanceOptimizationPostRequest** |             |       |

### Return type

**ApiPerformanceOptimizationPost200Response**

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
