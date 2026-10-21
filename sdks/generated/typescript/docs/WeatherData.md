# WeatherData

## Properties

| Name             | Type                                            | Description | Notes                             |
| ---------------- | ----------------------------------------------- | ----------- | --------------------------------- |
| **id**           | **string**                                      |             | [optional] [default to undefined] |
| **stationId**    | **string**                                      |             | [optional] [default to undefined] |
| **timestamp**    | **number**                                      |             | [optional] [default to undefined] |
| **location**     | [**Location**](Location.md)                     |             | [optional] [default to undefined] |
| **measurement**  | [**WeatherMeasurement**](WeatherMeasurement.md) |             | [optional] [default to undefined] |
| **qualityScore** | **number**                                      |             | [optional] [default to undefined] |
| **rewardAmount** | **string**                                      |             | [optional] [default to undefined] |

## Example

```typescript
import { WeatherData } from '@wrfcoin/sdk-typescript';

const instance: WeatherData = {
  id,
  stationId,
  timestamp,
  location,
  measurement,
  qualityScore,
  rewardAmount,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
