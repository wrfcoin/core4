# ApiWalletValidateMnemonicPostRequest

## Properties

| Name         | Type    | Description                       | Notes |
| ------------ | ------- | --------------------------------- | ----- |
| **mnemonic** | **str** | BIP39 mnemonic phrase to validate |

## Example

```python
from wrfcoin_sdk.models.api_wallet_validate_mnemonic_post_request import ApiWalletValidateMnemonicPostRequest

# TODO update the JSON string below
json = "{}"
# create an instance of ApiWalletValidateMnemonicPostRequest from a JSON string
api_wallet_validate_mnemonic_post_request_instance = ApiWalletValidateMnemonicPostRequest.from_json(json)
# print the JSON string representation of the object
print(ApiWalletValidateMnemonicPostRequest.to_json())

# convert the object into a dict
api_wallet_validate_mnemonic_post_request_dict = api_wallet_validate_mnemonic_post_request_instance.to_dict()
# create an instance of ApiWalletValidateMnemonicPostRequest from a dict
api_wallet_validate_mnemonic_post_request_from_dict = ApiWalletValidateMnemonicPostRequest.from_dict(api_wallet_validate_mnemonic_post_request_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
