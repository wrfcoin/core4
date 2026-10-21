# WeatherSubmissionResponseData

## Properties

| Name           | Type                                                                                                       | Description                | Notes                             |
| -------------- | ---------------------------------------------------------------------------------------------------------- | -------------------------- | --------------------------------- |
| **stationId**  | **string**                                                                                                 | Weather station identifier | [optional] [default to undefined] |
| **timestamp**  | **string**                                                                                                 | Submission timestamp       | [optional] [default to undefined] |
| **location**   | [**Location**](Location.md)                                                                                |                            | [optional] [default to undefined] |
| **sensors**    | [**Array&lt;WeatherSubmissionResponseDataSensorsInner&gt;**](WeatherSubmissionResponseDataSensorsInner.md) |                            | [optional] [default to undefined] |
| **quality**    | [**WeatherSubmissionResponseDataQuality**](WeatherSubmissionResponseDataQuality.md)                        |                            | [optional] [default to undefined] |
| **rewards**    | [**WeatherSubmissionResponseDataRewards**](WeatherSubmissionResponseDataRewards.md)                        |                            | [optional] [default to undefined] |
| **blockchain** | [**WeatherSubmissionResponseDataBlockchain**](WeatherSubmissionResponseDataBlockchain.md)                  |                            | [optional] [default to undefined] |

## Example

```typescript
import { WeatherSubmissionResponseData } from '@wrfcoin/sdk-typescript';

const instance: WeatherSubmissionResponseData = {
  stationId,
  timestamp,
  location,
  sensors,
  quality,
  rewards,
  blockchain,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
