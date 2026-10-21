# HealthStatusSecurityCertificates

## Properties

| Name           | Type         | Description | Notes      |
| -------------- | ------------ | ----------- | ---------- |
| **valid**      | **bool**     |             | [optional] |
| **expires_at** | **datetime** |             | [optional] |
| **grade**      | **str**      |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_security_certificates import HealthStatusSecurityCertificates

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusSecurityCertificates from a JSON string
health_status_security_certificates_instance = HealthStatusSecurityCertificates.from_json(json)
# print the JSON string representation of the object
print(HealthStatusSecurityCertificates.to_json())

# convert the object into a dict
health_status_security_certificates_dict = health_status_security_certificates_instance.to_dict()
# create an instance of HealthStatusSecurityCertificates from a dict
health_status_security_certificates_from_dict = HealthStatusSecurityCertificates.from_dict(health_status_security_certificates_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
