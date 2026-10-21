# ApiContractsEstimateGasPost200Response

## Properties

| Name             | Type    | Description                  | Notes      |
| ---------------- | ------- | ---------------------------- | ---------- |
| **gas_estimate** | **int** | Estimated gas units required | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_contracts_estimate_gas_post200_response import ApiContractsEstimateGasPost200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiContractsEstimateGasPost200Response from a JSON string
api_contracts_estimate_gas_post200_response_instance = ApiContractsEstimateGasPost200Response.from_json(json)
# print the JSON string representation of the object
print(ApiContractsEstimateGasPost200Response.to_json())

# convert the object into a dict
api_contracts_estimate_gas_post200_response_dict = api_contracts_estimate_gas_post200_response_instance.to_dict()
# create an instance of ApiContractsEstimateGasPost200Response from a dict
api_contracts_estimate_gas_post200_response_from_dict = ApiContractsEstimateGasPost200Response.from_dict(api_contracts_estimate_gas_post200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
