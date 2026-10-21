# \ComputeApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                                         | HTTP request                               | Description             |
| ---------------------------------------------------------------------------------------------- | ------------------------------------------ | ----------------------- |
| [**api_compute_providers_get**](ComputeApi.md#api_compute_providers_get)                       | **GET** /api/compute/providers             | List compute providers  |
| [**api_compute_tasks_post**](ComputeApi.md#api_compute_tasks_post)                             | **POST** /api/compute/tasks                | Submit compute task     |
| [**api_compute_tasks_task_id_status_get**](ComputeApi.md#api_compute_tasks_task_id_status_get) | **GET** /api/compute/tasks/{taskId}/status | Get compute task status |

## api_compute_providers_get

> models::ApiComputeProvidersGet200Response api_compute_providers_get() List compute providers

Retrieve list of available compute providers

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::ApiComputeProvidersGet200Response**](_api_compute_providers_get_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_compute_tasks_post

> models::ApiComputeTasksPost200Response api_compute_tasks_post(api_compute_tasks_post_request)
> Submit compute task

Submit a distributed computing task to the network

### Parameters

| Name                               | Type                                                            | Description | Required   | Notes |
| ---------------------------------- | --------------------------------------------------------------- | ----------- | ---------- | ----- |
| **api_compute_tasks_post_request** | [**ApiComputeTasksPostRequest**](ApiComputeTasksPostRequest.md) |             | [required] |

### Return type

[**models::ApiComputeTasksPost200Response**](_api_compute_tasks_post_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_compute_tasks_task_id_status_get

> models::ApiComputeTasksTaskIdStatusGet200Response api_compute_tasks_task_id_status_get(task_id)
> Get compute task status

Retrieve the status of a specific compute task

### Parameters

| Name        | Type       | Description     | Required   | Notes |
| ----------- | ---------- | --------------- | ---------- | ----- |
| **task_id** | **String** | Task identifier | [required] |

### Return type

[**models::ApiComputeTasksTaskIdStatusGet200Response**](_api_compute_tasks__taskId__status_get_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
