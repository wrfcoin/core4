# MiningReward

## Properties

| Name            | Type                                                      | Description              | Notes                             |
| --------------- | --------------------------------------------------------- | ------------------------ | --------------------------------- |
| **id**          | **string**                                                | Unique reward identifier | [optional] [default to undefined] |
| **recipient**   | **string**                                                | Recipient wallet address | [optional] [default to undefined] |
| **type**        | **string**                                                | Type of mining reward    | [optional] [default to undefined] |
| **amount**      | **number**                                                | Reward amount            | [optional] [default to undefined] |
| **currency**    | **string**                                                | Currency symbol          | [optional] [default to undefined] |
| **source**      | [**MiningRewardSource**](MiningRewardSource.md)           |                          | [optional] [default to undefined] |
| **multipliers** | [**MiningRewardMultipliers**](MiningRewardMultipliers.md) |                          | [optional] [default to undefined] |
| **performance** | [**MiningRewardPerformance**](MiningRewardPerformance.md) |                          | [optional] [default to undefined] |
| **blockchain**  | [**MiningRewardBlockchain**](MiningRewardBlockchain.md)   |                          | [optional] [default to undefined] |
| **timestamp**   | **string**                                                |                          | [optional] [default to undefined] |
| **status**      | **string**                                                |                          | [optional] [default to undefined] |

## Example

```typescript
import { MiningReward } from '@wrfcoin/sdk-typescript';

const instance: MiningReward = {
  id,
  recipient,
  type,
  amount,
  currency,
  source,
  multipliers,
  performance,
  blockchain,
  timestamp,
  status,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
