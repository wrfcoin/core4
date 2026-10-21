# IntegrationsApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                          | HTTP request                                 | Description                      |
| ------------------------------------------------------------------------------- | -------------------------------------------- | -------------------------------- |
| [**apiIntegrationsLangchainEnablePost**](#apiintegrationslangchainenablepost)   | **POST** /api/integrations/langchain/enable  | Enable LangChain integration     |
| [**apiIntegrationsLangchainInsightsGet**](#apiintegrationslangchaininsightsget) | **GET** /api/integrations/langchain/insights | Get LangChain insights           |
| [**apiIntegrationsLangchainStatusGet**](#apiintegrationslangchainstatusget)     | **GET** /api/integrations/langchain/status   | Get LangChain integration status |

# **apiIntegrationsLangchainEnablePost**

> ApiIntegrationsLangchainEnablePost200Response apiIntegrationsLangchainEnablePost()

Enable LangChain analytics integration

### Example

```typescript
import { IntegrationsApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new IntegrationsApi(configuration);

const { status, data } = await apiInstance.apiIntegrationsLangchainEnablePost();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**ApiIntegrationsLangchainEnablePost200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                | Response headers |
| ----------- | ------------------------------------------ | ---------------- |
| **200**     | LangChain integration enabled successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiIntegrationsLangchainInsightsGet**

> ApiIntegrationsLangchainInsightsGet200Response apiIntegrationsLangchainInsightsGet()

Retrieve AI-powered insights from LangChain analytics

### Example

```typescript
import { IntegrationsApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new IntegrationsApi(configuration);

const { status, data } = await apiInstance.apiIntegrationsLangchainInsightsGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**ApiIntegrationsLangchainInsightsGet200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                               | Response headers |
| ----------- | ----------------------------------------- | ---------------- |
| **200**     | LangChain insights retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiIntegrationsLangchainStatusGet**

> ApiIntegrationsLangchainStatusGet200Response apiIntegrationsLangchainStatusGet()

Retrieve status of LangChain analytics integration

### Example

```typescript
import { IntegrationsApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new IntegrationsApi(configuration);

const { status, data } = await apiInstance.apiIntegrationsLangchainStatusGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**ApiIntegrationsLangchainStatusGet200Response**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                         | Response headers |
| ----------- | --------------------------------------------------- | ---------------- |
| **200**     | LangChain integration status retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
