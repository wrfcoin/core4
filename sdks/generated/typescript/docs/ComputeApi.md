# ComputeApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                | HTTP request                               | Description             |
| --------------------------------------------------------------------- | ------------------------------------------ | ----------------------- |
| [**apiComputeProvidersGet**](#apicomputeprovidersget)                 | **GET** /api/compute/providers             | List compute providers  |
| [**apiComputeTasksPost**](#apicomputetaskspost)                       | **POST** /api/compute/tasks                | Submit compute task     |
| [**apiComputeTasksTaskIdStatusGet**](#apicomputetaskstaskidstatusget) | **GET** /api/compute/tasks/{taskId}/status | Get compute task status |

# **apiComputeProvidersGet**

> ApiComputeProvidersGet200Response apiComputeProvidersGet()

Retrieve list of available compute providers

### Example

```typescript
import { ComputeApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new ComputeApi(configuration);

const { status, data } = await apiInstance.apiComputeProvidersGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**ApiComputeProvidersGet200Response**

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

# **apiComputeTasksPost**

> ApiComputeTasksPost200Response apiComputeTasksPost(apiComputeTasksPostRequest)

Submit a distributed computing task to the network

### Example

```typescript
import { ComputeApi, Configuration, ApiComputeTasksPostRequest } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new ComputeApi(configuration);

let apiComputeTasksPostRequest: ApiComputeTasksPostRequest; //

const { status, data } = await apiInstance.apiComputeTasksPost(apiComputeTasksPostRequest);
```

### Parameters

| Name                           | Type                           | Description | Notes |
| ------------------------------ | ------------------------------ | ----------- | ----- |
| **apiComputeTasksPostRequest** | **ApiComputeTasksPostRequest** |             |       |

### Return type

**ApiComputeTasksPost200Response**

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

# **apiComputeTasksTaskIdStatusGet**

> ApiComputeTasksTaskIdStatusGet200Response apiComputeTasksTaskIdStatusGet()

Retrieve the status of a specific compute task

### Example

```typescript
import { ComputeApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new ComputeApi(configuration);

let taskId: string; //Task identifier (default to undefined)

const { status, data } = await apiInstance.apiComputeTasksTaskIdStatusGet(taskId);
```

### Parameters

| Name       | Type         | Description     | Notes                 |
| ---------- | ------------ | --------------- | --------------------- |
| **taskId** | [**string**] | Task identifier | defaults to undefined |

### Return type

**ApiComputeTasksTaskIdStatusGet200Response**

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
