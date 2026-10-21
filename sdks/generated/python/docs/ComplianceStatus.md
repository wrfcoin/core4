# ComplianceStatus

## Properties

| Name                  | Type                                                                                    | Description                    | Notes      |
| --------------------- | --------------------------------------------------------------------------------------- | ------------------------------ | ---------- |
| **overall_score**     | **float**                                                                               | Overall compliance score       | [optional] |
| **gdpr_compliance**   | **bool**                                                                                | GDPR compliance status         | [optional] |
| **iso27001_status**   | **str**                                                                                 | ISO 27001 certification status | [optional] |
| **last_audit**        | **date**                                                                                | Last audit date                | [optional] |
| **next_audit**        | **date**                                                                                | Next audit date                | [optional] |
| **certifications**    | [**List[ComplianceStatusCertificationsInner]**](ComplianceStatusCertificationsInner.md) |                                | [optional] |
| **data_protection**   | [**ComplianceStatusDataProtection**](ComplianceStatusDataProtection.md)                 |                                | [optional] |
| **policy_compliance** | **float**                                                                               |                                | [optional] |
| **violations**        | **int**                                                                                 |                                | [optional] |
| **remediation_tasks** | **int**                                                                                 |                                | [optional] |

## Example

```python
from wrfcoin_sdk.models.compliance_status import ComplianceStatus

# TODO update the JSON string below
json = "{}"
# create an instance of ComplianceStatus from a JSON string
compliance_status_instance = ComplianceStatus.from_json(json)
# print the JSON string representation of the object
print(ComplianceStatus.to_json())

# convert the object into a dict
compliance_status_dict = compliance_status_instance.to_dict()
# create an instance of ComplianceStatus from a dict
compliance_status_from_dict = ComplianceStatus.from_dict(compliance_status_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
