# Location

## Properties

| Name          | Type       | Description                  | Notes                             |
| ------------- | ---------- | ---------------------------- | --------------------------------- |
| **latitude**  | **number** | Latitude in decimal degrees  | [default to undefined]            |
| **longitude** | **number** | Longitude in decimal degrees | [default to undefined]            |
| **altitude**  | **number** | Altitude in meters           | [optional] [default to undefined] |
| **accuracy**  | **number** | Location accuracy in meters  | [optional] [default to undefined] |

## Example

```typescript
import { Location } from '@wrfcoin/sdk-typescript';

const instance: Location = {
  latitude,
  longitude,
  altitude,
  accuracy,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
