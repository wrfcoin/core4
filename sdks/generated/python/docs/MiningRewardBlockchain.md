# MiningRewardBlockchain

## Properties

| Name                 | Type    | Description | Notes      |
| -------------------- | ------- | ----------- | ---------- |
| **block_number**     | **int** |             | [optional] |
| **transaction_hash** | **str** |             | [optional] |
| **gas_used**         | **int** |             | [optional] |
| **confirmations**    | **int** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.mining_reward_blockchain import MiningRewardBlockchain

# TODO update the JSON string below
json = "{}"
# create an instance of MiningRewardBlockchain from a JSON string
mining_reward_blockchain_instance = MiningRewardBlockchain.from_json(json)
# print the JSON string representation of the object
print(MiningRewardBlockchain.to_json())

# convert the object into a dict
mining_reward_blockchain_dict = mining_reward_blockchain_instance.to_dict()
# create an instance of MiningRewardBlockchain from a dict
mining_reward_blockchain_from_dict = MiningRewardBlockchain.from_dict(mining_reward_blockchain_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
