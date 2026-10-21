# ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage

## Properties

| Name        | Type      | Description | Notes      |
| ----------- | --------- | ----------- | ---------- |
| **cpu**     | **float** |             | [optional] |
| **memory**  | **float** |             | [optional] |
| **storage** | **float** |             | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_compute_tasks_task_id_status_get200_response_resource_usage import ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage

# TODO update the JSON string below
json = "{}"
# create an instance of ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage from a JSON string
api_compute_tasks_task_id_status_get200_response_resource_usage_instance = ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage.from_json(json)
# print the JSON string representation of the object
print(ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage.to_json())

# convert the object into a dict
api_compute_tasks_task_id_status_get200_response_resource_usage_dict = api_compute_tasks_task_id_status_get200_response_resource_usage_instance.to_dict()
# create an instance of ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage from a dict
api_compute_tasks_task_id_status_get200_response_resource_usage_from_dict = ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage.from_dict(api_compute_tasks_task_id_status_get200_response_resource_usage_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
