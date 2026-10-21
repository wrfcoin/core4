# MiningRewardMultipliers

## Properties

| Name          | Type      | Description | Notes      |
| ------------- | --------- | ----------- | ---------- |
| **base**      | **float** |             | [optional] |
| **quality**   | **float** |             | [optional] |
| **proximity** | **float** |             | [optional] |
| **frequency** | **float** |             | [optional] |
| **total**     | **float** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.mining_reward_multipliers import MiningRewardMultipliers

# TODO update the JSON string below
json = "{}"
# create an instance of MiningRewardMultipliers from a JSON string
mining_reward_multipliers_instance = MiningRewardMultipliers.from_json(json)
# print the JSON string representation of the object
print(MiningRewardMultipliers.to_json())

# convert the object into a dict
mining_reward_multipliers_dict = mining_reward_multipliers_instance.to_dict()
# create an instance of MiningRewardMultipliers from a dict
mining_reward_multipliers_from_dict = MiningRewardMultipliers.from_dict(mining_reward_multipliers_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
