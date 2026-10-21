# ComputeAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                             | HTTP request                               | Description             |
| ---------------------------------------------------------------------------------- | ------------------------------------------ | ----------------------- |
| [**apiComputeProvidersGet**](ComputeAPI.md#apicomputeprovidersget)                 | **GET** /api/compute/providers             | List compute providers  |
| [**apiComputeTasksPost**](ComputeAPI.md#apicomputetaskspost)                       | **POST** /api/compute/tasks                | Submit compute task     |
| [**apiComputeTasksTaskIdStatusGet**](ComputeAPI.md#apicomputetaskstaskidstatusget) | **GET** /api/compute/tasks/{taskId}/status | Get compute task status |

# **apiComputeProvidersGet**

```swift
    open class func apiComputeProvidersGet(completion: @escaping (_ data: ApiComputeProvidersGet200Response?, _ error: Error?) -> Void)
```

List compute providers

Retrieve list of available compute providers

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// List compute providers
ComputeAPI.apiComputeProvidersGet() { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
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

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiComputeTasksPost**

```swift
    open class func apiComputeTasksPost(apiComputeTasksPostRequest: ApiComputeTasksPostRequest, completion: @escaping (_ data: ApiComputeTasksPost200Response?, _ error: Error?) -> Void)
```

Submit compute task

Submit a distributed computing task to the network

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let apiComputeTasksPostRequest = _api_compute_tasks_post_request(taskType: "taskType_example", priority: "priority_example", resourceRequirements: _api_compute_tasks_post_request_resourceRequirements(cpu: 123, memory: 123, storage: 123, gpu: false)) // ApiComputeTasksPostRequest |

// Submit compute task
ComputeAPI.apiComputeTasksPost(apiComputeTasksPostRequest: apiComputeTasksPostRequest) { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
```

### Parameters

| Name                           | Type                                                            | Description | Notes |
| ------------------------------ | --------------------------------------------------------------- | ----------- | ----- |
| **apiComputeTasksPostRequest** | [**ApiComputeTasksPostRequest**](ApiComputeTasksPostRequest.md) |             |

### Return type

[**ApiComputeTasksPost200Response**](ApiComputeTasksPost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiComputeTasksTaskIdStatusGet**

```swift
    open class func apiComputeTasksTaskIdStatusGet(taskId: String, completion: @escaping (_ data: ApiComputeTasksTaskIdStatusGet200Response?, _ error: Error?) -> Void)
```

Get compute task status

Retrieve the status of a specific compute task

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let taskId = "taskId_example" // String | Task identifier

// Get compute task status
ComputeAPI.apiComputeTasksTaskIdStatusGet(taskId: taskId) { (response, error) in
    guard error == nil else {
        print(error)
        return
    }

    if (response) {
        dump(response)
    }
}
```

### Parameters

| Name       | Type       | Description     | Notes |
| ---------- | ---------- | --------------- | ----- |
| **taskId** | **String** | Task identifier |

### Return type

[**ApiComputeTasksTaskIdStatusGet200Response**](ApiComputeTasksTaskIdStatusGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
