# AIMLAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                      | HTTP request                 | Description                 |
| ----------------------------------------------------------- | ---------------------------- | --------------------------- |
| [**apiAimlInferencePost**](AIMLAPI.md#apiaimlinferencepost) | **POST** /api/aiml/inference | Run AI/ML inference         |
| [**apiAimlModelsGet**](AIMLAPI.md#apiaimlmodelsget)         | **GET** /api/aiml/models     | List available AI/ML models |

# **apiAimlInferencePost**

```swift
    open class func apiAimlInferencePost(apiAimlInferencePostRequest: ApiAimlInferencePostRequest, completion: @escaping (_ data: ApiAimlInferencePost200Response?, _ error: Error?) -> Void)
```

Run AI/ML inference

Execute AI/ML inference on the compute network

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let apiAimlInferencePostRequest = _api_aiml_inference_post_request(modelId: "modelId_example", input: 123, parameters: 123) // ApiAimlInferencePostRequest |

// Run AI/ML inference
AIMLAPI.apiAimlInferencePost(apiAimlInferencePostRequest: apiAimlInferencePostRequest) { (response, error) in
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

| Name                            | Type                                                              | Description | Notes |
| ------------------------------- | ----------------------------------------------------------------- | ----------- | ----- |
| **apiAimlInferencePostRequest** | [**ApiAimlInferencePostRequest**](ApiAimlInferencePostRequest.md) |             |

### Return type

[**ApiAimlInferencePost200Response**](ApiAimlInferencePost200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiAimlModelsGet**

```swift
    open class func apiAimlModelsGet(completion: @escaping (_ data: ApiAimlModelsGet200Response?, _ error: Error?) -> Void)
```

List available AI/ML models

Retrieve list of available AI/ML models for computation

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// List available AI/ML models
AIMLAPI.apiAimlModelsGet() { (response, error) in
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

[**ApiAimlModelsGet200Response**](ApiAimlModelsGet200Response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
