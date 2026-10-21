# \AimlApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                            | HTTP request                 | Description                 |
| ----------------------------------------------------------------- | ---------------------------- | --------------------------- |
| [**api_aiml_inference_post**](AimlApi.md#api_aiml_inference_post) | **POST** /api/aiml/inference | Run AI/ML inference         |
| [**api_aiml_models_get**](AimlApi.md#api_aiml_models_get)         | **GET** /api/aiml/models     | List available AI/ML models |

## api_aiml_inference_post

> models::ApiAimlInferencePost200Response api_aiml_inference_post(api_aiml_inference_post_request)
> Run AI/ML inference

Execute AI/ML inference on the compute network

### Parameters

| Name                                | Type                                                              | Description | Required   | Notes |
| ----------------------------------- | ----------------------------------------------------------------- | ----------- | ---------- | ----- |
| **api_aiml_inference_post_request** | [**ApiAimlInferencePostRequest**](ApiAimlInferencePostRequest.md) |             | [required] |

### Return type

[**models::ApiAimlInferencePost200Response**](_api_aiml_inference_post_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## api_aiml_models_get

> models::ApiAimlModelsGet200Response api_aiml_models_get() List available AI/ML models

Retrieve list of available AI/ML models for computation

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::ApiAimlModelsGet200Response**](_api_aiml_models_get_200_response.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
