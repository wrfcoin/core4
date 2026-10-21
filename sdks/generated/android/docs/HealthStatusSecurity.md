# HealthStatusSecurity

## Properties

| Name                 | Type                                                                              | Description | Notes      |
| -------------------- | --------------------------------------------------------------------------------- | ----------- | ---------- |
| **status**           | **String**                                                                        |             | [optional] |
| **lastSecurityScan** | **OffsetDateTime**                                                                |             | [optional] |
| **vulnerabilities**  | [**HealthStatusSecurityVulnerabilities**](HealthStatusSecurityVulnerabilities.md) |             | [optional] |
| **certificates**     | [**HealthStatusSecurityCertificates**](HealthStatusSecurityCertificates.md)       |             | [optional] |
| **firewall**         | [**HealthStatusSecurityFirewall**](HealthStatusSecurityFirewall.md)               |             | [optional] |
