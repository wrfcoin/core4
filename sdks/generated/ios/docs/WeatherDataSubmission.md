# WeatherDataSubmission

## Properties

| Name             | Type                                                                | Description                | Notes      |
| ---------------- | ------------------------------------------------------------------- | -------------------------- | ---------- |
| **stationId**    | **String**                                                          | Weather station identifier |
| **timestamp**    | **Int**                                                             | Data collection timestamp  | [optional] |
| **location**     | [**Location**](Location.md)                                         |                            | [optional] |
| **measurements** | [WeatherMeasurement]                                                |                            | [optional] |
| **signature**    | **String**                                                          | Data integrity signature   | [optional] |
| **sensorData**   | [WeatherDataSubmissionSensorDataInner]                              | Array of sensor readings   |
| **quality**      | [**WeatherDataSubmissionQuality**](WeatherDataSubmissionQuality.md) |                            | [optional] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
