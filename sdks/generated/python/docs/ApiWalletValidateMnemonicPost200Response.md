# ApiWalletValidateMnemonicPost200Response

## Properties

| Name         | Type     | Description | Notes      |
| ------------ | -------- | ----------- | ---------- |
| **valid**    | **bool** |             | [optional] |
| **strength** | **str**  |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_wallet_validate_mnemonic_post200_response import ApiWalletValidateMnemonicPost200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiWalletValidateMnemonicPost200Response from a JSON string
api_wallet_validate_mnemonic_post200_response_instance = ApiWalletValidateMnemonicPost200Response.from_json(json)
# print the JSON string representation of the object
print(ApiWalletValidateMnemonicPost200Response.to_json())

# convert the object into a dict
api_wallet_validate_mnemonic_post200_response_dict = api_wallet_validate_mnemonic_post200_response_instance.to_dict()
# create an instance of ApiWalletValidateMnemonicPost200Response from a dict
api_wallet_validate_mnemonic_post200_response_from_dict = ApiWalletValidateMnemonicPost200Response.from_dict(api_wallet_validate_mnemonic_post200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
