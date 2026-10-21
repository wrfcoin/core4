# WeatherDataAPI

All URIs are relative to *https://api.wrfcoin.com/v1*

| Method                                                                           | HTTP request                         | Description                 |
| -------------------------------------------------------------------------------- | ------------------------------------ | --------------------------- |
| [**apiWeatherDataGet**](WeatherDataAPI.md#apiweatherdataget)                     | **GET** /api/weather/data            | Get weather data            |
| [**apiWeatherSecurityStatusGet**](WeatherDataAPI.md#apiweathersecuritystatusget) | **GET** /api/weather/security-status | Get weather security status |
| [**apiWeatherSubmitPost**](WeatherDataAPI.md#apiweathersubmitpost)               | **POST** /api/weather/submit         | Submit weather data         |

# **apiWeatherDataGet**

```swift
    open class func apiWeatherDataGet(stationId: String? = nil, startDate: Date? = nil, endDate: Date? = nil, dataType: DataType_apiWeatherDataGet? = nil, completion: @escaping (_ data: [WeatherData]?, _ error: Error?) -> Void)
```

Get weather data

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let stationId = "stationId_example" // String |  (optional)
let startDate = Date() // Date |  (optional)
let endDate = Date() // Date |  (optional)
let dataType = "dataType_example" // String |  (optional)

// Get weather data
WeatherDataAPI.apiWeatherDataGet(stationId: stationId, startDate: startDate, endDate: endDate, dataType: dataType) { (response, error) in
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

| Name          | Type       | Description | Notes      |
| ------------- | ---------- | ----------- | ---------- |
| **stationId** | **String** |             | [optional] |
| **startDate** | **Date**   |             | [optional] |
| **endDate**   | **Date**   |             | [optional] |
| **dataType**  | **String** |             | [optional] |

### Return type

[**[WeatherData]**](WeatherData.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiWeatherSecurityStatusGet**

```swift
    open class func apiWeatherSecurityStatusGet(completion: @escaping (_ data: WeatherSecurityStatus?, _ error: Error?) -> Void)
```

Get weather security status

Retrieve security status of weather stations and data integrity

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK


// Get weather security status
WeatherDataAPI.apiWeatherSecurityStatusGet() { (response, error) in
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

[**WeatherSecurityStatus**](WeatherSecurityStatus.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth), [BearerAuth](../README.md#BearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **apiWeatherSubmitPost**

```swift
    open class func apiWeatherSubmitPost(weatherDataSubmission: WeatherDataSubmission, completion: @escaping (_ data: WeatherSubmissionResponse?, _ error: Error?) -> Void)
```

Submit weather data

Submit environmental data from weather stations

### Example

```swift
// The following code samples are still beta. For any issue, please report via http://github.com/OpenAPITools/openapi-generator/issues/new
import WRFCoinSDK

let weatherDataSubmission = WeatherDataSubmission(stationId: "stationId_example", timestamp: 123, location: Location(latitude: 123, longitude: 123, altitude: 123, accuracy: 123), measurements: [WeatherMeasurement(type: "type_example", value: 123, unit: "unit_example", uncertainty: 123)], signature: "signature_example", sensorData: [WeatherDataSubmission_sensorData_inner(type: "type_example", value: 123, unit: "unit_example", status: "status_example")], quality: WeatherDataSubmission_quality(score: 123, issues: ["issues_example"])) // WeatherDataSubmission |

// Submit weather data
WeatherDataAPI.apiWeatherSubmitPost(weatherDataSubmission: weatherDataSubmission) { (response, error) in
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

| Name                      | Type                                                  | Description | Notes |
| ------------------------- | ----------------------------------------------------- | ----------- | ----- |
| **weatherDataSubmission** | [**WeatherDataSubmission**](WeatherDataSubmission.md) |             |

### Return type

[**WeatherSubmissionResponse**](WeatherSubmissionResponse.md)

### Authorization

[ApiKeyAuth](../README.md#ApiKeyAuth)

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints)
[[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
