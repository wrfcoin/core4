# WeatherDataSubmission

## Properties

| Name             | Type                                                                                             | Description                | Notes                             |
| ---------------- | ------------------------------------------------------------------------------------------------ | -------------------------- | --------------------------------- |
| **stationId**    | **string**                                                                                       | Weather station identifier | [default to undefined]            |
| **timestamp**    | **number**                                                                                       | Data collection timestamp  | [optional] [default to undefined] |
| **location**     | [**Location**](Location.md)                                                                      |                            | [optional] [default to undefined] |
| **measurements** | [**Array&lt;WeatherMeasurement&gt;**](WeatherMeasurement.md)                                     |                            | [optional] [default to undefined] |
| **signature**    | **string**                                                                                       | Data integrity signature   | [optional] [default to undefined] |
| **sensorData**   | [**Array&lt;WeatherDataSubmissionSensorDataInner&gt;**](WeatherDataSubmissionSensorDataInner.md) | Array of sensor readings   | [default to undefined]            |
| **quality**      | [**WeatherDataSubmissionQuality**](WeatherDataSubmissionQuality.md)                              |                            | [optional] [default to undefined] |

## Example

```typescript
import { WeatherDataSubmission } from '@wrfcoin/sdk-typescript';

const instance: WeatherDataSubmission = {
  stationId,
  timestamp,
  location,
  measurements,
  signature,
  sensorData,
  quality,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
