# WeatherMeasurement

## Properties

| Name            | Type                      | Description             | Notes      |
| --------------- | ------------------------- | ----------------------- | ---------- |
| **type**        | [**TypeEnum**](#TypeEnum) |                         |            |
| **value**       | **BigDecimal**            | Measurement value       |            |
| **unit**        | **String**                | Unit of measurement     |            |
| **uncertainty** | **BigDecimal**            | Measurement uncertainty | [optional] |

## Enum: TypeEnum

| Name           | Value                      |
| -------------- | -------------------------- |
| TEMPERATURE    | &quot;temperature&quot;    |
| HUMIDITY       | &quot;humidity&quot;       |
| PRESSURE       | &quot;pressure&quot;       |
| WIND_SPEED     | &quot;wind_speed&quot;     |
| WIND_DIRECTION | &quot;wind_direction&quot; |
| PRECIPITATION  | &quot;precipitation&quot;  |
| VISIBILITY     | &quot;visibility&quot;     |
| UV_INDEX       | &quot;uv_index&quot;       |
