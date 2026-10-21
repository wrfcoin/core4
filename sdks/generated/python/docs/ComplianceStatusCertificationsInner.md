# ComplianceStatusCertificationsInner

## Properties

| Name            | Type     | Description | Notes      |
| --------------- | -------- | ----------- | ---------- |
| **name**        | **str**  |             | [optional] |
| **status**      | **str**  |             | [optional] |
| **expires**     | **date** |             | [optional] |
| **last_review** | **date** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.compliance_status_certifications_inner import ComplianceStatusCertificationsInner

# TODO update the JSON string below
json = "{}"
# create an instance of ComplianceStatusCertificationsInner from a JSON string
compliance_status_certifications_inner_instance = ComplianceStatusCertificationsInner.from_json(json)
# print the JSON string representation of the object
print(ComplianceStatusCertificationsInner.to_json())

# convert the object into a dict
compliance_status_certifications_inner_dict = compliance_status_certifications_inner_instance.to_dict()
# create an instance of ComplianceStatusCertificationsInner from a dict
compliance_status_certifications_inner_from_dict = ComplianceStatusCertificationsInner.from_dict(compliance_status_certifications_inner_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
