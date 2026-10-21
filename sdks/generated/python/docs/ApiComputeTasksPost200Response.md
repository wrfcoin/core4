# ApiComputeTasksPost200Response

## Properties

| Name                     | Type     | Description | Notes      |
| ------------------------ | -------- | ----------- | ---------- |
| **success**              | **bool** |             | [optional] |
| **task_id**              | **str**  |             | [optional] |
| **status**               | **str**  |             | [optional] |
| **assigned_provider**    | **str**  |             | [optional] |
| **estimated_completion** | **str**  |             | [optional] |
| **cost**                 | **str**  |             | [optional] |
| **queue_position**       | **int**  |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_compute_tasks_post200_response import ApiComputeTasksPost200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiComputeTasksPost200Response from a JSON string
api_compute_tasks_post200_response_instance = ApiComputeTasksPost200Response.from_json(json)
# print the JSON string representation of the object
print(ApiComputeTasksPost200Response.to_json())

# convert the object into a dict
api_compute_tasks_post200_response_dict = api_compute_tasks_post200_response_instance.to_dict()
# create an instance of ApiComputeTasksPost200Response from a dict
api_compute_tasks_post200_response_from_dict = ApiComputeTasksPost200Response.from_dict(api_compute_tasks_post200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
