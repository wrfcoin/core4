# HealthStatusBlockchain

## Properties

| Name                | Type         | Description | Notes      |
| ------------------- | ------------ | ----------- | ---------- |
| **status**          | **str**      |             | [optional] |
| **node_connection** | **bool**     |             | [optional] |
| **sync_status**     | **str**      |             | [optional] |
| **blocks_behind**   | **int**      |             | [optional] |
| **last_block_time** | **datetime** |             | [optional] |
| **network_latency** | **float**    |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_blockchain import HealthStatusBlockchain

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusBlockchain from a JSON string
health_status_blockchain_instance = HealthStatusBlockchain.from_json(json)
# print the JSON string representation of the object
print(HealthStatusBlockchain.to_json())

# convert the object into a dict
health_status_blockchain_dict = health_status_blockchain_instance.to_dict()
# create an instance of HealthStatusBlockchain from a dict
health_status_blockchain_from_dict = HealthStatusBlockchain.from_dict(health_status_blockchain_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
