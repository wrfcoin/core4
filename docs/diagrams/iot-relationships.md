# Environmental & Sensor Data - Schema Relationships

## Tables Overview

### `iot.environmental_data_submissions`

**Primary Key:** `id`

**Relationships:**

- one to_many with `iot.environmental_attestations` (via `submission_id`)

### `iot.luftdaten_sensors`

**Primary Key:** `id`

**Relationships:**

- one to_many with `iot.luftdaten_measurements` (via `sensor_id`)

### `iot.luftdaten_measurements`

**Primary Key:** `id`

**Relationships:**

- many to_one with `iot.luftdaten_sensors` (via `sensor_id`)

### `iot.ttn_devices`

**Primary Key:** `id`

**Relationships:**

- one to_many with `iot.ttn_measurements` (via `device_id`)
- one to_many with `iot.ttn_sensors` (via `device_id`)

### `iot.streamr_streams`

**Primary Key:** `id`

**Relationships:**

- one to_many with `iot.streamr_data_points` (via `stream_id`)
- one to_many with `iot.streamr_subscriptions` (via `stream_id`)
