# HealthStatusSecurity

## Properties

| Name                 | Type                                                                              | Description | Notes                             |
| -------------------- | --------------------------------------------------------------------------------- | ----------- | --------------------------------- |
| **status**           | **string**                                                                        |             | [optional] [default to undefined] |
| **lastSecurityScan** | **string**                                                                        |             | [optional] [default to undefined] |
| **vulnerabilities**  | [**HealthStatusSecurityVulnerabilities**](HealthStatusSecurityVulnerabilities.md) |             | [optional] [default to undefined] |
| **certificates**     | [**HealthStatusSecurityCertificates**](HealthStatusSecurityCertificates.md)       |             | [optional] [default to undefined] |
| **firewall**         | [**HealthStatusSecurityFirewall**](HealthStatusSecurityFirewall.md)               |             | [optional] [default to undefined] |

## Example

```typescript
import { HealthStatusSecurity } from '@wrfcoin/sdk-typescript';

const instance: HealthStatusSecurity = {
  status,
  lastSecurityScan,
  vulnerabilities,
  certificates,
  firewall,
};
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
