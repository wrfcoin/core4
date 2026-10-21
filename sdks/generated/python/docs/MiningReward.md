# MiningReward

## Properties

| Name            | Type                                                      | Description              | Notes      |
| --------------- | --------------------------------------------------------- | ------------------------ | ---------- |
| **id**          | **str**                                                   | Unique reward identifier | [optional] |
| **recipient**   | **str**                                                   | Recipient wallet address | [optional] |
| **type**        | **str**                                                   | Type of mining reward    | [optional] |
| **amount**      | **float**                                                 | Reward amount            | [optional] |
| **currency**    | **str**                                                   | Currency symbol          | [optional] |
| **source**      | [**MiningRewardSource**](MiningRewardSource.md)           |                          | [optional] |
| **multipliers** | [**MiningRewardMultipliers**](MiningRewardMultipliers.md) |                          | [optional] |
| **performance** | [**MiningRewardPerformance**](MiningRewardPerformance.md) |                          | [optional] |
| **blockchain**  | [**MiningRewardBlockchain**](MiningRewardBlockchain.md)   |                          | [optional] |
| **timestamp**   | **datetime**                                              |                          | [optional] |
| **status**      | **str**                                                   |                          | [optional] |

## Example

```python
from wrfcoin_sdk.models.mining_reward import MiningReward

# TODO update the JSON string below
json = "{}"
# create an instance of MiningReward from a JSON string
mining_reward_instance = MiningReward.from_json(json)
# print the JSON string representation of the object
print(MiningReward.to_json())

# convert the object into a dict
mining_reward_dict = mining_reward_instance.to_dict()
# create an instance of MiningReward from a dict
mining_reward_from_dict = MiningReward.from_dict(mining_reward_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
