# Block

## Properties

| Name                  | Type      | Description                     | Notes      |
| --------------------- | --------- | ------------------------------- | ---------- |
| **hash**              | **str**   | Block hash                      | [optional] |
| **index**             | **int**   | Block number                    | [optional] |
| **previous_hash**     | **str**   | Previous block hash             | [optional] |
| **timestamp**         | **int**   | Block timestamp                 | [optional] |
| **merkle_root**       | **str**   | Merkle root of transactions     | [optional] |
| **nonce**             | **int**   | Mining nonce                    | [optional] |
| **difficulty**        | **float** | Block difficulty                | [optional] |
| **transaction_count** | **int**   | Number of transactions in block | [optional] |
| **size**              | **int**   | Block size in bytes             | [optional] |
| **gas_used**          | **int**   | Total gas used in block         | [optional] |
| **gas_limit**         | **int**   | Block gas limit                 | [optional] |

## Example

```python
from wrfcoin_sdk.models.block import Block

# TODO update the JSON string below
json = "{}"
# create an instance of Block from a JSON string
block_instance = Block.from_json(json)
# print the JSON string representation of the object
print(Block.to_json())

# convert the object into a dict
block_dict = block_instance.to_dict()
# create an instance of Block from a dict
block_from_dict = Block.from_dict(block_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
