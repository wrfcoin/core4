# HealthStatusSecurityVulnerabilities

## Properties

| Name         | Type    | Description | Notes      |
| ------------ | ------- | ----------- | ---------- |
| **critical** | **int** |             | [optional] |
| **high**     | **int** |             | [optional] |
| **medium**   | **int** |             | [optional] |
| **low**      | **int** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_security_vulnerabilities import HealthStatusSecurityVulnerabilities

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusSecurityVulnerabilities from a JSON string
health_status_security_vulnerabilities_instance = HealthStatusSecurityVulnerabilities.from_json(json)
# print the JSON string representation of the object
print(HealthStatusSecurityVulnerabilities.to_json())

# convert the object into a dict
health_status_security_vulnerabilities_dict = health_status_security_vulnerabilities_instance.to_dict()
# create an instance of HealthStatusSecurityVulnerabilities from a dict
health_status_security_vulnerabilities_from_dict = HealthStatusSecurityVulnerabilities.from_dict(health_status_security_vulnerabilities_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
