# ContractQueryRequest

## Properties

| Name                 | Type             | Description         | Notes      |
| -------------------- | ---------------- | ------------------- | ---------- |
| **contract_address** | **str**          | Contract address    |
| **function_name**    | **str**          | Function to query   |
| **parameters**       | **List[object]** | Function parameters | [optional] |

## Example

```python
from wrfcoin_sdk.models.contract_query_request import ContractQueryRequest

# TODO update the JSON string below
json = "{}"
# create an instance of ContractQueryRequest from a JSON string
contract_query_request_instance = ContractQueryRequest.from_json(json)
# print the JSON string representation of the object
print(ContractQueryRequest.to_json())

# convert the object into a dict
contract_query_request_dict = contract_query_request_instance.to_dict()
# create an instance of ContractQueryRequest from a dict
contract_query_request_from_dict = ContractQueryRequest.from_dict(contract_query_request_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
