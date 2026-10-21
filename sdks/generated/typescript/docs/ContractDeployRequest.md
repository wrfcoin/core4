# ContractDeployRequest

## Properties

| Name                | Type                    | Description                       | Notes                             |
| ------------------- | ----------------------- | --------------------------------- | --------------------------------- |
| **contractCode**    | **string**              | Base64 encoded WASM contract code | [default to undefined]            |
| **constructorArgs** | **Array&lt;string&gt;** | Constructor arguments             | [optional] [default to undefined] |
| **gasLimit**        | **number**              | Gas limit for deployment          | [default to undefined]            |
| **gasPrice**        | **string**              | Gas price in wei                  | [default to undefined]            |

## Example

```typescript
import { ContractDeployRequest } from '@wrfcoin/sdk-typescript';

const instance: ContractDeployRequest = {
  contractCode,
  constructorArgs,
  gasLimit,
  gasPrice,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
