# WeatherDataSubmission

## Properties

| Name             | Type                                                                                            | Description                | Notes      |
| ---------------- | ----------------------------------------------------------------------------------------------- | -------------------------- | ---------- |
| **stationId**    | **String**                                                                                      | Weather station identifier |            |
| **timestamp**    | **Integer**                                                                                     | Data collection timestamp  | [optional] |
| **location**     | [**Location**](Location.md)                                                                     |                            | [optional] |
| **measurements** | [**List&lt;WeatherMeasurement&gt;**](WeatherMeasurement.md)                                     |                            | [optional] |
| **signature**    | **String**                                                                                      | Data integrity signature   | [optional] |
| **sensorData**   | [**List&lt;WeatherDataSubmissionSensorDataInner&gt;**](WeatherDataSubmissionSensorDataInner.md) | Array of sensor readings   |            |
| **quality**      | [**WeatherDataSubmissionQuality**](WeatherDataSubmissionQuality.md)                             |                            | [optional] |
