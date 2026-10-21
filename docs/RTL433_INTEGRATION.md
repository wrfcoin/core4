# RTL-433 RF Sensor Data Integration

## Overview

The RTL-433 integration enables WRFCoin nodes to ingest weather data from RF sensors (e.g., Acurite, Ambient Weather, Davis, Netatmo) using an RTL-SDR USB dongle and `rtl_433` software.

**Data Pipeline:**
```
RF Sensor → RTL-SDR → rtl_433 → MQTT → IoT Gateway → RTL-433 Handler →
Normalization → Deduplication → Physics Validation → Weather API (backend) →
Database & Analytics
```

## Architecture

### Components

1. **RTL-SDR Dongle** — Software-defined radio receiver (RTL2832U chipset, ~$25)
   - Receives RF signals from weather stations on 433 MHz
   - USB connection to compute node

2. **rtl_433 Software** — Open-source RF decoder
   - Receives raw RF data from RTL-SDR
   - Detects protocol (Acurite, Ambient Weather, Davis, etc.)
   - Outputs decoded JSON via stdout or MQTT
   - Runs in Docker container

3. **MQTT Broker** (Eclipse Mosquitto)
   - Receives rtl_433 JSON output on `rtl_433/+/json` topic
   - Holds readings for subscribed clients
   - Port 1883 (internal network)

4. **IoT Edge Gateway** (`server/iot-edge-gateway/`)
   - Runs MQTT server for sensor device registration
   - RTL433Handler subscribes to `rtl_433/+/json` topics
   - Normalizes protocol-specific fields to canonical schema
   - Deduplicates readings (same station, timestamp)
   - Validates physics constraints
   - Emits normalized readings to `wrfcoin/weather/rtl433-ready` topic

5. **RTL-433 Ingestion Microservice** (port 3052)
   - Consumes normalized readings from MQTT
   - Submits to `POST /api/weather/data` with `source=rf-sdr` tag
   - Retries on failure
   - Tracks metrics (received, submitted, failures)

6. **Backend Weather API** (port 4001)
   - Receives RTL-433 readings tagged with `source: 'rf-sdr'`
   - Stores in PostgreSQL database
   - Makes data available for analytics, reports, rewards

## Supported Sensor Protocols

| Protocol | Device Examples | Fields Decoded |
|----------|---|---|
| **Acurite** | 606TX, 00986A, 05422 | temp_c, humidity, wind_speed_kmh, wind_dir |
| **Ambient Weather** | WH65, WH80, WH2 | temp_c, humidity, pressure, wind, rain |
| **Davis** | Vantage Pro, Vue | temp_c, humidity, wind, pressure, rain |
| **Netatmo** | Weather Station, Rain Gauge | temp_c, humidity, pressure |
| **Fine Offset** | WH1080, WH2400 | temp_c, humidity, wind, rain, pressure |

## Setup & Deployment

### Hardware Requirements

- RTL-SDR dongle (RTL2832U-based, ~$25)
  - Model: NooElec NESDR, FlightAware Pro Stick, etc.
  - USB 2.0/3.0 connection
- Compute node with Docker + USB passthrough
- RF antenna (included with most dongles)
- Line-of-sight to target weather stations (100-300m range)

### Docker Compose Deployment

```bash
# Start RTL-433 stack (includes rtl_433, MQTT, IoT Gateway, ingestion service)
docker-compose -f infra/docker/docker-compose.rtl433.yml up -d

# Check service status
docker-compose -f infra/docker/docker-compose.rtl433.yml ps

# View logs
docker logs wrfcoin-rtl-433
docker logs wrfcoin-mqtt-broker
docker logs wrfcoin-iot-gateway
docker logs wrfcoin-rtl433-ingestion
```

### Manual Deployment (Testing)

```bash
# 1. Start MQTT broker
docker run -d --name mqtt -p 1883:1883 eclipse-mosquitto

# 2. Start RTL-433 decoder
docker run -d --name rtl433 \
  --device=/dev/bus/usb \
  rtl433/rtl_433:latest \
  rtl_433 -F json -M newmodel -f 433.05M

# 3. Start IoT Gateway (requires backend code)
cd backend
npm run start:iot-gateway

# 4. Start RTL-433 Ingestion Service
npm run start:rtl433-ingestion

# 5. Verify services
curl http://localhost:5684/health        # IoT Gateway
curl http://localhost:3052/health        # RTL-433 Service
curl http://localhost:3052/metrics       # RTL-433 Metrics
```

## Data Flow & Normalization

### Example: Acurite 606TX Sensor Reading

**Raw RTL-433 Output (JSON):**
```json
{
  "time": "2026-04-09T18:30:45.123456Z",
  "model": "Acurite 606TX",
  "id": 42,
  "channel": 1,
  "temp_c": 22.5,
  "humidity": 65,
  "wind_speed_kmh": 12.5,
  "wind_direction": 180,
  "battery_ok": 1
}
```

**RTL433Handler Normalization:**
```json
{
  "stationId": "rtl433-acurite-606tx-42",
  "timestamp": 1712718645123,
  "temperature_c": 22.5,
  "humidity_percent": 65,
  "wind_speed_ms": 3.47,
  "wind_direction_deg": 180,
  "source": "rf-sdr",
  "protocol": "acurite",
  "messageId": 42
}
```

**Backend Weather API Submission:**
```bash
POST /api/weather/data
Content-Type: application/json
Authorization: Bearer <token>

{
  "stationId": "rtl433-acurite-606tx-42",
  "timestamp": "2026-04-09T18:30:45.123Z",
  "temperature_c": 22.5,
  "humidity_percent": 65,
  "wind_speed_ms": 3.47,
  "wind_direction_deg": 180,
  "source": "rf-sdr",
  "protocol": "acurite"
}
```

## Deduplication & Validation

### Deduplication Window

Readings are deduplicated within a 60-second sliding window using:
- **Key:** `{stationId}:{timestamp}:{messageId}`
- **Behavior:** Duplicate readings (same station, time, message ID) are silently dropped
- **Why:** RF protocols often re-transmit messages; deduplication prevents double-counting

### Physics Validation

Readings are validated against real-world constraints:

| Field | Min | Max | Unit | Notes |
|-------|-----|-----|------|-------|
| Temperature | -90 | 75 | °C | Antarctica to Death Valley extremes |
| Humidity | 0 | 100 | % | Percentage saturation |
| Pressure | 850 | 1100 | hPa | Sea-level pressure range |
| Wind Speed | 0 | 100 | m/s | Hurricane-force upper limit |
| Wind Direction | 0 | 360 | ° | Compass bearing |
| Precipitation | 0 | 1000 | mm | Extreme daily rainfall |

**Example: Validation Failure**
```
[RTL433] temperature out of range: -150°C → REJECTED
[RTL433] humidity out of range: 150% → REJECTED
[RTL433] wind speed out of range: 200 m/s → REJECTED
```

## Monitoring & Metrics

### Health Checks

**IoT Gateway:**
```bash
curl http://localhost:5684/health
```
Response:
```json
{
  "status": "healthy",
  "mqtt": { "port": 1883, "connections": 3 },
  "coap": { "port": 5683 },
  "batchSigner": { "pendingReadings": 5, "lastFlush": "2026-04-09T18:35:12Z" },
  "registeredDevices": 12
}
```

**RTL-433 Ingestion Service:**
```bash
curl http://localhost:3052/health
curl http://localhost:3052/metrics
curl http://localhost:3052/config
```

### Metrics Endpoint

```json
{
  "service": "rtl433-ingestion",
  "metrics": {
    "readingsReceived": 1542,
    "readingsSubmitted": 1520,
    "submissionFailures": 22,
    "startTime": "2026-04-09T12:00:00Z",
    "lastReadingTime": "2026-04-09T18:35:47Z",
    "uptime": 23647,
    "successRate": 98.57
  }
}
```

## Troubleshooting

### RTL-SDR Dongle Not Recognized

```bash
# Check USB device
lsusb
# Expected: ID 0bda:2838 Realtek Semiconductor Corp. RTL2838 DVB-T

# Check permissions (may need udev rules)
ls -la /dev/bus/usb/
sudo usermod -a -G plugdev $USER

# Test rtl_433
docker run --rm --device=/dev/bus/usb rtl433/rtl_433:latest rtl_433 -e help
```

### No Readings from Sensors

1. **Check rtl_433 is receiving signals:**
   ```bash
   docker logs wrfcoin-rtl-433 | grep -i signal
   ```

2. **Verify antenna connection:** Ensure antenna is firmly attached to RTL-SDR dongle

3. **Check sensor battery:** RF sensors often have battery status in output

4. **Test rtl_433 standalone:**
   ```bash
   docker exec -it wrfcoin-rtl-433 rtl_433 -e all
   ```

5. **Verify MQTT broker:**
   ```bash
   docker exec -it mqtt mosquitto_sub -h mqtt -t 'rtl_433/+/json'
   ```

### Submission Failures to Backend

```bash
# Check service is running
docker ps | grep rtl433-ingestion

# View logs
docker logs wrfcoin-rtl433-ingestion

# Verify backend API is accessible
curl http://localhost:4001/health

# Check metrics
curl http://localhost:3052/metrics | jq .metrics.submissionFailures
```

### High Failure Rate

- **Check backend database:** `SELECT COUNT(*) FROM weather_data WHERE source='rf-sdr'`
- **Check retry configuration:** RTL-433 service retries 3x with exponential backoff
- **Check network connectivity:** MQTT → backend routing

## Field Mapping Reference

### Acurite Protocol
| RTL-433 Field | Canonical Field | Unit Conversion |
|---|---|---|
| temp_c | temperature_c | (no conversion) |
| humidity | humidity_percent | (no conversion) |
| wind_speed_kmh | wind_speed_ms | ÷ 3.6 |
| wind_direction | wind_direction_deg | (no conversion) |

### Ambient Weather Protocol
| RTL-433 Field | Canonical Field | Unit Conversion |
|---|---|---|
| temp_c | temperature_c | (no conversion) |
| humidity | humidity_percent | (no conversion) |
| windspeed_ms | wind_speed_ms | (no conversion) |
| wind_dir | wind_direction_deg | (no conversion) |
| rain_mm | precipitation_mm | (no conversion) |
| pressure_hpa | pressure_hpa | (no conversion) |

## API Integration

### Submitting Readings via REST (Manual Testing)

```bash
# Get authentication token
TOKEN=$(curl -X POST http://localhost:4001/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","password":"password"}' \
  | jq -r '.token')

# Submit RTL-433 reading
curl -X POST http://localhost:4001/api/weather/data \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "stationId": "rtl433-ambient-weather-1234",
    "timestamp": "2026-04-09T18:30:45Z",
    "temperature_c": 22.5,
    "humidity_percent": 65,
    "wind_speed_ms": 3.5,
    "wind_direction_deg": 180,
    "source": "rf-sdr",
    "protocol": "ambient-weather"
  }'
```

### Backend Storage

RTL-433 readings are stored in PostgreSQL with source tag:
```sql
SELECT stationId, timestamp, temperature_c, source, protocol
FROM weather_data
WHERE source = 'rf-sdr'
ORDER BY timestamp DESC
LIMIT 10;
```

## Performance Considerations

- **MQTT throughput:** ~1000 messages/sec (typically 1-10 readings/sec from RF sensors)
- **Network bandwidth:** ~500 bytes/reading (JSON overhead)
- **CPU:** Minimal (JSON parsing, deduplication, validation)
- **Typical latency:** <1 second from sensor transmission to database

## Future Enhancements

- [ ] Geographic clustering (deduplicate readings from multiple sensors in same location)
- [ ] Quality scoring (signal strength, battery status)
- [ ] Multi-frequency support (2-band RF decoding)
- [ ] Custom protocol support (community-contributed sensors)
- [ ] Real-time dashboards (WebSocket feed for live readings)

## References

- [rtl_433 Project](https://github.com/merbanan/rtl_433)
- [RTL-SDR Dongles](https://www.rtl-sdr.com/)
- [Acurite Protocol](https://github.com/merbanan/rtl_433/blob/master/src/devices/acurite.c)
- [WRFCoin Weather API](../backend/CLAUDE.md)
