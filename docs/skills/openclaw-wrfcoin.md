# WRFCoin Weather Contributor

Earn WRF tokens by contributing weather data, running forecast models, and sharing environmental sensor readings through the WRFCoin decentralized weather network.

## Activation

This skill activates when the user mentions:
- weather station, weather data, sensor data, weather readings
- WRFCoin, WRF tokens, WRF earnings, weather earnings
- submit forecast, publish forecast, ensemble forecast
- station status, quality score, data quality
- carbon credits, carbon footprint

## What You Can Do

You are a WRFCoin weather network contributor. You help the user:

1. **Submit weather sensor readings** to earn WRF tokens
2. **Run and publish weather forecasts** using available compute
3. **Monitor station health** and data quality scores
4. **Track earnings** and achievement progress
5. **Manage wallet** and inspect native WRF balances and transactions

## Setup

Before first use, ensure the WRFCoin CLI or API is accessible:

```bash
# Option A: CLI (preferred)
wrfcoin --version

# Option B: Direct API
curl -s http://localhost:4001/health | jq .status

# Option C: Weather earnings API
curl -s http://localhost:8080/health
```

Set your API key for authenticated submissions:
```bash
export WRFCOIN_API_KEY="your-api-key"
export WRFCOIN_API_URL="http://localhost:4001"
```

## Commands Reference

### Weather Data Submission

Submit a single sensor reading:
```bash
wrfcoin weather submit \
  --parameter temperature \
  --value 23.4 \
  --unit celsius \
  --latitude 40.7128 \
  --longitude -74.0060 \
  --device-id "ws-001" \
  --source "ambient-weather" \
  --json
```

Submit a batch of readings from file:
```bash
wrfcoin weather submit-batch --file readings.json --validate --json
```

Submit wind data (combined speed and direction):
```bash
wrfcoin weather submit-wind \
  --speed 12.3 \
  --direction 315 \
  --unit km/h \
  --latitude 40.7128 \
  --longitude -74.0060 \
  --device-id "ws-001" \
  --json
```

Pipe sensor data from stdin:
```bash
echo '{"parameter":"temperature","value":23.4,"unit":"celsius","lat":40.7128,"lon":-74.006}' \
  | wrfcoin weather submit --station ws-001 --stdin --json
```

### Forecast Publishing

Publish a weather forecast:
```bash
wrfcoin forecast publish \
  --title "24h NYC Temperature Forecast" \
  --forecast-type weather \
  --region "US-NY-NYC" \
  --data-file forecast_output.json \
  --validity 24 \
  --confidence 85 \
  --json
```

Publish an ensemble forecast (multiple model runs):
```bash
wrfcoin forecast publish-ensemble \
  --title "48h Northeast Ensemble" \
  --forecast-type weather \
  --region "US-NE" \
  --ensemble-dir ./ensemble_members/ \
  --validity 48 \
  --json
```

Validate a forecast against observed data:
```bash
wrfcoin forecast validate <forecast-id> \
  --observed-data observations.json \
  --method rmse \
  --save-results \
  --json
```

### Station Monitoring

Check station status and quality:
```bash
wrfcoin monitor status --json
wrfcoin monitor iot --json
```

Check earnings:
```bash
curl -s http://localhost:8080/weather/station/ws-001/metrics | jq
curl -s http://localhost:8080/weather/station/ws-001/achievements | jq
curl -s http://localhost:8080/weather/earnings/history?stationId=ws-001 | jq
```

### Wallet Operations

Check balance:
```bash
wrfcoin wallet balance --json
```

Send WRF tokens:
```bash
wrfcoin wallet send --to <address> --amount 10 --json
```

View transaction history:
```bash
wrfcoin wallet history --json
```

### Network Status

```bash
wrfcoin monitor status --json       # Overall health
wrfcoin monitor blockchain --json   # Chain metrics
```

## Data Quality Requirements

WRFCoin validates weather data using physics-based checks. Higher quality scores earn more WRF:

| Validation | Check | Impact |
|-----------|-------|--------|
| Range | Temperature: -89.2C to 70.7C | Reject if outside |
| Temporal | Change rate within physical limits | Score penalty |
| Spatial | Agreement with nearby stations | Score penalty |
| Cross-parameter | Logical relationships (e.g., dewpoint <= temperature) | Score penalty |

**Quality score tiers and earnings multiplier:**
- 95-100: Diamond (2.0x)
- 90-94: Platinum (1.5x)
- 85-89: Gold (1.2x)
- 80-84: Silver (1.0x)
- 70-79: Bronze (0.8x)
- Below 70: No rewards

## Achievement System

Consistent high-quality contributions unlock achievement tiers:

| Achievement | Requirement | Bonus |
|-----------|------------|-------|
| Data Collector | Total readings submitted | Bronze 100 / Silver 1K / Gold 10K / Platinum 100K / Diamond 1M |
| Consistency Champion | Consecutive days submitting | 7d / 30d / 90d / 180d / 365d |
| Quality Pioneer | Average quality score maintained | 70+ / 80+ / 85+ / 90+ / 95+ |

## Batch Submission Format

For `wrfcoin weather submit-batch`, prepare a JSON file:

```json
{
  "station_id": "ws-001",
  "source": "ambient-weather",
  "readings": [
    {
      "timestamp": "2026-02-16T12:00:00Z",
      "parameters": {
        "temperature": { "value": 23.4, "unit": "celsius" },
        "humidity": { "value": 65.2, "unit": "percent" },
        "pressure": { "value": 1013.25, "unit": "hpa" },
        "wind_speed": { "value": 12.3, "unit": "km/h" },
        "wind_direction": { "value": 315, "unit": "degrees" },
        "precipitation": { "value": 0.0, "unit": "mm" }
      },
      "location": { "lat": 40.7128, "lon": -74.006, "elevation": 10.0 }
    }
  ]
}
```

## Forecast Data Format

For `wrfcoin forecast publish`, prepare a JSON file:

```json
{
  "model": "wrf-arw-4km",
  "init_time": "2026-02-16T00:00:00Z",
  "grid": {
    "type": "lat-lon",
    "resolution_km": 4.0,
    "bounds": { "north": 45.0, "south": 38.0, "east": -70.0, "west": -80.0 }
  },
  "parameters": ["temperature_2m", "precipitation", "wind_speed_10m", "wind_direction_10m"],
  "timesteps": [
    {
      "valid_time": "2026-02-16T06:00:00Z",
      "forecast_hour": 6,
      "fields": {
        "temperature_2m": { "mean": 5.2, "unit": "celsius" },
        "precipitation": { "sum": 0.3, "unit": "mm" },
        "wind_speed_10m": { "mean": 15.4, "unit": "km/h" },
        "wind_direction_10m": { "mean": 270, "unit": "degrees" }
      }
    }
  ]
}
```

## Automation Patterns

### Scheduled Data Collection (via cron)

Collect and submit every 5 minutes:
```bash
# crontab entry
*/5 * * * * /path/to/collect-and-submit.sh
```

```bash
#!/bin/bash
# collect-and-submit.sh
READING=$(curl -s "http://your-station-ip/api/current")
echo "$READING" | wrfcoin weather submit --station ws-001 --stdin --json >> /var/log/wrfcoin-submissions.log 2>&1
```

### Ambient Weather API Collector

```bash
# Collect from Ambient Weather API
curl -s "https://rt.ambientweather.net/v1/devices?applicationKey=$APP_KEY&apiKey=$API_KEY" \
  | jq '.[0].lastData | {
      parameter: "temperature",
      value: .tempf,
      unit: "fahrenheit",
      latitude: .coords.coords.lat,
      longitude: .coords.coords.lon
    }' \
  | wrfcoin weather submit --stdin --device-id "ambient-001" --json
```

### Run WRF Model and Submit Forecast

```bash
# 1. Run WRF model (if you have compute access)
# 2. Post-process output to WRFCoin format
python3 wrf_to_wrfcoin.py --input wrfout_d01.nc --output forecast.json

# 3. Publish forecast
wrfcoin forecast publish \
  --title "24h Local Forecast $(date +%Y-%m-%d)" \
  --forecast-type weather \
  --region "US-NY-NYC" \
  --data-file forecast.json \
  --validity 24 \
  --json
```

## Error Handling

All commands with `--json` return structured responses:

```json
// Success
{ "ok": true, "data": { "quality_score": 87, "reward_pending": 1.23, "tx_hash": "0xabc..." } }

// Error
{ "ok": false, "error": { "code": "VALIDATION_FAILED", "message": "Temperature 999C exceeds physical range", "details": { "parameter": "temperature", "value": 999, "max": 70.7 } } }
```

**Exit codes:**
| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Input/validation error |
| 2 | Network error (API unreachable) |
| 3 | Authentication error |
| 4 | Rate limited |
| 5 | Blockchain error |

## Earning WRF Tokens

The earning flow:
1. Submit quality weather data or verified forecasts
2. Data passes physics-based validation and quality scoring
3. WRF tokens accrue to your wallet based on quality tier
4. Track earnings via `wrfcoin wallet balance --json`

## Links

- WRFCoin GitHub: https://github.com/wrfcoin/core4
- WRFCoin CLI: `cargo install wrfcoin`
- API Docs: http://localhost:4001/api-explorer
- Weather Earnings API: http://localhost:8080
- Supported IoT Platforms: AcuRite, Ambient Weather, Davis Instruments, Netatmo, Ecowitt, PurpleAir, Awair, The Things Network, Helium, IoTeX, Sensor.Community, OpenAQ, and 10+ more
