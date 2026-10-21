# ContractDeployResponse

## Properties

| Name                | Type       | Description                 | Notes                             |
| ------------------- | ---------- | --------------------------- | --------------------------------- |
| **contractAddress** | **string** | Deployed contract address   | [optional] [default to undefined] |
| **transactionHash** | **string** | Deployment transaction hash | [optional] [default to undefined] |
| **gasUsed**         | **number** | Gas used for deployment     | [optional] [default to undefined] |

## Example

```typescript
import { ContractDeployResponse } from '@wrfcoin/sdk-typescript';

const instance: ContractDeployResponse = {
  contractAddress,
  transactionHash,
  gasUsed,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
