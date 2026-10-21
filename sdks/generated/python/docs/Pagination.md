# Pagination

## Properties

| Name             | Type     | Description                      | Notes      |
| ---------------- | -------- | -------------------------------- | ---------- |
| **limit**        | **int**  | Items per page                   | [optional] |
| **offset**       | **int**  | Items to skip                    | [optional] |
| **total**        | **int**  | Total number of items            | [optional] |
| **has_next**     | **bool** | Whether there are more items     | [optional] |
| **has_previous** | **bool** | Whether there are previous items | [optional] |

## Example

```python
from wrfcoin_sdk.models.pagination import Pagination

# TODO update the JSON string below
json = "{}"
# create an instance of Pagination from a JSON string
pagination_instance = Pagination.from_json(json)
# print the JSON string representation of the object
print(Pagination.to_json())

# convert the object into a dict
pagination_dict = pagination_instance.to_dict()
# create an instance of Pagination from a dict
pagination_from_dict = Pagination.from_dict(pagination_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
