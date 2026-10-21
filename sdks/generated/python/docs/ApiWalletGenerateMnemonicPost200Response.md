# ApiWalletGenerateMnemonicPost200Response

## Properties

| Name         | Type    | Description                 | Notes      |
| ------------ | ------- | --------------------------- | ---------- |
| **mnemonic** | **str** | BIP39 mnemonic phrase       | [optional] |
| **entropy**  | **str** | Entropy used for generation | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_wallet_generate_mnemonic_post200_response import ApiWalletGenerateMnemonicPost200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiWalletGenerateMnemonicPost200Response from a JSON string
api_wallet_generate_mnemonic_post200_response_instance = ApiWalletGenerateMnemonicPost200Response.from_json(json)
# print the JSON string representation of the object
print(ApiWalletGenerateMnemonicPost200Response.to_json())

# convert the object into a dict
api_wallet_generate_mnemonic_post200_response_dict = api_wallet_generate_mnemonic_post200_response_instance.to_dict()
# create an instance of ApiWalletGenerateMnemonicPost200Response from a dict
api_wallet_generate_mnemonic_post200_response_from_dict = ApiWalletGenerateMnemonicPost200Response.from_dict(api_wallet_generate_mnemonic_post200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
