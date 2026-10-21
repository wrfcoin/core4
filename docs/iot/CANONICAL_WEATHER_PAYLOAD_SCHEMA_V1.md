# Canonical Weather Payload Schema v1

Issue: `#389`  
Schema identifier: `wrf.weather.observation.v1`  
Status: canonical v1 contract for commit-reveal payload `data`

## Purpose

Define a deterministic, versioned weather payload contract that:
- keeps validation/math in `shared-protocol`,
- preserves compatibility for legacy alias payloads during migration,
- gives backend/frontend/device producers a single canonical output shape.

## Canonical v1 Envelope

```json
{
  "schema_version": "wrf.weather.observation.v1",
  "station_id": "station-8",
  "observed_at_unix": 1700110000,
  "observation": {
    "temperature_c": 19.5,
    "humidity": 66.0,
    "pressure_hpa": 1008.2,
    "wind_speed_ms": 4.2,
    "precipitation": 0.0,
    "dewpoint_c": 13.0
  }
}
```

## Field Contract

### Top-level fields

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `schema_version` | string | Yes (canonical payloads) | Must be exactly `wrf.weather.observation.v1` |
| `observation` | object | Yes | Must include at least one recognized weather metric |
| `station_id` | string | No | Metadata only in current `shared-protocol` parsing path |
| `observed_at_unix` | integer | No | Metadata only in current parsing path (reveal envelope timestamp is authoritative for validation) |

### Observation fields (canonical names and units)

| Canonical field | Unit | Type | Required |
| --- | --- | --- | --- |
| `temperature_c` | C | number | Optional |
| `humidity` | `%` | number | Optional |
| `pressure_hpa` | hPa | number | Optional |
| `wind_speed_ms` | m/s | number | Optional |
| `precipitation` | mm | number | Optional |
| `dewpoint_c` | C | number | Optional |

At least one observation field is required.

## `schema_version` Rules

- Canonical producers MUST send `schema_version`.
- Canonical producers MUST use only `wrf.weather.observation.v1` for v1 payloads.
- If a version marker is present but not supported, payload parsing fails.
- If no version marker is present, parser falls back to legacy alias mode during migration.

Compatibility note:
- Current parser also recognizes legacy version-key aliases (`schema`, `weather_schema_version`) for transition safety, but producers should not emit them.

## Parsing + Validation Behavior (v1)

- `schema_version == wrf.weather.observation.v1`:
  - parser reads weather values from `observation`,
  - parser rejects if `observation` is missing or has no recognized weather fields.
- Numeric values can be JSON numbers or numeric strings.
- Range and temporal validation remains deterministic in `shared-protocol`.
- Missing optional observation fields are allowed; validation applies only to fields that are present.

## Legacy Alias Compatibility (Migration Window)

When `schema_version` is absent, parser attempts legacy extraction from:
- top-level weather keys, then
- nested `observation` weather keys.

Accepted legacy aliases today:
- temperature: `temperature`, `temp_c`, `temperature_c`, `temperature_celsius`
- humidity: `humidity`, `relative_humidity`
- pressure: `pressure`, `pressure_hpa`
- wind speed: `windSpeed`, `wind_speed`, `wind_speed_ms`
- precipitation: `precipitation`, `rainfall`, `precip_mm`
- dewpoint: `dewpoint`, `dew_point`, `dewpoint_c`

## Valid Examples

Canonical v1, full:

```json
{
  "schema_version": "wrf.weather.observation.v1",
  "station_id": "station-10",
  "observed_at_unix": 1700120000,
  "observation": {
    "temperature_c": 21.4,
    "humidity": 58.0,
    "pressure_hpa": 1009.4,
    "wind_speed_ms": 5.1,
    "precipitation": 0.0,
    "dewpoint_c": 12.7
  }
}
```

Canonical v1, partial:

```json
{
  "schema_version": "wrf.weather.observation.v1",
  "observation": {
    "temperature_c": 20.0
  }
}
```

Legacy alias payload (migration compatibility):

```json
{
  "temperature": 20.0,
  "humidity": 55.0,
  "pressure": 1007.8
}
```

## Canonicalization Helper Before/After Examples

Reference helper:
- `shared-protocol/src/weather/schema.rs` -> `canonicalize_weather_payload(...)`

### Example A: Top-level legacy aliases

Before (legacy producer payload):

```json
{
  "stationId": "station-legacy-a",
  "observedAtUnix": "1700111222",
  "temperature": 21.5,
  "humidity": 61.0
}
```

After (`canonicalize_weather_payload` output):

```json
{
  "schema_version": "wrf.weather.observation.v1",
  "station_id": "station-legacy-a",
  "observed_at_unix": 1700111222,
  "observation": {
    "temperature_c": 21.5,
    "humidity": 61.0
  }
}
```

### Example B: Nested `observation` legacy aliases

Before (legacy nested keys):

```json
{
  "station_id": "station-legacy-b",
  "observed_at_unix": 1700209999,
  "observation": {
    "temp_c": 18.25,
    "dew_point": 11.5
  }
}
```

After (`canonicalize_weather_payload` output):

```json
{
  "schema_version": "wrf.weather.observation.v1",
  "station_id": "station-legacy-b",
  "observed_at_unix": 1700209999,
  "observation": {
    "temperature_c": 18.25,
    "dewpoint_c": 11.5
  }
}
```

## Invalid Examples

Unknown schema version:

```json
{
  "schema_version": "wrf.weather.observation.v2",
  "observation": {
    "temperature_c": 20.0
  }
}
```

v1 missing observation:

```json
{
  "schema_version": "wrf.weather.observation.v1",
  "temperature_c": 20.0
}
```

v1 observation with no recognized weather fields:

```json
{
  "schema_version": "wrf.weather.observation.v1",
  "observation": {
    "unknown_metric": 1
  }
}
```

## Deprecation Policy Reference

Deprecation window and cutoff criteria are defined in:
- `docs/plans/2026-02-27-issue-389-weather-schema-versioning-rollout.md`
- `docs/runbooks/WEATHER_PAYLOAD_MIGRATION_TELEMETRY_RUNBOOK.md`
