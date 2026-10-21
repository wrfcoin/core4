# WeatherSubmissionResponseData

## Properties

| Name           | Type                                                                                                                  | Description                | Notes      |
| -------------- | --------------------------------------------------------------------------------------------------------------------- | -------------------------- | ---------- |
| **station_id** | Option<**String**>                                                                                                    | Weather station identifier | [optional] |
| **timestamp**  | Option<**String**>                                                                                                    | Submission timestamp       | [optional] |
| **location**   | Option<[**models::Location**](Location.md)>                                                                           |                            | [optional] |
| **sensors**    | Option<[**Vec<models::WeatherSubmissionResponseDataSensorsInner>**](WeatherSubmissionResponse_data_sensors_inner.md)> |                            | [optional] |
| **quality**    | Option<[**models::WeatherSubmissionResponseDataQuality**](WeatherSubmissionResponse_data_quality.md)>                 |                            | [optional] |
| **rewards**    | Option<[**models::WeatherSubmissionResponseDataRewards**](WeatherSubmissionResponse_data_rewards.md)>                 |                            | [optional] |
| **blockchain** | Option<[**models::WeatherSubmissionResponseDataBlockchain**](WeatherSubmissionResponse_data_blockchain.md)>           |                            | [optional] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
