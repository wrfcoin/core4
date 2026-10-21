# ApiComputeProvidersGet200ResponseProvidersInner

## Properties

| Name             | Type                                                                                                                        | Description | Notes                             |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------- | ----------- | --------------------------------- |
| **name**         | **string**                                                                                                                  |             | [optional] [default to undefined] |
| **status**       | **string**                                                                                                                  |             | [optional] [default to undefined] |
| **availability** | **number**                                                                                                                  |             | [optional] [default to undefined] |
| **pricePerHour** | **string**                                                                                                                  |             | [optional] [default to undefined] |
| **capabilities** | **Array&lt;string&gt;**                                                                                                     |             | [optional] [default to undefined] |
| **resources**    | [**ApiComputeProvidersGet200ResponseProvidersInnerResources**](ApiComputeProvidersGet200ResponseProvidersInnerResources.md) |             | [optional] [default to undefined] |

## Example

```typescript
import { ApiComputeProvidersGet200ResponseProvidersInner } from '@wrfcoin/sdk-typescript';

const instance: ApiComputeProvidersGet200ResponseProvidersInner = {
  name,
  status,
  availability,
  pricePerHour,
  capabilities,
  resources,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
