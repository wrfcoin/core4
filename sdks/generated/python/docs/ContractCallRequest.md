# ContractCallRequest

## Properties

| Name                 | Type             | Description                  | Notes                       |
| -------------------- | ---------------- | ---------------------------- | --------------------------- |
| **contract_address** | **str**          | Contract address             |
| **function_name**    | **str**          | Function to call             |
| **parameters**       | **List[object]** | Function parameters          | [optional]                  |
| **gas_limit**        | **int**          | Gas limit                    |
| **gas_price**        | **str**          | Gas price in wei             |
| **value**            | **str**          | ETH value to send (optional) | [optional] [default to '0'] |

## Example

```python
from wrfcoin_sdk.models.contract_call_request import ContractCallRequest

# TODO update the JSON string below
json = "{}"
# create an instance of ContractCallRequest from a JSON string
contract_call_request_instance = ContractCallRequest.from_json(json)
# print the JSON string representation of the object
print(ContractCallRequest.to_json())

# convert the object into a dict
contract_call_request_dict = contract_call_request_instance.to_dict()
# create an instance of ContractCallRequest from a dict
contract_call_request_from_dict = ContractCallRequest.from_dict(contract_call_request_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
