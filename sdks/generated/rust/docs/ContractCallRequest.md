# ContractCallRequest

## Properties

| Name                 | Type                                                       | Description                  | Notes                    |
| -------------------- | ---------------------------------------------------------- | ---------------------------- | ------------------------ |
| **contract_address** | **String**                                                 | Contract address             |
| **function_name**    | **String**                                                 | Function to call             |
| **parameters**       | Option<[**Vec<serde_json::Value>**](serde_json::Value.md)> | Function parameters          | [optional]               |
| **gas_limit**        | **i32**                                                    | Gas limit                    |
| **gas_price**        | **String**                                                 | Gas price in wei             |
| **value**            | Option<**String**>                                         | ETH value to send (optional) | [optional][default to 0] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
