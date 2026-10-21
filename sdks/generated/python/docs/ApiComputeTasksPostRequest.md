# ApiComputeTasksPostRequest

## Properties

| Name                      | Type                                                                                                    | Description          | Notes      |
| ------------------------- | ------------------------------------------------------------------------------------------------------- | -------------------- | ---------- |
| **task_type**             | **str**                                                                                                 | Type of compute task |
| **priority**              | **str**                                                                                                 | Task priority level  | [optional] |
| **resource_requirements** | [**ApiComputeTasksPostRequestResourceRequirements**](ApiComputeTasksPostRequestResourceRequirements.md) |                      | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_compute_tasks_post_request import ApiComputeTasksPostRequest

# TODO update the JSON string below
json = "{}"
# create an instance of ApiComputeTasksPostRequest from a JSON string
api_compute_tasks_post_request_instance = ApiComputeTasksPostRequest.from_json(json)
# print the JSON string representation of the object
print(ApiComputeTasksPostRequest.to_json())

# convert the object into a dict
api_compute_tasks_post_request_dict = api_compute_tasks_post_request_instance.to_dict()
# create an instance of ApiComputeTasksPostRequest from a dict
api_compute_tasks_post_request_from_dict = ApiComputeTasksPostRequest.from_dict(api_compute_tasks_post_request_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
