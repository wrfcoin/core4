# MiningRewardSource

## Properties

| Name        | Type       | Description | Notes      |
| ----------- | ---------- | ----------- | ---------- |
| **type**    | **str**    |             | [optional] |
| **id**      | **str**    |             | [optional] |
| **details** | **object** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.mining_reward_source import MiningRewardSource

# TODO update the JSON string below
json = "{}"
# create an instance of MiningRewardSource from a JSON string
mining_reward_source_instance = MiningRewardSource.from_json(json)
# print the JSON string representation of the object
print(MiningRewardSource.to_json())

# convert the object into a dict
mining_reward_source_dict = mining_reward_source_instance.to_dict()
# create an instance of MiningRewardSource from a dict
mining_reward_source_from_dict = MiningRewardSource.from_dict(mining_reward_source_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
