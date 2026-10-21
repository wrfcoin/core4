# ContractCallResponse

## Properties

| Name                | Type                    | Description        | Notes                             |
| ------------------- | ----------------------- | ------------------ | --------------------------------- |
| **transactionHash** | **string**              | Transaction hash   | [optional] [default to undefined] |
| **gasUsed**         | **number**              | Gas used           | [optional] [default to undefined] |
| **logs**            | **Array&lt;object&gt;** | Event logs emitted | [optional] [default to undefined] |

## Example

```typescript
import { ContractCallResponse } from '@wrfcoin/sdk-typescript';

const instance: ContractCallResponse = {
  transactionHash,
  gasUsed,
  logs,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
