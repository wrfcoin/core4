# ApiBlockchainBlocksGet200Response

## Properties

| Name           | Type                            | Description | Notes      |
| -------------- | ------------------------------- | ----------- | ---------- |
| **blocks**     | [**List[Block]**](Block.md)     |             | [optional] |
| **total**      | **int**                         |             | [optional] |
| **pagination** | [**Pagination**](Pagination.md) |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_blockchain_blocks_get200_response import ApiBlockchainBlocksGet200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiBlockchainBlocksGet200Response from a JSON string
api_blockchain_blocks_get200_response_instance = ApiBlockchainBlocksGet200Response.from_json(json)
# print the JSON string representation of the object
print(ApiBlockchainBlocksGet200Response.to_json())

# convert the object into a dict
api_blockchain_blocks_get200_response_dict = api_blockchain_blocks_get200_response_instance.to_dict()
# create an instance of ApiBlockchainBlocksGet200Response from a dict
api_blockchain_blocks_get200_response_from_dict = ApiBlockchainBlocksGet200Response.from_dict(api_blockchain_blocks_get200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
