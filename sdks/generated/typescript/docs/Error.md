# ModelError

## Properties

| Name          | Type       | Description              | Notes                             |
| ------------- | ---------- | ------------------------ | --------------------------------- |
| **error**     | **string** | Error message            | [default to undefined]            |
| **code**      | **string** | Error code               | [default to undefined]            |
| **details**   | **object** | Additional error details | [optional] [default to undefined] |
| **timestamp** | **string** |                          | [optional] [default to undefined] |

## Example

```typescript
import { ModelError } from '@wrfcoin/sdk-typescript';

const instance: ModelError = {
  error,
  code,
  details,
  timestamp,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
