# HealthStatusBlockchain

## Properties

| Name               | Type        | Description | Notes                             |
| ------------------ | ----------- | ----------- | --------------------------------- |
| **status**         | **string**  |             | [optional] [default to undefined] |
| **nodeConnection** | **boolean** |             | [optional] [default to undefined] |
| **syncStatus**     | **string**  |             | [optional] [default to undefined] |
| **blocksBehind**   | **number**  |             | [optional] [default to undefined] |
| **lastBlockTime**  | **string**  |             | [optional] [default to undefined] |
| **networkLatency** | **number**  |             | [optional] [default to undefined] |

## Example

```typescript
import { HealthStatusBlockchain } from '@wrfcoin/sdk-typescript';

const instance: HealthStatusBlockchain = {
  status,
  nodeConnection,
  syncStatus,
  blocksBehind,
  lastBlockTime,
  networkLatency,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
