# HardwareDevice

## Properties

| Name                  | Type                    | Description                | Notes                             |
| --------------------- | ----------------------- | -------------------------- | --------------------------------- |
| **deviceId**          | **string**              | Unique device identifier   | [optional] [default to undefined] |
| **deviceType**        | **string**              |                            | [optional] [default to undefined] |
| **deviceName**        | **string**              | Human-readable device name | [optional] [default to undefined] |
| **firmwareVersion**   | **string**              | Device firmware version    | [optional] [default to undefined] |
| **isConnected**       | **boolean**             | Connection status          | [optional] [default to undefined] |
| **isAuthenticated**   | **boolean**             | Authentication status      | [optional] [default to undefined] |
| **supportedFeatures** | **Array&lt;string&gt;** | List of supported features | [optional] [default to undefined] |

## Example

```typescript
import { HardwareDevice } from '@wrfcoin/sdk-typescript';

const instance: HardwareDevice = {
  deviceId,
  deviceType,
  deviceName,
  firmwareVersion,
  isConnected,
  isAuthenticated,
  supportedFeatures,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
