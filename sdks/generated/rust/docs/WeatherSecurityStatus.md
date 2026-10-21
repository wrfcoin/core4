# WeatherSecurityStatus

## Properties

| Name                       | Type                                                                                                | Description | Notes      |
| -------------------------- | --------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| **station_security**       | Option<**String**>                                                                                  |             | [optional] |
| **data_integrity**         | Option<**f64**>                                                                                     |             | [optional] |
| **transmission_encrypted** | Option<**bool**>                                                                                    |             | [optional] |
| **last_security_check**    | Option<**String**>                                                                                  |             | [optional] |
| **active_stations**        | Option<**i32**>                                                                                     |             | [optional] |
| **secure_stations**        | Option<**i32**>                                                                                     |             | [optional] |
| **flagged_stations**       | Option<**i32**>                                                                                     |             | [optional] |
| **authentication_status**  | Option<**String**>                                                                                  |             | [optional] |
| **api_security**           | Option<[**models::WeatherSecurityStatusApiSecurity**](WeatherSecurityStatus_api_security.md)>       |             | [optional] |
| **data_validation**        | Option<[**models::WeatherSecurityStatusDataValidation**](WeatherSecurityStatus_data_validation.md)> |             | [optional] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
