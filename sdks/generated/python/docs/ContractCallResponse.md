# ContractCallResponse

## Properties

| Name                 | Type             | Description        | Notes      |
| -------------------- | ---------------- | ------------------ | ---------- |
| **transaction_hash** | **str**          | Transaction hash   | [optional] |
| **gas_used**         | **int**          | Gas used           | [optional] |
| **logs**             | **List[object]** | Event logs emitted | [optional] |

## Example

```python
from wrfcoin_sdk.models.contract_call_response import ContractCallResponse

# TODO update the JSON string below
json = "{}"
# create an instance of ContractCallResponse from a JSON string
contract_call_response_instance = ContractCallResponse.from_json(json)
# print the JSON string representation of the object
print(ContractCallResponse.to_json())

# convert the object into a dict
contract_call_response_dict = contract_call_response_instance.to_dict()
# create an instance of ContractCallResponse from a dict
contract_call_response_from_dict = ContractCallResponse.from_dict(contract_call_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
