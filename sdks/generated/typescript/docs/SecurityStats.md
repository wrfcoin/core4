# SecurityStats

## Properties

| Name                  | Type       | Description | Notes                             |
| --------------------- | ---------- | ----------- | --------------------------------- |
| **totalEvents**       | **number** |             | [optional] [default to undefined] |
| **threatLevel**       | **string** |             | [optional] [default to undefined] |
| **activeThreatCount** | **number** |             | [optional] [default to undefined] |
| **blockedAttacks24h** | **number** |             | [optional] [default to undefined] |
| **securityScore**     | **number** |             | [optional] [default to undefined] |
| **lastUpdated**       | **string** |             | [optional] [default to undefined] |

## Example

```typescript
import { SecurityStats } from '@wrfcoin/sdk-typescript';

const instance: SecurityStats = {
  totalEvents,
  threatLevel,
  activeThreatCount,
  blockedAttacks24h,
  securityScore,
  lastUpdated,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
