# AIMLApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                            | HTTP request                 | Description                 |
| ------------------------------------------------- | ---------------------------- | --------------------------- |
| [**apiAimlInferencePost**](#apiaimlinferencepost) | **POST** /api/aiml/inference | Run AI/ML inference         |
| [**apiAimlModelsGet**](#apiaimlmodelsget)         | **GET** /api/aiml/models     | List available AI/ML models |

# **apiAimlInferencePost**

> ApiAimlInferencePost200Response apiAimlInferencePost(apiAimlInferencePostRequest)

Execute AI/ML inference on the compute network

### Example

```typescript
import { AIMLApi, Configuration, ApiAimlInferencePostRequest } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new AIMLApi(configuration);

let apiAimlInferencePostRequest: ApiAimlInferencePostRequest; //

const { status, data } = await apiInstance.apiAimlInferencePost(apiAimlInferencePostRequest);
```

### Parameters

| Name                            | Type                            | Description | Notes |
| ------------------------------- | ------------------------------- | ----------- | ----- |
| **apiAimlInferencePostRequest** | **ApiAimlInferencePostRequest** |             |       |

### Return type

**ApiAimlInferencePost200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                      | Response headers |
| ----------- | -------------------------------- | ---------------- |
| **200**     | Inference completed successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiAimlModelsGet**

> ApiAimlModelsGet200Response apiAimlModelsGet()

Retrieve list of available AI/ML models for computation

### Example

```typescript
import { AIMLApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new AIMLApi(configuration);

const { status, data } = await apiInstance.apiAimlModelsGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**ApiAimlModelsGet200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | AI/ML models retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
