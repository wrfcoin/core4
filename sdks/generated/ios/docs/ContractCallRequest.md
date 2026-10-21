# ContractCallRequest

## Properties

| Name                | Type             | Description                  | Notes                       |
| ------------------- | ---------------- | ---------------------------- | --------------------------- |
| **contractAddress** | **String**       | Contract address             |
| **functionName**    | **String**       | Function to call             |
| **parameters**      | **[AnyCodable]** | Function parameters          | [optional]                  |
| **gasLimit**        | **Int**          | Gas limit                    |
| **gasPrice**        | **String**       | Gas price in wei             |
| **value**           | **String**       | ETH value to send (optional) | [optional] [default to "0"] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
