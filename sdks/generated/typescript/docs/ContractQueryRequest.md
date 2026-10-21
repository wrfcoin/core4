# ContractQueryRequest

## Properties

| Name                | Type                 | Description         | Notes                             |
| ------------------- | -------------------- | ------------------- | --------------------------------- |
| **contractAddress** | **string**           | Contract address    | [default to undefined]            |
| **functionName**    | **string**           | Function to query   | [default to undefined]            |
| **parameters**      | **Array&lt;any&gt;** | Function parameters | [optional] [default to undefined] |

## Example

```typescript
import { ContractQueryRequest } from '@wrfcoin/sdk-typescript';

const instance: ContractQueryRequest = {
  contractAddress,
  functionName,
  parameters,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
