# WeatherDataApi

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                           | HTTP request                         | Description                 |
| -------------------------------------------------------------------------------- | ------------------------------------ | --------------------------- |
| [**apiWeatherDataGet**](WeatherDataApi.md#apiWeatherDataGet)                     | **GET** /api/weather/data            | Get weather data            |
| [**apiWeatherSecurityStatusGet**](WeatherDataApi.md#apiWeatherSecurityStatusGet) | **GET** /api/weather/security-status | Get weather security status |
| [**apiWeatherSubmitPost**](WeatherDataApi.md#apiWeatherSubmitPost)               | **POST** /api/weather/submit         | Submit weather data         |

## apiWeatherDataGet

> List&lt;WeatherData&gt; apiWeatherDataGet(stationId, startDate, endDate, dataType)

Get weather data

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.WeatherDataApi;

public class Example {
    public static void main(String[] args) {
        ApiClient defaultClient = Configuration.getDefaultApiClient();
        defaultClient.setBasePath("https://api.wrfcoin.com/v1");

        // Configure API key authorization: ApiKeyAuth
        ApiKeyAuth ApiKeyAuth = (ApiKeyAuth) defaultClient.getAuthentication("ApiKeyAuth");
        ApiKeyAuth.setApiKey("YOUR API KEY");
        // Uncomment the following line to set a prefix for the API key, e.g. "Token" (defaults to null)
        //ApiKeyAuth.setApiKeyPrefix("Token");

        // Configure HTTP bearer authorization: BearerAuth
        HttpBearerAuth BearerAuth = (HttpBearerAuth) defaultClient.getAuthentication("BearerAuth");
        BearerAuth.setBearerToken("BEARER TOKEN");

        WeatherDataApi apiInstance = new WeatherDataApi(defaultClient);
        String stationId = "stationId_example"; // String |
        OffsetDateTime startDate = OffsetDateTime.now(); // OffsetDateTime |
        OffsetDateTime endDate = OffsetDateTime.now(); // OffsetDateTime |
        String dataType = "temperature"; // String |
        try {
            List<WeatherData> result = apiInstance.apiWeatherDataGet(stationId, startDate, endDate, dataType);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling WeatherDataApi#apiWeatherDataGet");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name          | Type               | Description | Notes                                                                   |
| ------------- | ------------------ | ----------- | ----------------------------------------------------------------------- |
| **stationId** | **String**         |             | [optional]                                                              |
| **startDate** | **OffsetDateTime** |             | [optional]                                                              |
| **endDate**   | **OffsetDateTime** |             | [optional]                                                              |
| **dataType**  | **String**         |             | [optional] [enum: temperature, humidity, pressure, wind, precipitation] |

### Return type

[**List&lt;WeatherData&gt;**](WeatherData.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                         | Response headers |
| ----------- | ----------------------------------- | ---------------- |
| **200**     | Weather data retrieved successfully | -                |

## apiWeatherSecurityStatusGet

> WeatherSecurityStatus apiWeatherSecurityStatusGet()

Get weather security status

Retrieve security status of weather stations and data integrity

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.WeatherDataApi;

public class Example {
    public static void main(String[] args) {
        ApiClient defaultClient = Configuration.getDefaultApiClient();
        defaultClient.setBasePath("https://api.wrfcoin.com/v1");

        // Configure API key authorization: ApiKeyAuth
        ApiKeyAuth ApiKeyAuth = (ApiKeyAuth) defaultClient.getAuthentication("ApiKeyAuth");
        ApiKeyAuth.setApiKey("YOUR API KEY");
        // Uncomment the following line to set a prefix for the API key, e.g. "Token" (defaults to null)
        //ApiKeyAuth.setApiKeyPrefix("Token");

        // Configure HTTP bearer authorization: BearerAuth
        HttpBearerAuth BearerAuth = (HttpBearerAuth) defaultClient.getAuthentication("BearerAuth");
        BearerAuth.setBearerToken("BEARER TOKEN");

        WeatherDataApi apiInstance = new WeatherDataApi(defaultClient);
        try {
            WeatherSecurityStatus result = apiInstance.apiWeatherSecurityStatusGet();
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling WeatherDataApi#apiWeatherSecurityStatusGet");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**WeatherSecurityStatus**](WeatherSecurityStatus.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

### HTTP response details

| Status code | Description                                    | Response headers |
| ----------- | ---------------------------------------------- | ---------------- |
| **200**     | Weather security status retrieved successfully | -                |

## apiWeatherSubmitPost

> WeatherSubmissionResponse apiWeatherSubmitPost(weatherDataSubmission)

Submit weather data

Submit environmental data from weather stations

### Example

```java
// Import classes:
import com.wrfcoin.sdk.ApiClient;
import com.wrfcoin.sdk.ApiException;
import com.wrfcoin.sdk.Configuration;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.models.*;
import com.wrfcoin.sdk.api.WeatherDataApi;

public class Example {
    public static void main(String[] args) {
        ApiClient defaultClient = Configuration.getDefaultApiClient();
        defaultClient.setBasePath("https://api.wrfcoin.com/v1");

        // Configure API key authorization: ApiKeyAuth
        ApiKeyAuth ApiKeyAuth = (ApiKeyAuth) defaultClient.getAuthentication("ApiKeyAuth");
        ApiKeyAuth.setApiKey("YOUR API KEY");
        // Uncomment the following line to set a prefix for the API key, e.g. "Token" (defaults to null)
        //ApiKeyAuth.setApiKeyPrefix("Token");

        WeatherDataApi apiInstance = new WeatherDataApi(defaultClient);
        WeatherDataSubmission weatherDataSubmission = new WeatherDataSubmission(); // WeatherDataSubmission |
        try {
            WeatherSubmissionResponse result = apiInstance.apiWeatherSubmitPost(weatherDataSubmission);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling WeatherDataApi#apiWeatherSubmitPost");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}
```

### Parameters

| Name                      | Type                                                  | Description | Notes |
| ------------------------- | ----------------------------------------------------- | ----------- | ----- |
| **weatherDataSubmission** | [**WeatherDataSubmission**](WeatherDataSubmission.md) |             |       |

### Return type

[**WeatherSubmissionResponse**](WeatherSubmissionResponse.md)

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
