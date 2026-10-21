# WeatherSubmissionResponseData

## Properties

| Name           | Type                                                                                                      | Description                | Notes      |
| -------------- | --------------------------------------------------------------------------------------------------------- | -------------------------- | ---------- |
| **stationId**  | **String**                                                                                                | Weather station identifier | [optional] |
| **timestamp**  | **OffsetDateTime**                                                                                        | Submission timestamp       | [optional] |
| **location**   | [**Location**](Location.md)                                                                               |                            | [optional] |
| **sensors**    | [**List&lt;WeatherSubmissionResponseDataSensorsInner&gt;**](WeatherSubmissionResponseDataSensorsInner.md) |                            | [optional] |
| **quality**    | [**WeatherSubmissionResponseDataQuality**](WeatherSubmissionResponseDataQuality.md)                       |                            | [optional] |
| **rewards**    | [**WeatherSubmissionResponseDataRewards**](WeatherSubmissionResponseDataRewards.md)                       |                            | [optional] |
| **blockchain** | [**WeatherSubmissionResponseDataBlockchain**](WeatherSubmissionResponseDataBlockchain.md)                 |                            | [optional] |
