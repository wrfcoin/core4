# Proposal

## Properties

| Name               | Type    | Description                         | Notes      |
| ------------------ | ------- | ----------------------------------- | ---------- |
| **id**             | **str** | Proposal ID                         | [optional] |
| **title**          | **str** | Proposal title                      | [optional] |
| **description**    | **str** | Proposal description                | [optional] |
| **proposer**       | **str** | Proposer address                    | [optional] |
| **status**         | **str** |                                     | [optional] |
| **votes_for**      | **str** | Total votes in favor                | [optional] |
| **votes_against**  | **str** | Total votes against                 | [optional] |
| **votes_abstain**  | **str** | Total abstain votes                 | [optional] |
| **start_time**     | **int** | Voting start timestamp              | [optional] |
| **end_time**       | **int** | Voting end timestamp                | [optional] |
| **execution_time** | **int** | Execution timestamp (if applicable) | [optional] |
| **quorum**         | **str** | Required quorum                     | [optional] |
| **threshold**      | **str** | Approval threshold                  | [optional] |

## Example

```python
from wrfcoin_sdk.models.proposal import Proposal

# TODO update the JSON string below
json = "{}"
# create an instance of Proposal from a JSON string
proposal_instance = Proposal.from_json(json)
# print the JSON string representation of the object
print(Proposal.to_json())

# convert the object into a dict
proposal_dict = proposal_instance.to_dict()
# create an instance of Proposal from a dict
proposal_from_dict = Proposal.from_dict(proposal_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
