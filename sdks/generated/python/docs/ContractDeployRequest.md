# ContractDeployRequest

## Properties

| Name                 | Type          | Description                       | Notes      |
| -------------------- | ------------- | --------------------------------- | ---------- |
| **contract_code**    | **str**       | Base64 encoded WASM contract code |
| **constructor_args** | **List[str]** | Constructor arguments             | [optional] |
| **gas_limit**        | **int**       | Gas limit for deployment          |
| **gas_price**        | **str**       | Gas price in wei                  |

## Example

```python
from wrfcoin_sdk.models.contract_deploy_request import ContractDeployRequest

# TODO update the JSON string below
json = "{}"
# create an instance of ContractDeployRequest from a JSON string
contract_deploy_request_instance = ContractDeployRequest.from_json(json)
# print the JSON string representation of the object
print(ContractDeployRequest.to_json())

# convert the object into a dict
contract_deploy_request_dict = contract_deploy_request_instance.to_dict()
# create an instance of ContractDeployRequest from a dict
contract_deploy_request_from_dict = ContractDeployRequest.from_dict(contract_deploy_request_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
