# Transaction

## Properties

| Name                  | Type    | Description                       | Notes      |
| --------------------- | ------- | --------------------------------- | ---------- |
| **hash**              | **str** | Transaction hash                  | [optional] |
| **var_from**          | **str** | Sender address                    | [optional] |
| **to**                | **str** | Recipient address                 | [optional] |
| **value**             | **str** | Transaction value in wei          | [optional] |
| **gas**               | **int** | Gas limit                         | [optional] |
| **gas_price**         | **str** | Gas price in wei                  | [optional] |
| **gas_used**          | **int** | Gas used                          | [optional] |
| **nonce**             | **int** | Transaction nonce                 | [optional] |
| **block_hash**        | **str** | Block hash containing transaction | [optional] |
| **block_number**      | **int** | Block number                      | [optional] |
| **transaction_index** | **int** | Transaction index in block        | [optional] |
| **status**            | **str** |                                   | [optional] |
| **timestamp**         | **int** | Transaction timestamp             | [optional] |

## Example

```python
from wrfcoin_sdk.models.transaction import Transaction

# TODO update the JSON string below
json = "{}"
# create an instance of Transaction from a JSON string
transaction_instance = Transaction.from_json(json)
# print the JSON string representation of the object
print(Transaction.to_json())

# convert the object into a dict
transaction_dict = transaction_instance.to_dict()
# create an instance of Transaction from a dict
transaction_from_dict = Transaction.from_dict(transaction_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
