# WeatherDataApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                          | HTTP request                         | Description                 |
| --------------------------------------------------------------- | ------------------------------------ | --------------------------- |
| [**apiWeatherDataGet**](#apiweatherdataget)                     | **GET** /api/weather/data            | Get weather data            |
| [**apiWeatherSecurityStatusGet**](#apiweathersecuritystatusget) | **GET** /api/weather/security-status | Get weather security status |
| [**apiWeatherSubmitPost**](#apiweathersubmitpost)               | **POST** /api/weather/submit         | Submit weather data         |

# **apiWeatherDataGet**

> Array<WeatherData> apiWeatherDataGet()

### Example

```typescript
import { WeatherDataApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new WeatherDataApi(configuration);

let stationId: string; // (optional) (default to undefined)
let startDate: string; // (optional) (default to undefined)
let endDate: string; // (optional) (default to undefined)
let dataType: 'temperature' | 'humidity' | 'pressure' | 'wind' | 'precipitation'; // (optional) (default to undefined)

const { status, data } = await apiInstance.apiWeatherDataGet(
  stationId,
  startDate,
  endDate,
  dataType
);
```

### Parameters

| Name          | Type                       | Description        | Notes                            |
| ------------- | -------------------------- | ------------------ | -------------------------------- | -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --- | -------------------------------- |
| **stationId** | [**string**]               |                    | (optional) defaults to undefined |
| **startDate** | [**string**]               |                    | (optional) defaults to undefined |
| **endDate**   | [**string**]               |                    | (optional) defaults to undefined |
| **dataType**  | [\*\*&#39;temperature&#39; | &#39;humidity&#39; | &#39;pressure&#39;               | &#39;wind&#39; | &#39;precipitation&#39;**]**Array<&#39;temperature&#39; &#124; &#39;humidity&#39; &#124; &#39;pressure&#39; &#124; &#39;wind&#39; &#124; &#39;precipitation&#39;>\*\* |     | (optional) defaults to undefined |

### Return type

**Array<WeatherData>**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | Weather data retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiWeatherSecurityStatusGet**

> WeatherSecurityStatus apiWeatherSecurityStatusGet()

Retrieve security status of weather stations and data integrity

### Example

```typescript
import { WeatherDataApi, Configuration } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new WeatherDataApi(configuration);

const { status, data } = await apiInstance.apiWeatherSecurityStatusGet();
```

### Parameters

This endpoint does not have any parameters.

### Return type

**WeatherSecurityStatus**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                    | Response headers |
| ----------- | ---------------------------------------------- | ---------------- |
| **200**     | Weather security status retrieved successfully | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiWeatherSubmitPost**

> WeatherSubmissionResponse apiWeatherSubmitPost(weatherDataSubmission)

Submit environmental data from weather stations

### Example

```typescript
import { WeatherDataApi, Configuration, WeatherDataSubmission } from '@wrfcoin/sdk-typescript';

const configuration = new Configuration();
const apiInstance = new WeatherDataApi(configuration);

let weatherDataSubmission: WeatherDataSubmission; //

const { status, data } = await apiInstance.apiWeatherSubmitPost(weatherDataSubmission);
```

### Parameters

| Name                      | Type                      | Description | Notes |
| ------------------------- | ------------------------- | ----------- | ----- |
| **weatherDataSubmission** | **WeatherDataSubmission** |             |       |

### Return type

**WeatherSubmissionResponse**

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | Weather data submitted successfully | -                |
| **400**     | Invalid weather data                | -                |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
