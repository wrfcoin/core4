# WeatherDataSubmission

## Properties

| Name             | Type                                                                                               | Description                | Notes      |
| ---------------- | -------------------------------------------------------------------------------------------------- | -------------------------- | ---------- |
| **station_id**   | **String**                                                                                         | Weather station identifier |
| **timestamp**    | Option<**i32**>                                                                                    | Data collection timestamp  | [optional] |
| **location**     | Option<[**models::Location**](Location.md)>                                                        |                            | [optional] |
| **measurements** | Option<[**Vec<models::WeatherMeasurement>**](WeatherMeasurement.md)>                               |                            | [optional] |
| **signature**    | Option<**String**>                                                                                 | Data integrity signature   | [optional] |
| **sensor_data**  | [**Vec<models::WeatherDataSubmissionSensorDataInner>**](WeatherDataSubmission_sensorData_inner.md) | Array of sensor readings   |
| **quality**      | Option<[**models::WeatherDataSubmissionQuality**](WeatherDataSubmission_quality.md)>               |                            | [optional] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
