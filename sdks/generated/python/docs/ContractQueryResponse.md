# ContractQueryResponse

## Properties

| Name             | Type       | Description                 | Notes      |
| ---------------- | ---------- | --------------------------- | ---------- |
| **result**       | **object** | Function return value       | [optional] |
| **gas_estimate** | **int**    | Estimated gas for this call | [optional] |

## Example

```python
from wrfcoin_sdk.models.contract_query_response import ContractQueryResponse

# TODO update the JSON string below
json = "{}"
# create an instance of ContractQueryResponse from a JSON string
contract_query_response_instance = ContractQueryResponse.from_json(json)
# print the JSON string representation of the object
print(ContractQueryResponse.to_json())

# convert the object into a dict
contract_query_response_dict = contract_query_response_instance.to_dict()
# create an instance of ContractQueryResponse from a dict
contract_query_response_from_dict = ContractQueryResponse.from_dict(contract_query_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
