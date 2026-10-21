# ComplianceStatus

## Properties

| Name                  | Type                                                                                           | Description                    | Notes                             |
| --------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------ | --------------------------------- |
| **overall_score**     | **number**                                                                                     | Overall compliance score       | [optional] [default to undefined] |
| **gdpr_compliance**   | **boolean**                                                                                    | GDPR compliance status         | [optional] [default to undefined] |
| **iso27001_status**   | **string**                                                                                     | ISO 27001 certification status | [optional] [default to undefined] |
| **last_audit**        | **string**                                                                                     | Last audit date                | [optional] [default to undefined] |
| **next_audit**        | **string**                                                                                     | Next audit date                | [optional] [default to undefined] |
| **certifications**    | [**Array&lt;ComplianceStatusCertificationsInner&gt;**](ComplianceStatusCertificationsInner.md) |                                | [optional] [default to undefined] |
| **data_protection**   | [**ComplianceStatusDataProtection**](ComplianceStatusDataProtection.md)                        |                                | [optional] [default to undefined] |
| **policy_compliance** | **number**                                                                                     |                                | [optional] [default to undefined] |
| **violations**        | **number**                                                                                     |                                | [optional] [default to undefined] |
| **remediation_tasks** | **number**                                                                                     |                                | [optional] [default to undefined] |

## Example

```typescript
import { ComplianceStatus } from '@wrfcoin/sdk-typescript';

const instance: ComplianceStatus = {
  overall_score,
  gdpr_compliance,
  iso27001_status,
  last_audit,
  next_audit,
  certifications,
  data_protection,
  policy_compliance,
  violations,
  remediation_tasks,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
