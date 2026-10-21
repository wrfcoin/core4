#!/bin/bash
# Example: Cortex Foundation zkSNARK Integration for Private Weather Data
# This script demonstrates how to use Cortex Foundation's zkSNARK implementation
# for private and verifiable cross-chain weather data transfers

# Step 1: Setup test environment
echo "Setting up test environment..."

# Wrfcoin wallet
WRFCOIN_WALLET=$(wrfcoin wallet new-address --name "ZK Privacy Tester" --json | jq -r '.address')
echo "Wrfcoin wallet address: $WRFCOIN_WALLET"

# Initialize cross-chain configuration
echo "Initializing cross-chain configuration..."
wrfcoin crosschain init \
  --default-wallet "$WRFCOIN_WALLET" \
  --networks "ethereum,optimism,base,solana,cosmos,polkadot,near" \
  --config-file "zk_privacy_config.json"

# Step 2: Create an example weather data set
echo "Creating example weather data set..."

mkdir -p test_data

cat > test_data/sensitive_weather_data.json << 'EOF'
{
  "metadata": {
    "description": "Sensitive weather station data with precise location",
    "source": "Proprietary Weather Station Network",
    "created": "2025-05-17T00:00:00Z",
    "units": "celsius",
    "sensitivity": "high",
    "owner": "Weather Research Institute"
  },
  "stations": [
    {
      "id": "WS-0001",
      "exact_location": {
        "latitude": 40.7128,
        "longitude": -74.0060,
        "elevation": 10.5
      },
      "readings": [
        {
          "timestamp": "2025-05-17T00:00:00Z",
          "temperature": 22.5,
          "humidity": 65.3,
          "pressure": 1013.2,
          "wind_speed": 8.7,
          "wind_direction": 270.0,
          "precipitation": 0.0
        },
        {
          "timestamp": "2025-05-17T01:00:00Z",
          "temperature": 21.8,
          "humidity": 67.1,
          "pressure": 1012.9,
          "wind_speed": 7.5,
          "wind_direction": 265.5,
          "precipitation": 0.2
        }
      ]
    },
    {
      "id": "WS-0002",
      "exact_location": {
        "latitude": 34.0522,
        "longitude": -118.2437,
        "elevation": 93.0
      },
      "readings": [
        {
          "timestamp": "2025-05-17T00:00:00Z",
          "temperature": 25.7,
          "humidity": 52.1,
          "pressure": 1011.5,
          "wind_speed": 3.4,
          "wind_direction": 180.0,
          "precipitation": 0.0
        },
        {
          "timestamp": "2025-05-17T01:00:00Z",
          "temperature": 24.9,
          "humidity": 53.2,
          "pressure": 1011.2,
          "wind_speed": 4.1,
          "wind_direction": 175.5,
          "precipitation": 0.0
        }
      ]
    }
  ]
}
EOF

# Step 3: Install Cortex zkSNARK library
echo "Setting up Cortex zkSNARK library..."

# Initialize zkSNARK configuration
wrfcoin privacy init-zk \
  --provider "cortex" \
  --proving-scheme "groth16" \
  --curve "bn254" \
  --config-file "zk_config.json"

# Step 4: Generate zkSNARK circuit for weather data privacy
echo "Generating zkSNARK circuit for weather data privacy..."

cat > weather_data_circuit.circom << 'EOF'
pragma circom 2.0.0;

include "node_modules/circomlib/circuits/comparators.circom";
include "node_modules/circomlib/circuits/poseidon.circom";

// Circuit for proving temperature range without revealing exact value
template TemperatureRangeProof() {
    signal input temperature;
    signal input min_range;
    signal input max_range;
    signal output in_range;
    
    // Check if temperature is within range
    component gtMin = GreaterThan(32);
    component ltMax = LessThan(32);
    
    gtMin.in[0] <== temperature;
    gtMin.in[1] <== min_range;
    
    ltMax.in[0] <== temperature;
    ltMax.in[1] <== max_range;
    
    in_range <== gtMin.out * ltMax.out;
}

// Circuit for proving location is within a region without revealing exact location
template LocationRegionProof() {
    signal input latitude;
    signal input longitude;
    signal input region_center_lat;
    signal input region_center_lon;
    signal input max_distance;
    signal output in_region;
    
    // Calculate squared distance (simplified for demonstration)
    signal lat_diff <== latitude - region_center_lat;
    signal lon_diff <== longitude - region_center_lon;
    signal lat_diff_squared <== lat_diff * lat_diff;
    signal lon_diff_squared <== lon_diff * lon_diff;
    signal distance_squared <== lat_diff_squared + lon_diff_squared;
    
    // Check if distance squared is less than max_distance squared
    component lt = LessThan(64);
    lt.in[0] <== distance_squared;
    lt.in[1] <== max_distance * max_distance;
    
    in_region <== lt.out;
}

// Main circuit for proving weather data properties
template WeatherDataPrivacy() {
    // Private station location
    signal private input latitude;
    signal private input longitude;
    
    // Private weather readings
    signal private input temperature;
    signal private input humidity;
    signal private input pressure;
    
    // Public region parameters
    signal input region_center_lat;
    signal input region_center_lon;
    signal input max_distance;
    
    // Public temperature range parameters
    signal input min_temperature;
    signal input max_temperature;
    
    // Hash of the full data set (for integrity verification)
    signal private input data_hash;
    signal input expected_data_hash;
    
    // Outputs - public proofs
    signal output location_verified;
    signal output temperature_in_range;
    signal output data_integrity_verified;
    
    // Verify location is in allowed region
    component locationProof = LocationRegionProof();
    locationProof.latitude <== latitude;
    locationProof.longitude <== longitude;
    locationProof.region_center_lat <== region_center_lat;
    locationProof.region_center_lon <== region_center_lon;
    locationProof.max_distance <== max_distance;
    location_verified <== locationProof.in_region;
    
    // Verify temperature is in expected range
    component tempProof = TemperatureRangeProof();
    tempProof.temperature <== temperature;
    tempProof.min_range <== min_temperature;
    tempProof.max_range <== max_temperature;
    temperature_in_range <== tempProof.in_range;
    
    // Verify data integrity (hash matches)
    data_integrity_verified <== (data_hash == expected_data_hash) ? 1 : 0;
    
    // Additional verification: combine with hashed private values
    component hashFunc = Poseidon(5);
    hashFunc.inputs[0] <== latitude;
    hashFunc.inputs[1] <== longitude;
    hashFunc.inputs[2] <== temperature;
    hashFunc.inputs[3] <== humidity;
    hashFunc.inputs[4] <== pressure;
}

component main {public [region_center_lat, region_center_lon, max_distance, min_temperature, max_temperature, expected_data_hash]} = WeatherDataPrivacy();
EOF

# Compile circuit
wrfcoin privacy compile-circuit \
  --circuit-file weather_data_circuit.circom \
  --output-dir zk_build

# Setup zkSNARK keys
wrfcoin privacy setup-keys \
  --circuit-dir zk_build \
  --verification-key-output "weather_vkey.json"

# Step 5: Create a private data submission with zkSNARK proof
echo "Creating private data submission with zkSNARK proof..."

# Extract and hash the original data
WEATHER_DATA_HASH=$(wrfcoin data hash-file \
  --file "test_data/sensitive_weather_data.json" \
  --algorithm "poseidon" \
  --field "bn254" \
  --json | jq -r '.hash')

echo "Original weather data hash: $WEATHER_DATA_HASH"

# Generate witness for zkSNARK proof
wrfcoin privacy generate-witness \
  --circuit-dir zk_build \
  --input-json "test_data/sensitive_weather_data.json" \
  --extract-inputs "stations[0].exact_location.latitude, stations[0].exact_location.longitude, stations[0].readings[0].temperature, stations[0].readings[0].humidity, stations[0].readings[0].pressure" \
  --public-inputs "40.5, -74.5, 0.5, 0, 50, $WEATHER_DATA_HASH" \
  --witness-output "weather_witness.json"

# Generate zkSNARK proof
ZK_PROOF=$(wrfcoin privacy generate-proof \
  --witness-file "weather_witness.json" \
  --proving-key "zk_build/proving_key.json" \
  --json | jq -r '.proof')

echo "Generated zkSNARK proof"

# Step 6: Create a privacy-preserving data submission
echo "Creating privacy-preserving data submission..."

# Create a shielded version of the data with only non-sensitive information
cat > test_data/public_weather_data.json << EOF
{
  "metadata": {
    "description": "Anonymized weather data with privacy-preserving proofs",
    "source": "Proprietary Weather Station Network",
    "created": "2025-05-17T00:00:00Z",
    "units": "celsius",
    "privacy_proof": "$ZK_PROOF",
    "data_hash": "$WEATHER_DATA_HASH"
  },
  "region_data": [
    {
      "region_name": "Northeast US",
      "region_center": {
        "latitude": 40.5,
        "longitude": -74.5
      },
      "radius_km": 0.5,
      "summary": {
        "temperature_range": {
          "min": 0.0,
          "max": 50.0,
          "in_range_verified": true
        },
        "timestamp": "2025-05-17T00:00:00Z",
        "data_integrity_verified": true
      }
    }
  ]
}
EOF

# Step 7: Submit the privacy-preserved data to blockchain
echo "Submitting privacy-preserved data to blockchain..."

# Submit public data and ZK proof to Wrfcoin
SUBMISSION_ID=$(wrfcoin weather submit-private-data \
  --public-file "test_data/public_weather_data.json" \
  --zk-proof "$ZK_PROOF" \
  --verification-key "weather_vkey.json" \
  --source-address "$WRFCOIN_WALLET" \
  --metadata-tags "privacy=zk-snark,provider=cortex" \
  --json | jq -r '.id')

echo "Submitted private weather data with ID: $SUBMISSION_ID"

# Step 8: Cross-chain verification of private data
echo "Testing cross-chain verification of private data..."

# Function to test verification on different chains
test_verification() {
  local chain=$1
  local provider=$2
  
  echo "Testing verification on $chain via $provider..."
  
  VERIFY_RESULT=$(wrfcoin crosschain verify-zk-proof \
    --submission-id "$SUBMISSION_ID" \
    --target-chain $chain \
    --provider $provider \
    --json)
  
  VERIFIED=$(echo $VERIFY_RESULT | jq -r '.verified')
  
  if [ "$VERIFIED" == "true" ]; then
    echo "  ✅ Verification successful on $chain"
  else
    echo "  ❌ Verification failed on $chain: $(echo $VERIFY_RESULT | jq -r '.reason')"
  fi
}

# Test verification on multiple chains
test_verification "ethereum" "layerzero"
test_verification "optimism" "optimism-bridge"
test_verification "solana" "wormhole"

# Step 9: Cross-chain data attestation
echo "Testing cross-chain data attestation..."

# Create attestation from multiple chains
ATTESTATION=$(wrfcoin crosschain create-attestation \
  --submission-id "$SUBMISSION_ID" \
  --chains "ethereum,optimism,solana" \
  --threshold 2 \
  --json | jq -r '.attestation_id')

echo "Created multi-chain attestation with ID: $ATTESTATION"

# Step 10: Test data usage without revealing sensitive details
echo "Testing data usage without revealing sensitive details..."

# Create a computational task that uses private data
TASK_ID=$(wrfcoin compute submit-privacy-preserving-task \
  --title "Regional Temperature Analysis" \
  --description "Analyze temperature patterns in a region without accessing exact location data" \
  --input-data "$SUBMISSION_ID" \
  --attestation "$ATTESTATION" \
  --algorithm "regional-aggregate" \
  --parameters "task_params.json" \
  --result-access "aggregated-only" \
  --preserve-privacy \
  --budget 15 \
  --json | jq -r '.id')

echo "Submitted privacy-preserving computational task with ID: $TASK_ID"

# Step 11: Generate report on privacy guarantees
echo "Generating privacy guarantee report..."

wrfcoin privacy generate-report \
  --submission-id "$SUBMISSION_ID" \
  --task-id "$TASK_ID" \
  --attestation-id "$ATTESTATION" \
  --include-zk-circuits \
  --include-cortex-verification \
  --output-file "privacy_report.html"

echo "Privacy testing completed!"
echo "Report available at: privacy_report.html"