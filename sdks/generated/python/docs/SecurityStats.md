# SecurityStats

## Properties

| Name                    | Type         | Description | Notes      |
| ----------------------- | ------------ | ----------- | ---------- |
| **total_events**        | **int**      |             | [optional] |
| **threat_level**        | **str**      |             | [optional] |
| **active_threat_count** | **int**      |             | [optional] |
| **blocked_attacks24h**  | **int**      |             | [optional] |
| **security_score**      | **float**    |             | [optional] |
| **last_updated**        | **datetime** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.security_stats import SecurityStats

# TODO update the JSON string below
json = "{}"
# create an instance of SecurityStats from a JSON string
security_stats_instance = SecurityStats.from_json(json)
# print the JSON string representation of the object
print(SecurityStats.to_json())

# convert the object into a dict
security_stats_dict = security_stats_instance.to_dict()
# create an instance of SecurityStats from a dict
security_stats_from_dict = SecurityStats.from_dict(security_stats_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
