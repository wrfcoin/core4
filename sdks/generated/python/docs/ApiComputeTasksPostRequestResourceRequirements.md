# ApiComputeTasksPostRequestResourceRequirements

## Properties

| Name        | Type     | Description        | Notes      |
| ----------- | -------- | ------------------ | ---------- |
| **cpu**     | **int**  | CPU cores required | [optional] |
| **memory**  | **int**  | Memory in GB       | [optional] |
| **storage** | **int**  | Storage in GB      | [optional] |
| **gpu**     | **bool** | GPU requirement    | [optional] |

## Example

```python
from wrfcoin_sdk.models.api_compute_tasks_post_request_resource_requirements import ApiComputeTasksPostRequestResourceRequirements

# TODO update the JSON string below
json = "{}"
# create an instance of ApiComputeTasksPostRequestResourceRequirements from a JSON string
api_compute_tasks_post_request_resource_requirements_instance = ApiComputeTasksPostRequestResourceRequirements.from_json(json)
# print the JSON string representation of the object
print(ApiComputeTasksPostRequestResourceRequirements.to_json())

# convert the object into a dict
api_compute_tasks_post_request_resource_requirements_dict = api_compute_tasks_post_request_resource_requirements_instance.to_dict()
# create an instance of ApiComputeTasksPostRequestResourceRequirements from a dict
api_compute_tasks_post_request_resource_requirements_from_dict = ApiComputeTasksPostRequestResourceRequirements.from_dict(api_compute_tasks_post_request_resource_requirements_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
