# WeatherSecurityStatusDataValidation

## Properties

| Name                    | Type        | Description | Notes                             |
| ----------------------- | ----------- | ----------- | --------------------------------- |
| **sensor_checks**       | **boolean** |             | [optional] [default to undefined] |
| **anomaly_detection**   | **boolean** |             | [optional] [default to undefined] |
| **data_sanitization**   | **boolean** |             | [optional] [default to undefined] |
| **backup_verification** | **boolean** |             | [optional] [default to undefined] |

## Example

```typescript
import { WeatherSecurityStatusDataValidation } from '@wrfcoin/sdk-typescript';

const instance: WeatherSecurityStatusDataValidation = {
  sensor_checks,
  anomaly_detection,
  data_sanitization,
  backup_verification,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
