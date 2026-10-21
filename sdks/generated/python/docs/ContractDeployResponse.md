# ContractDeployResponse

## Properties

| Name                 | Type    | Description                 | Notes      |
| -------------------- | ------- | --------------------------- | ---------- |
| **contract_address** | **str** | Deployed contract address   | [optional] |
| **transaction_hash** | **str** | Deployment transaction hash | [optional] |
| **gas_used**         | **int** | Gas used for deployment     | [optional] |

## Example

```python
from wrfcoin_sdk.models.contract_deploy_response import ContractDeployResponse

# TODO update the JSON string below
json = "{}"
# create an instance of ContractDeployResponse from a JSON string
contract_deploy_response_instance = ContractDeployResponse.from_json(json)
# print the JSON string representation of the object
print(ContractDeployResponse.to_json())

# convert the object into a dict
contract_deploy_response_dict = contract_deploy_response_instance.to_dict()
# create an instance of ContractDeployResponse from a dict
contract_deploy_response_from_dict = ContractDeployResponse.from_dict(contract_deploy_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
