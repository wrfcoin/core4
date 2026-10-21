# LangChainInsight

## Properties

| Name               | Type    | Description | Notes      |
| ------------------ | ------- | ----------- | ---------- |
| **type**           | **str** |             | [optional] |
| **value**          | **str** |             | [optional] |
| **trend**          | **str** |             | [optional] |
| **recommendation** | **str** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.lang_chain_insight import LangChainInsight

# TODO update the JSON string below
json = "{}"
# create an instance of LangChainInsight from a JSON string
lang_chain_insight_instance = LangChainInsight.from_json(json)
# print the JSON string representation of the object
print(LangChainInsight.to_json())

# convert the object into a dict
lang_chain_insight_dict = lang_chain_insight_instance.to_dict()
# create an instance of LangChainInsight from a dict
lang_chain_insight_from_dict = LangChainInsight.from_dict(lang_chain_insight_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
