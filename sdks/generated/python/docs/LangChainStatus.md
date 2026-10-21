# LangChainStatus

## Properties

| Name             | Type                                                        | Description | Notes      |
| ---------------- | ----------------------------------------------------------- | ----------- | ---------- |
| **enabled**      | **bool**                                                    |             | [optional] |
| **available**    | **bool**                                                    |             | [optional] |
| **analytics**    | [**LangChainStatusAnalytics**](LangChainStatusAnalytics.md) |             | [optional] |
| **description**  | **str**                                                     |             | [optional] |
| **independence** | **str**                                                     |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.lang_chain_status import LangChainStatus

# TODO update the JSON string below
json = "{}"
# create an instance of LangChainStatus from a JSON string
lang_chain_status_instance = LangChainStatus.from_json(json)
# print the JSON string representation of the object
print(LangChainStatus.to_json())

# convert the object into a dict
lang_chain_status_dict = lang_chain_status_instance.to_dict()
# create an instance of LangChainStatus from a dict
lang_chain_status_from_dict = LangChainStatus.from_dict(lang_chain_status_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
