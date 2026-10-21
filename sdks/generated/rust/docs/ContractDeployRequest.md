# ContractDeployRequest

## Properties

| Name                 | Type                    | Description                       | Notes      |
| -------------------- | ----------------------- | --------------------------------- | ---------- |
| **contract_code**    | **String**              | Base64 encoded WASM contract code |
| **constructor_args** | Option<**Vec<String>**> | Constructor arguments             | [optional] |
| **gas_limit**        | **i32**                 | Gas limit for deployment          |
| **gas_price**        | **String**              | Gas price in wei                  |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
