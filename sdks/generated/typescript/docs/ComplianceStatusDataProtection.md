# ComplianceStatusDataProtection

## Properties

| Name                      | Type        | Description | Notes                             |
| ------------------------- | ----------- | ----------- | --------------------------------- |
| **encryption_at_rest**    | **boolean** |             | [optional] [default to undefined] |
| **encryption_in_transit** | **boolean** |             | [optional] [default to undefined] |
| **backup_encryption**     | **boolean** |             | [optional] [default to undefined] |
| **access_logging**        | **boolean** |             | [optional] [default to undefined] |
| **data_retention_policy** | **boolean** |             | [optional] [default to undefined] |
| **right_to_deletion**     | **boolean** |             | [optional] [default to undefined] |
| **consent_management**    | **boolean** |             | [optional] [default to undefined] |

## Example

```typescript
import { ComplianceStatusDataProtection } from '@wrfcoin/sdk-typescript';

const instance: ComplianceStatusDataProtection = {
  encryption_at_rest,
  encryption_in_transit,
  backup_encryption,
  access_logging,
  data_retention_policy,
  right_to_deletion,
  consent_management,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
