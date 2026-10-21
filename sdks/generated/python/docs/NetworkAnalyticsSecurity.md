# NetworkAnalyticsSecurity

## Properties

| Name                | Type      | Description | Notes      |
| ------------------- | --------- | ----------- | ---------- |
| **threat_level**    | **str**   |             | [optional] |
| **active_threats**  | **int**   |             | [optional] |
| **blocked_attacks** | **int**   |             | [optional] |
| **security_score**  | **float** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.network_analytics_security import NetworkAnalyticsSecurity

# TODO update the JSON string below
json = "{}"
# create an instance of NetworkAnalyticsSecurity from a JSON string
network_analytics_security_instance = NetworkAnalyticsSecurity.from_json(json)
# print the JSON string representation of the object
print(NetworkAnalyticsSecurity.to_json())

# convert the object into a dict
network_analytics_security_dict = network_analytics_security_instance.to_dict()
# create an instance of NetworkAnalyticsSecurity from a dict
network_analytics_security_from_dict = NetworkAnalyticsSecurity.from_dict(network_analytics_security_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
