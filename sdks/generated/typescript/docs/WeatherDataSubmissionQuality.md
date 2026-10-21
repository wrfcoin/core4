# WeatherDataSubmissionQuality

## Properties

| Name       | Type                    | Description             | Notes                             |
| ---------- | ----------------------- | ----------------------- | --------------------------------- |
| **score**  | **number**              | Data quality score      | [optional] [default to undefined] |
| **issues** | **Array&lt;string&gt;** | Quality issues detected | [optional] [default to undefined] |

## Example

```typescript
import { WeatherDataSubmissionQuality } from '@wrfcoin/sdk-typescript';

const instance: WeatherDataSubmissionQuality = {
  score,
  issues,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
