# ComplianceStatus

## Properties

| Name                 | Type                                                                    | Description                    | Notes      |
| -------------------- | ----------------------------------------------------------------------- | ------------------------------ | ---------- |
| **overallScore**     | **Double**                                                              | Overall compliance score       | [optional] |
| **gdprCompliance**   | **Bool**                                                                | GDPR compliance status         | [optional] |
| **iso27001Status**   | **String**                                                              | ISO 27001 certification status | [optional] |
| **lastAudit**        | **Date**                                                                | Last audit date                | [optional] |
| **nextAudit**        | **Date**                                                                | Next audit date                | [optional] |
| **certifications**   | [ComplianceStatusCertificationsInner]                                   |                                | [optional] |
| **dataProtection**   | [**ComplianceStatusDataProtection**](ComplianceStatusDataProtection.md) |                                | [optional] |
| **policyCompliance** | **Double**                                                              |                                | [optional] |
| **violations**       | **Int**                                                                 |                                | [optional] |
| **remediationTasks** | **Int**                                                                 |                                | [optional] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
