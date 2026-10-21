# HardwareSignRequest

## Properties

| Name                     | Type     | Description                       | Notes                        |
| ------------------------ | -------- | --------------------------------- | ---------------------------- |
| **device_id**            | **str**  | Hardware device ID                |
| **transaction_hash**     | **str**  | Transaction hash to sign          |
| **derivation_path**      | **str**  | BIP32 derivation path             |
| **display_on_device**    | **bool** | Show transaction on device screen | [optional] [default to True] |
| **require_confirmation** | **bool** | Require user confirmation         | [optional] [default to True] |

## Example

```python
from wrfcoin_sdk.models.hardware_sign_request import HardwareSignRequest

# TODO update the JSON string below
json = "{}"
# create an instance of HardwareSignRequest from a JSON string
hardware_sign_request_instance = HardwareSignRequest.from_json(json)
# print the JSON string representation of the object
print(HardwareSignRequest.to_json())

# convert the object into a dict
hardware_sign_request_dict = hardware_sign_request_instance.to_dict()
# create an instance of HardwareSignRequest from a dict
hardware_sign_request_from_dict = HardwareSignRequest.from_dict(hardware_sign_request_dict)
```

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
