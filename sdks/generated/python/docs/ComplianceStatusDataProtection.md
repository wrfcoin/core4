# ComplianceStatusDataProtection

## Properties

| Name                      | Type     | Description | Notes      |
| ------------------------- | -------- | ----------- | ---------- |
| **encryption_at_rest**    | **bool** |             | [optional] |
| **encryption_in_transit** | **bool** |             | [optional] |
| **backup_encryption**     | **bool** |             | [optional] |
| **access_logging**        | **bool** |             | [optional] |
| **data_retention_policy** | **bool** |             | [optional] |
| **right_to_deletion**     | **bool** |             | [optional] |
| **consent_management**    | **bool** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.compliance_status_data_protection import ComplianceStatusDataProtection

# TODO update the JSON string below
json = "{}"
# create an instance of ComplianceStatusDataProtection from a JSON string
compliance_status_data_protection_instance = ComplianceStatusDataProtection.from_json(json)
# print the JSON string representation of the object
print(ComplianceStatusDataProtection.to_json())

# convert the object into a dict
compliance_status_data_protection_dict = compliance_status_data_protection_instance.to_dict()
# create an instance of ComplianceStatusDataProtection from a dict
compliance_status_data_protection_from_dict = ComplianceStatusDataProtection.from_dict(compliance_status_data_protection_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
