# HardwareSignResponse

## Properties

| Name                | Type        | Description                      | Notes                             |
| ------------------- | ----------- | -------------------------------- | --------------------------------- |
| **signature**       | **string**  | Transaction signature            | [optional] [default to undefined] |
| **publicKey**       | **string**  | Public key used for signing      | [optional] [default to undefined] |
| **deviceConfirmed** | **boolean** | Whether user confirmed on device | [optional] [default to undefined] |
| **timestamp**       | **number**  | Signing timestamp                | [optional] [default to undefined] |

## Example

```typescript
import { HardwareSignResponse } from '@wrfcoin/sdk-typescript';

const instance: HardwareSignResponse = {
  signature,
  publicKey,
  deviceConfirmed,
  timestamp,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
