# Location

## Properties

| Name          | Type      | Description                  | Notes      |
| ------------- | --------- | ---------------------------- | ---------- |
| **latitude**  | **float** | Latitude in decimal degrees  |
| **longitude** | **float** | Longitude in decimal degrees |
| **altitude**  | **float** | Altitude in meters           | [optional] |
| **accuracy**  | **float** | Location accuracy in meters  | [optional] |

## Example

```python
from wrfcoin_sdk.models.location import Location

# TODO update the JSON string below
json = "{}"
# create an instance of Location from a JSON string
location_instance = Location.from_json(json)
# print the JSON string representation of the object
print(Location.to_json())

# convert the object into a dict
location_dict = location_instance.to_dict()
# create an instance of Location from a dict
location_from_dict = Location.from_dict(location_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
