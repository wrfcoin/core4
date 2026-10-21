# MiningRewardPerformance

## Properties

| Name         | Type      | Description | Notes      |
| ------------ | --------- | ----------- | ---------- |
| **uptime**   | **float** |             | [optional] |
| **accuracy** | **float** |             | [optional] |
| **speed**    | **float** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.mining_reward_performance import MiningRewardPerformance

# TODO update the JSON string below
json = "{}"
# create an instance of MiningRewardPerformance from a JSON string
mining_reward_performance_instance = MiningRewardPerformance.from_json(json)
# print the JSON string representation of the object
print(MiningRewardPerformance.to_json())

# convert the object into a dict
mining_reward_performance_dict = mining_reward_performance_instance.to_dict()
# create an instance of MiningRewardPerformance from a dict
mining_reward_performance_from_dict = MiningRewardPerformance.from_dict(mining_reward_performance_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
