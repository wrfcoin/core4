# WeatherSecurityStatus

## Properties

| Name                       | Type                                                                              | Description | Notes                             |
| -------------------------- | --------------------------------------------------------------------------------- | ----------- | --------------------------------- |
| **station_security**       | **string**                                                                        |             | [optional] [default to undefined] |
| **data_integrity**         | **number**                                                                        |             | [optional] [default to undefined] |
| **transmission_encrypted** | **boolean**                                                                       |             | [optional] [default to undefined] |
| **last_security_check**    | **string**                                                                        |             | [optional] [default to undefined] |
| **active_stations**        | **number**                                                                        |             | [optional] [default to undefined] |
| **secure_stations**        | **number**                                                                        |             | [optional] [default to undefined] |
| **flagged_stations**       | **number**                                                                        |             | [optional] [default to undefined] |
| **authentication_status**  | **string**                                                                        |             | [optional] [default to undefined] |
| **api_security**           | [**WeatherSecurityStatusApiSecurity**](WeatherSecurityStatusApiSecurity.md)       |             | [optional] [default to undefined] |
| **data_validation**        | [**WeatherSecurityStatusDataValidation**](WeatherSecurityStatusDataValidation.md) |             | [optional] [default to undefined] |

## Example

```typescript
import { WeatherSecurityStatus } from '@wrfcoin/sdk-typescript';

const instance: WeatherSecurityStatus = {
  station_security,
  data_integrity,
  transmission_encrypted,
  last_security_check,
  active_stations,
  secure_stations,
  flagged_stations,
  authentication_status,
  api_security,
  data_validation,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
