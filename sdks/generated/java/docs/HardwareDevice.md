# HardwareDevice

## Properties

| Name                  | Type                                  | Description                | Notes      |
| --------------------- | ------------------------------------- | -------------------------- | ---------- |
| **deviceId**          | **String**                            | Unique device identifier   | [optional] |
| **deviceType**        | [**DeviceTypeEnum**](#DeviceTypeEnum) |                            | [optional] |
| **deviceName**        | **String**                            | Human-readable device name | [optional] |
| **firmwareVersion**   | **String**                            | Device firmware version    | [optional] |
| **isConnected**       | **Boolean**                           | Connection status          | [optional] |
| **isAuthenticated**   | **Boolean**                           | Authentication status      | [optional] |
| **supportedFeatures** | **List&lt;String&gt;**                | List of supported features | [optional] |

## Enum: DeviceTypeEnum

| Name           | Value                      |
| -------------- | -------------------------- |
| LEDGER         | &quot;ledger&quot;         |
| TREZOR         | &quot;trezor&quot;         |
| YUBIHSM        | &quot;yubihsm&quot;        |
| SECURE_ENCLAVE | &quot;secure_enclave&quot; |
| PKCS11         | &quot;pkcs11&quot;         |
