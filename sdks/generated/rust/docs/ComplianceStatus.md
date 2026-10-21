# ComplianceStatus

## Properties

| Name                  | Type                                                                                                     | Description                    | Notes      |
| --------------------- | -------------------------------------------------------------------------------------------------------- | ------------------------------ | ---------- |
| **overall_score**     | Option<**f64**>                                                                                          | Overall compliance score       | [optional] |
| **gdpr_compliance**   | Option<**bool**>                                                                                         | GDPR compliance status         | [optional] |
| **iso27001_status**   | Option<**String**>                                                                                       | ISO 27001 certification status | [optional] |
| **last_audit**        | Option<[**String**](string.md)>                                                                          | Last audit date                | [optional] |
| **next_audit**        | Option<[**String**](string.md)>                                                                          | Next audit date                | [optional] |
| **certifications**    | Option<[**Vec<models::ComplianceStatusCertificationsInner>**](ComplianceStatus_certifications_inner.md)> |                                | [optional] |
| **data_protection**   | Option<[**models::ComplianceStatusDataProtection**](ComplianceStatus_data_protection.md)>                |                                | [optional] |
| **policy_compliance** | Option<**f64**>                                                                                          |                                | [optional] |
| **violations**        | Option<**i32**>                                                                                          |                                | [optional] |
| **remediation_tasks** | Option<**i32**>                                                                                          |                                | [optional] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
