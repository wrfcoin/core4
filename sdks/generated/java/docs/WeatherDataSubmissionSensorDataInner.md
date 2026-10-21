# WeatherDataSubmissionSensorDataInner

## Properties

| Name       | Type                          | Description          | Notes      |
| ---------- | ----------------------------- | -------------------- | ---------- |
| **type**   | [**TypeEnum**](#TypeEnum)     |                      | [optional] |
| **value**  | **BigDecimal**                | Sensor reading value | [optional] |
| **unit**   | **String**                    | Unit of measurement  | [optional] |
| **status** | [**StatusEnum**](#StatusEnum) |                      | [optional] |

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

## Enum: StatusEnum

| Name     | Value                |
| -------- | -------------------- |
| ACTIVE   | &quot;active&quot;   |
| INACTIVE | &quot;inactive&quot; |
| ERROR    | &quot;error&quot;    |
