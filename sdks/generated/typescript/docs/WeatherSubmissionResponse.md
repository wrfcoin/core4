# WeatherSubmissionResponse

## Properties

| Name          | Type                                                                  | Description                           | Notes                             |
| ------------- | --------------------------------------------------------------------- | ------------------------------------- | --------------------------------- |
| **success**   | **boolean**                                                           | Whether the submission was successful | [optional] [default to undefined] |
| **data**      | [**WeatherSubmissionResponseData**](WeatherSubmissionResponseData.md) |                                       | [optional] [default to undefined] |
| **message**   | **string**                                                            | Response message                      | [optional] [default to undefined] |
| **timestamp** | **string**                                                            | Response timestamp                    | [optional] [default to undefined] |

## Example

```typescript
import { WeatherSubmissionResponse } from '@wrfcoin/sdk-typescript';

const instance: WeatherSubmissionResponse = {
  success,
  data,
  message,
  timestamp,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
