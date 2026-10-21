# LangChainStatusAnalytics

## Properties

| Name                | Type    | Description | Notes      |
| ------------------- | ------- | ----------- | ---------- |
| **network_health**  | **str** |             | [optional] |
| **recommendations** | **int** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.lang_chain_status_analytics import LangChainStatusAnalytics

# TODO update the JSON string below
json = "{}"
# create an instance of LangChainStatusAnalytics from a JSON string
lang_chain_status_analytics_instance = LangChainStatusAnalytics.from_json(json)
# print the JSON string representation of the object
print(LangChainStatusAnalytics.to_json())

# convert the object into a dict
lang_chain_status_analytics_dict = lang_chain_status_analytics_instance.to_dict()
# create an instance of LangChainStatusAnalytics from a dict
lang_chain_status_analytics_from_dict = LangChainStatusAnalytics.from_dict(lang_chain_status_analytics_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
