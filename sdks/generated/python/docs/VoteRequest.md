# VoteRequest

## Properties

| Name             | Type    | Description            | Notes      |
| ---------------- | ------- | ---------------------- | ---------- |
| **vote**         | **str** |                        |
| **voting_power** | **str** | Voting power to use    | [optional] |
| **reason**       | **str** | Optional voting reason | [optional] |

## Example

```python
from wrfcoin_sdk.models.vote_request import VoteRequest

# TODO update the JSON string below
json = "{}"
# create an instance of VoteRequest from a JSON string
vote_request_instance = VoteRequest.from_json(json)
# print the JSON string representation of the object
print(VoteRequest.to_json())

# convert the object into a dict
vote_request_dict = vote_request_instance.to_dict()
# create an instance of VoteRequest from a dict
vote_request_from_dict = VoteRequest.from_dict(vote_request_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
