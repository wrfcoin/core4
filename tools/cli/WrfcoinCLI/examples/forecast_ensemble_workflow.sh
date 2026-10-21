#!/bin/bash
# Example: Forecast Ensemble Workflow with Wrfcoin CLI
# This script demonstrates how to create and publish weather forecast ensembles

# Step 1: Create a dedicated wallet for the forecast provider
echo "Creating wallet for forecast provider..."
PROVIDER_ADDRESS=$(wrfcoin wallet new-address --name "Global Weather Forecaster" --json | jq -r '.address')
echo "Forecast provider wallet address: $PROVIDER_ADDRESS"

# Step 2: Set up directory structure for ensemble members
echo "Creating directory structure for ensemble forecast..."
mkdir -p ./ensemble/members

# Step 3: Generate sample ensemble member files (in a real scenario, these would be real forecast outputs)
for i in {1..5}; do
  cat > ./ensemble/members/member_$i.json << EOF
{
  "model": "WRF-ARW",
  "member_id": "$i",
  "version": "4.3.1",
  "initialization_time": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "forecast_hours": 72,
  "grid": {
    "resolution": 0.25,
    "projection": "lambert_conformal",
    "center_lat": 40.0,
    "center_lon": -98.0,
    "nx": 400,
    "ny": 300
  },
  "parameters": [
    "temperature_2m",
    "relative_humidity_2m",
    "wind_speed_10m",
    "wind_direction_10m",
    "precipitation_rate",
    "total_precipitation",
    "pressure_msl"
  ],
  "perturbation": {
    "method": "stochastic_physics",
    "magnitude": $(echo "0.1 + $i * 0.05" | bc)
  },
  "data": [
    {
      "time": "$(date -u -d "+6 hours" +"%Y-%m-%dT%H:%M:%SZ")",
      "values": {
        "temperature_2m": {
          "mean": $(echo "22.5 + $i * 0.2 - 0.5" | bc),
          "min": $(echo "21.5 + $i * 0.1 - 0.5" | bc),
          "max": $(echo "23.5 + $i * 0.3 - 0.5" | bc)
        },
        "precipitation_rate": $(echo "0.5 + $i * 0.2" | bc),
        "total_precipitation": $(echo "2.0 + $i * 0.5" | bc)
      }
    },
    {
      "time": "$(date -u -d "+12 hours" +"%Y-%m-%dT%H:%M:%SZ")",
      "values": {
        "temperature_2m": {
          "mean": $(echo "24.0 + $i * 0.3 - 0.6" | bc),
          "min": $(echo "22.8 + $i * 0.2 - 0.6" | bc),
          "max": $(echo "25.2 + $i * 0.4 - 0.6" | bc)
        },
        "precipitation_rate": $(echo "0.7 + $i * 0.3" | bc),
        "total_precipitation": $(echo "5.0 + $i * 0.8" | bc)
      }
    }
  ]
}
EOF
done

# Step 4: Create ensemble metadata file
cat > ./ensemble/metadata.json << EOF
{
  "title": "North America 72-hour Forecast Ensemble",
  "description": "5-member ensemble forecast for North America using WRF-ARW model with stochastic physics perturbations",
  "forecast_type": "weather",
  "region": "North America",
  "bbox": [20.0, -130.0, 55.0, -65.0],
  "initialization_time": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "lead_time_hours": 72,
  "member_count": 5,
  "uncertainty_quantification": "stochastic_physics",
  "resolution": "0.25deg",
  "version": "1.0",
  "creator": "$PROVIDER_ADDRESS",
  "license": "CC-BY-4.0",
  "tags": ["WRF", "ensemble", "stochastic_physics", "north_america", "operational"]
}
EOF

# Step 5: Publish the ensemble forecast
echo "Publishing ensemble forecast to the blockchain..."
FORECAST_ID=$(wrfcoin forecast publish-ensemble \
  --title "North America 72-hour Forecast Ensemble" \
  --forecast-type weather \
  --region "North America" \
  --ensemble-dir ./ensemble \
  --metadata-file ./ensemble/metadata.json \
  --creator-address "$PROVIDER_ADDRESS" \
  --store-data-on-chain \
  --json | jq -r '.id')

echo "Ensemble forecast published with ID: $FORECAST_ID"

# Step 6: Display the forecast details
echo "Retrieving forecast details..."
wrfcoin forecast show "$FORECAST_ID" --show-members --format text

# Step 7: Wait for 10 seconds (in a real scenario, this would be hours or days later)
echo "Waiting for 10 seconds to simulate the passage of time..."
sleep 10

# Step 8: Generate sample observational data file for validation
cat > ./observations.json << EOF
{
  "source": "NOAA",
  "type": "weather_station_network",
  "collection_time": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "region": "North America",
  "bbox": [20.0, -130.0, 55.0, -65.0],
  "station_count": 250,
  "parameters": ["temperature_2m", "precipitation_rate", "total_precipitation"],
  "data": [
    {
      "time": "$(date -u -d "+6 hours" +"%Y-%m-%dT%H:%M:%SZ")",
      "values": {
        "temperature_2m": {
          "mean": 22.7,
          "min": 21.3,
          "max": 24.1,
          "stations": 250
        },
        "precipitation_rate": 0.6,
        "total_precipitation": 2.3
      }
    },
    {
      "time": "$(date -u -d "+12 hours" +"%Y-%m-%dT%H:%M:%SZ")",
      "values": {
        "temperature_2m": {
          "mean": 24.3,
          "min": 22.9,
          "max": 25.7,
          "stations": 250
        },
        "precipitation_rate": 0.8,
        "total_precipitation": 5.5
      }
    }
  ]
}
EOF

# Step 9: Validate the forecast against observations
echo "Validating forecast against observations..."
wrfcoin forecast validate "$FORECAST_ID" \
  --observed-data ./observations.json \
  --method "rmse" \
  --parameters "temperature_2m,precipitation_rate,total_precipitation" \
  --output validation_results.json

# Step 10: Create a new forecast based on ensemble results and observations
echo "Creating improved forecast based on ensemble results and observations..."
wrfcoin forecast publish \
  --title "Improved North America 24-hour Forecast" \
  --forecast-type weather \
  --region "North America" \
  --based-on "$FORECAST_ID" \
  --data-file ./ensemble/members/member_3.json \
  --creator-address "$PROVIDER_ADDRESS" \
  --store-data-on-chain

# Step 11: Monitor forecast submissions in real-time
echo "Setting up monitoring for 30 seconds..."
timeout 30s wrfcoin monitor forecast-submissions \
  --interval 5 \
  --region "North America" \
  --forecast-type weather \
  --format text

echo "Forecast ensemble workflow demonstration complete!"