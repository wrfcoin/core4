# BlockchainStatus

## Properties

| Name                  | Type      | Description                   | Notes      |
| --------------------- | --------- | ----------------------------- | ---------- |
| **block_height**      | **int**   | Current block height          | [optional] |
| **last_block_hash**   | **str**   | Hash of the most recent block | [optional] |
| **network_hash_rate** | **float** | Network hash rate in H/s      | [optional] |
| **difficulty**        | **float** | Current mining difficulty     | [optional] |
| **consensus_state**   | **str**   |                               | [optional] |
| **peer_count**        | **int**   | Number of connected peers     | [optional] |
| **sync_status**       | **str**   |                               | [optional] |
| **uptime_seconds**    | **int**   | Node uptime in seconds        | [optional] |

## Example

```python
from wrfcoin_sdk.models.blockchain_status import BlockchainStatus

# TODO update the JSON string below
json = "{}"
# create an instance of BlockchainStatus from a JSON string
blockchain_status_instance = BlockchainStatus.from_json(json)
# print the JSON string representation of the object
print(BlockchainStatus.to_json())

# convert the object into a dict
blockchain_status_dict = blockchain_status_instance.to_dict()
# create an instance of BlockchainStatus from a dict
blockchain_status_from_dict = BlockchainStatus.from_dict(blockchain_status_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
