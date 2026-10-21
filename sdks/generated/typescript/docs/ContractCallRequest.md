# ContractCallRequest

## Properties

| Name                | Type                 | Description                  | Notes                             |
| ------------------- | -------------------- | ---------------------------- | --------------------------------- |
| **contractAddress** | **string**           | Contract address             | [default to undefined]            |
| **functionName**    | **string**           | Function to call             | [default to undefined]            |
| **parameters**      | **Array&lt;any&gt;** | Function parameters          | [optional] [default to undefined] |
| **gasLimit**        | **number**           | Gas limit                    | [default to undefined]            |
| **gasPrice**        | **string**           | Gas price in wei             | [default to undefined]            |
| **value**           | **string**           | ETH value to send (optional) | [optional] [default to '0']       |

## Example

```typescript
import { ContractCallRequest } from '@wrfcoin/sdk-typescript';

const instance: ContractCallRequest = {
  contractAddress,
  functionName,
  parameters,
  gasLimit,
  gasPrice,
  value,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
