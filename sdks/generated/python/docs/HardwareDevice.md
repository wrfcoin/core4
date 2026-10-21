# HardwareDevice

## Properties

| Name                   | Type          | Description                | Notes      |
| ---------------------- | ------------- | -------------------------- | ---------- |
| **device_id**          | **str**       | Unique device identifier   | [optional] |
| **device_type**        | **str**       |                            | [optional] |
| **device_name**        | **str**       | Human-readable device name | [optional] |
| **firmware_version**   | **str**       | Device firmware version    | [optional] |
| **is_connected**       | **bool**      | Connection status          | [optional] |
| **is_authenticated**   | **bool**      | Authentication status      | [optional] |
| **supported_features** | **List[str]** | List of supported features | [optional] |

## Example

```python
from wrfcoin_sdk.models.hardware_device import HardwareDevice

# TODO update the JSON string below
json = "{}"
# create an instance of HardwareDevice from a JSON string
hardware_device_instance = HardwareDevice.from_json(json)
# print the JSON string representation of the object
print(HardwareDevice.to_json())

# convert the object into a dict
hardware_device_dict = hardware_device_instance.to_dict()
# create an instance of HardwareDevice from a dict
hardware_device_from_dict = HardwareDevice.from_dict(hardware_device_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
