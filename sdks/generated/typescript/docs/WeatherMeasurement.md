# WeatherMeasurement

## Properties

| Name            | Type       | Description             | Notes                             |
| --------------- | ---------- | ----------------------- | --------------------------------- |
| **type**        | **string** |                         | [default to undefined]            |
| **value**       | **number** | Measurement value       | [default to undefined]            |
| **unit**        | **string** | Unit of measurement     | [default to undefined]            |
| **uncertainty** | **number** | Measurement uncertainty | [optional] [default to undefined] |

## Example

```typescript
import { WeatherMeasurement } from '@wrfcoin/sdk-typescript';

const instance: WeatherMeasurement = {
  type,
  value,
  unit,
  uncertainty,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
