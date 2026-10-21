# HealthStatusSecurity

## Properties

| Name                   | Type                                                                              | Description | Notes      |
| ---------------------- | --------------------------------------------------------------------------------- | ----------- | ---------- |
| **status**             | **str**                                                                           |             | [optional] |
| **last_security_scan** | **datetime**                                                                      |             | [optional] |
| **vulnerabilities**    | [**HealthStatusSecurityVulnerabilities**](HealthStatusSecurityVulnerabilities.md) |             | [optional] |
| **certificates**       | [**HealthStatusSecurityCertificates**](HealthStatusSecurityCertificates.md)       |             | [optional] |
| **firewall**           | [**HealthStatusSecurityFirewall**](HealthStatusSecurityFirewall.md)               |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_security import HealthStatusSecurity

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusSecurity from a JSON string
health_status_security_instance = HealthStatusSecurity.from_json(json)
# print the JSON string representation of the object
print(HealthStatusSecurity.to_json())

# convert the object into a dict
health_status_security_dict = health_status_security_instance.to_dict()
# create an instance of HealthStatusSecurity from a dict
health_status_security_from_dict = HealthStatusSecurity.from_dict(health_status_security_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
