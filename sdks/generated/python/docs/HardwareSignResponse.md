# HardwareSignResponse

## Properties

| Name                 | Type     | Description                      | Notes      |
| -------------------- | -------- | -------------------------------- | ---------- |
| **signature**        | **str**  | Transaction signature            | [optional] |
| **public_key**       | **str**  | Public key used for signing      | [optional] |
| **device_confirmed** | **bool** | Whether user confirmed on device | [optional] |
| **timestamp**        | **int**  | Signing timestamp                | [optional] |

## Example

```python
from wrfcoin_sdk.models.hardware_sign_response import HardwareSignResponse

# TODO update the JSON string below
json = "{}"
# create an instance of HardwareSignResponse from a JSON string
hardware_sign_response_instance = HardwareSignResponse.from_json(json)
# print the JSON string representation of the object
print(HardwareSignResponse.to_json())

# convert the object into a dict
hardware_sign_response_dict = hardware_sign_response_instance.to_dict()
# create an instance of HardwareSignResponse from a dict
hardware_sign_response_from_dict = HardwareSignResponse.from_dict(hardware_sign_response_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
