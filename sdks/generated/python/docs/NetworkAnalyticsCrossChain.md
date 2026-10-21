# NetworkAnalyticsCrossChain

## Properties

| Name                    | Type      | Description | Notes      |
| ----------------------- | --------- | ----------- | ---------- |
| **supported_chains**    | **int**   |             | [optional] |
| **active_bridges**      | **int**   |             | [optional] |
| **bridge_volume24h**    | **float** |             | [optional] |
| **bridge_success_rate** | **float** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.network_analytics_cross_chain import NetworkAnalyticsCrossChain

# TODO update the JSON string below
json = "{}"
# create an instance of NetworkAnalyticsCrossChain from a JSON string
network_analytics_cross_chain_instance = NetworkAnalyticsCrossChain.from_json(json)
# print the JSON string representation of the object
print(NetworkAnalyticsCrossChain.to_json())

# convert the object into a dict
network_analytics_cross_chain_dict = network_analytics_cross_chain_instance.to_dict()
# create an instance of NetworkAnalyticsCrossChain from a dict
network_analytics_cross_chain_from_dict = NetworkAnalyticsCrossChain.from_dict(network_analytics_cross_chain_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
