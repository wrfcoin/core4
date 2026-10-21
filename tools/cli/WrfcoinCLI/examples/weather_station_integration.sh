#!/bin/bash
# Example: Weather Station Integration with Wrfcoin CLI
# This script demonstrates how to set up a weather station and automate data submission

# Step 1: Set up the environment
echo "Setting up Wrfcoin environment for weather station..."
wrfcoin setup init --dir ./wrfcoin-weather-station --network testnet

# Step 2: Configure weather station
echo "Configuring weather station..."
wrfcoin setup configure-station \
  --station-type davis-vantage \
  --station-id "DV-1234" \
  --location "40.7128,-74.0060" \
  --name "NYC Central Park Station" \
  --elevation 25 \
  --auto-submit

# Step 3: Create a dedicated wallet for the station
echo "Creating wallet for weather station..."
STATION_ADDRESS=$(wrfcoin wallet new-address --name "NYC Weather Station" --json | jq -r '.address')
echo "Weather station wallet address: $STATION_ADDRESS"

# Step 4: Submit individual temperature reading
echo "Submitting temperature data..."
wrfcoin weather submit \
  --parameter temperature \
  --value 22.5 \
  --latitude 40.7128 \
  --longitude -74.0060 \
  --elevation 25 \
  --unit celsius \
  --timestamp "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
  --station-id "DV-1234" \
  --source-address "$STATION_ADDRESS"

# Step 5: Submit humidity reading
echo "Submitting humidity data..."
wrfcoin weather submit \
  --parameter humidity \
  --value 65.3 \
  --latitude 40.7128 \
  --longitude -74.0060 \
  --elevation 25 \
  --unit percent \
  --timestamp "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
  --station-id "DV-1234" \
  --source-address "$STATION_ADDRESS"

# Step 6: Submit pressure reading
echo "Submitting pressure data..."
wrfcoin weather submit \
  --parameter pressure \
  --value 1013.25 \
  --latitude 40.7128 \
  --longitude -74.0060 \
  --elevation 25 \
  --unit hPa \
  --timestamp "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
  --station-id "DV-1234" \
  --source-address "$STATION_ADDRESS"

# Step 7: Submit wind data (combined speed and direction)
echo "Submitting wind data..."
wrfcoin weather submit-wind \
  --speed 5.2 \
  --direction 225 \
  --latitude 40.7128 \
  --longitude -74.0060 \
  --elevation 25 \
  --unit "m/s" \
  --timestamp "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
  --station-id "DV-1234" \
  --source-address "$STATION_ADDRESS"

# Step 8: Create a batch submission file
cat > weather_batch.json << EOF
{
  "station_id": "DV-1234",
  "location": {
    "latitude": 40.7128,
    "longitude": -74.0060,
    "elevation": 25
  },
  "readings": [
    {
      "parameter": "temperature",
      "value": 22.8,
      "unit": "celsius",
      "timestamp": "$(date -u -d '10 minutes ago' +"%Y-%m-%dT%H:%M:%SZ")"
    },
    {
      "parameter": "humidity",
      "value": 66.1,
      "unit": "percent",
      "timestamp": "$(date -u -d '10 minutes ago' +"%Y-%m-%dT%H:%M:%SZ")"
    },
    {
      "parameter": "pressure",
      "value": 1012.9,
      "unit": "hPa",
      "timestamp": "$(date -u -d '10 minutes ago' +"%Y-%m-%dT%H:%M:%SZ")"
    },
    {
      "parameter": "wind",
      "value": 4.8,
      "metadata": {
        "direction": 230
      },
      "unit": "m/s",
      "timestamp": "$(date -u -d '10 minutes ago' +"%Y-%m-%dT%H:%M:%SZ")"
    },
    {
      "parameter": "precipitation",
      "value": 0.2,
      "unit": "mm",
      "timestamp": "$(date -u -d '10 minutes ago' +"%Y-%m-%dT%H:%M:%SZ")"
    }
  ]
}
EOF

# Step 9: Submit batch data
echo "Submitting batch weather data..."
wrfcoin weather submit-batch \
  --file weather_batch.json \
  --validate \
  --source-address "$STATION_ADDRESS"

# Step 10: Create an NFT from the weather data
echo "Creating weather data NFT..."
wrfcoin weather create-nft \
  --title "NYC Weather Data Collection - $(date +"%Y-%m-%d")" \
  --description "Comprehensive weather dataset from Central Park NYC" \
  --station-id "DV-1234" \
  --time-range "$(date -u -d '1 hour ago' +"%Y-%m-%dT%H:%M:%SZ"),$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
  --parameters "temperature,humidity,pressure,wind,precipitation" \
  --creator-address "$STATION_ADDRESS"

# Step 11: Query historical data
echo "Querying historical weather data..."
wrfcoin weather query-history \
  --start-date "$(date -u -d '1 day ago' +"%Y-%m-%dT%H:%M:%SZ")" \
  --end-date "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
  --parameter temperature \
  --station-id "DV-1234" \
  --format json \
  --output "temperature_history.json"

# Step 12: Monitor weather data submissions in real-time
echo "Setting up monitoring for 30 seconds..."
timeout 30s wrfcoin monitor weather-data \
  --interval 5 \
  --region "40.7,-74.1,40.8,-73.9" \
  --data-type "all" \
  --format text

echo "Weather station integration demonstration complete!"