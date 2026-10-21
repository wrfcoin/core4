# HealthStatusSecurityFirewall

## Properties

| Name                       | Type     | Description | Notes      |
| -------------------------- | -------- | ----------- | ---------- |
| **active**                 | **bool** |             | [optional] |
| **rules**                  | **int**  |             | [optional] |
| **blocked_connections24h** | **int**  |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.health_status_security_firewall import HealthStatusSecurityFirewall

# TODO update the JSON string below
json = "{}"
# create an instance of HealthStatusSecurityFirewall from a JSON string
health_status_security_firewall_instance = HealthStatusSecurityFirewall.from_json(json)
# print the JSON string representation of the object
print(HealthStatusSecurityFirewall.to_json())

# convert the object into a dict
health_status_security_firewall_dict = health_status_security_firewall_instance.to_dict()
# create an instance of HealthStatusSecurityFirewall from a dict
health_status_security_firewall_from_dict = HealthStatusSecurityFirewall.from_dict(health_status_security_firewall_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
