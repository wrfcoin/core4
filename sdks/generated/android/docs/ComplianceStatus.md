# ComplianceStatus

## Properties

| Name                 | Type                                                                                          | Description                    | Notes      |
| -------------------- | --------------------------------------------------------------------------------------------- | ------------------------------ | ---------- |
| **overallScore**     | **BigDecimal**                                                                                | Overall compliance score       | [optional] |
| **gdprCompliance**   | **Boolean**                                                                                   | GDPR compliance status         | [optional] |
| **iso27001Status**   | **String**                                                                                    | ISO 27001 certification status | [optional] |
| **lastAudit**        | **LocalDate**                                                                                 | Last audit date                | [optional] |
| **nextAudit**        | **LocalDate**                                                                                 | Next audit date                | [optional] |
| **certifications**   | [**List&lt;ComplianceStatusCertificationsInner&gt;**](ComplianceStatusCertificationsInner.md) |                                | [optional] |
| **dataProtection**   | [**ComplianceStatusDataProtection**](ComplianceStatusDataProtection.md)                       |                                | [optional] |
| **policyCompliance** | **BigDecimal**                                                                                |                                | [optional] |
| **violations**       | **Integer**                                                                                   |                                | [optional] |
| **remediationTasks** | **Integer**                                                                                   |                                | [optional] |
