# HardwareSignRequest

## Properties

| Name                    | Type        | Description                       | Notes                        |
| ----------------------- | ----------- | --------------------------------- | ---------------------------- |
| **deviceId**            | **string**  | Hardware device ID                | [default to undefined]       |
| **transactionHash**     | **string**  | Transaction hash to sign          | [default to undefined]       |
| **derivationPath**      | **string**  | BIP32 derivation path             | [default to undefined]       |
| **displayOnDevice**     | **boolean** | Show transaction on device screen | [optional] [default to true] |
| **requireConfirmation** | **boolean** | Require user confirmation         | [optional] [default to true] |

## Example

```typescript
import { HardwareSignRequest } from '@wrfcoin/sdk-typescript';

const instance: HardwareSignRequest = {
  deviceId,
  transactionHash,
  derivationPath,
  displayOnDevice,
  requireConfirmation,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
