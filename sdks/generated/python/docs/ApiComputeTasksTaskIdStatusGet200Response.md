# ApiComputeTasksTaskIdStatusGet200Response

## Properties

| Name                     | Type                                                                                                                    | Description | Notes      |
| ------------------------ | ----------------------------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| **task_id**              | **str**                                                                                                                 |             | [optional] |
| **status**               | **str**                                                                                                                 |             | [optional] |
| **progress**             | **int**                                                                                                                 |             | [optional] |
| **provider**             | **str**                                                                                                                 |             | [optional] |
| **estimated_completion** | **str**                                                                                                                 |             | [optional] |
| **resource_usage**       | [**ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage**](ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage.md) |             | [optional] |
| **cost**                 | **str**                                                                                                                 |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_compute_tasks_task_id_status_get200_response import ApiComputeTasksTaskIdStatusGet200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ApiComputeTasksTaskIdStatusGet200Response from a JSON string
api_compute_tasks_task_id_status_get200_response_instance = ApiComputeTasksTaskIdStatusGet200Response.from_json(json)
# print the JSON string representation of the object
print(ApiComputeTasksTaskIdStatusGet200Response.to_json())

# convert the object into a dict
api_compute_tasks_task_id_status_get200_response_dict = api_compute_tasks_task_id_status_get200_response_instance.to_dict()
# create an instance of ApiComputeTasksTaskIdStatusGet200Response from a dict
api_compute_tasks_task_id_status_get200_response_from_dict = ApiComputeTasksTaskIdStatusGet200Response.from_dict(api_compute_tasks_task_id_status_get200_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
