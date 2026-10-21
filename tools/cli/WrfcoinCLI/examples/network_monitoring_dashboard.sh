#!/bin/bash
# Example: Network Monitoring Dashboard with Wrfcoin CLI
# This script demonstrates how to use the monitoring commands to create a dashboard view

# Step 1: Check for required tools
if ! command -v jq &> /dev/null; then
    echo "Error: jq is required but not installed. Please install jq first."
    exit 1
fi

if ! command -v watch &> /dev/null; then
    echo "Warning: 'watch' command not found. Real-time updates may not work properly."
fi

# Step 2: Create a directory for monitoring outputs
echo "Setting up monitoring environment..."
mkdir -p ./monitoring/logs
mkdir -p ./monitoring/exports

# Step 3: Define helper functions
display_header() {
    clear
    echo "=========================================================="
    echo "               WRFCOIN NETWORK DASHBOARD                  "
    echo "=========================================================="
    echo "Current time: $(date)"
    echo "Network: TESTNET"
    echo "=========================================================="
}

# Step 4: Initialize base monitoring data files
wrfcoin monitor status --detailed --format json > ./monitoring/logs/network_status.json
wrfcoin monitor validators --interval 1 --limit 1 --show-slashing --show-rewards --format json > ./monitoring/logs/validators.json
wrfcoin monitor nodes --interval 1 --limit 1 --format json > ./monitoring/logs/nodes.json
wrfcoin monitor blockchain --interval 1 --limit 1 --show-blocks --show-transactions --format json > ./monitoring/logs/blockchain.json
wrfcoin monitor weather-data --interval 1 --limit 1 --format json > ./monitoring/logs/weather_data.json
wrfcoin monitor computational-tasks --interval 1 --limit 1 --format json > ./monitoring/logs/computational_tasks.json

# Step 5: Function to update and display dashboard
update_dashboard() {
    display_header

    # Update data in background
    wrfcoin monitor status --detailed --format json > ./monitoring/logs/network_status.json &
    wrfcoin monitor validators --interval 1 --limit 1 --show-slashing --show-rewards --format json > ./monitoring/logs/validators.json &
    wrfcoin monitor nodes --interval 1 --limit 1 --format json > ./monitoring/logs/nodes.json &
    wrfcoin monitor blockchain --interval 1 --limit 1 --show-blocks --show-transactions --format json > ./monitoring/logs/blockchain.json &
    wrfcoin monitor weather-data --interval 1 --limit 1 --format json > ./monitoring/logs/weather_data.json &
    wrfcoin monitor computational-tasks --interval 1 --limit 1 --format json > ./monitoring/logs/computational_tasks.json &
    
    # Wait for background jobs to finish
    wait

    # Display blockchain status
    echo ""
    echo "BLOCKCHAIN STATUS"
    echo "----------------"
    BLOCK_HEIGHT=$(jq -r '.blockHeight | select(. != null) // "N/A"' ./monitoring/logs/blockchain.json)
    PENDING_TX=$(jq -r '.pendingTransactions | select(. != null) // "N/A"' ./monitoring/logs/blockchain.json)
    HASHRATE=$(jq -r '.hashrate | select(. != null) // "N/A"' ./monitoring/logs/blockchain.json)
    DIFFICULTY=$(jq -r '.difficulty | select(. != null) // "N/A"' ./monitoring/logs/blockchain.json)
    
    echo "Block Height: $BLOCK_HEIGHT"
    echo "Pending Transactions: $PENDING_TX"
    echo "Hashrate: $HASHRATE"
    echo "Difficulty: $DIFFICULTY"

    # Display latest blocks
    LATEST_BLOCKS=$(jq -r '.latestBlocks | select(. != null) // []' ./monitoring/logs/blockchain.json)
    if [ "$LATEST_BLOCKS" != "[]" ]; then
        echo ""
        echo "LATEST BLOCKS"
        echo "------------"
        jq -r '.latestBlocks | select(. != null) // [] | .[:3] | .[] | "Block \(.height): \(.transactions) txs, \(.timeAgo) ago"' ./monitoring/logs/blockchain.json
    fi

    # Display node status
    echo ""
    echo "NODE STATUS"
    echo "----------"
    ACTIVE_NODES=$(jq -r '.activeNodes | select(. != null) // "N/A"' ./monitoring/logs/nodes.json)
    VALIDATOR_NODES=$(jq -r '.validatorNodes | select(. != null) // "N/A"' ./monitoring/logs/nodes.json)
    COMPUTE_NODES=$(jq -r '.computeNodes | select(. != null) // "N/A"' ./monitoring/logs/nodes.json)
    STORAGE_NODES=$(jq -r '.storageNodes | select(. != null) // "N/A"' ./monitoring/logs/nodes.json)
    DATA_NODES=$(jq -r '.dataCollectionNodes | select(. != null) // "N/A"' ./monitoring/logs/nodes.json)
    
    echo "Active Nodes: $ACTIVE_NODES"
    echo "Validator Nodes: $VALIDATOR_NODES"
    echo "Compute Nodes: $COMPUTE_NODES"
    echo "Storage Nodes: $STORAGE_NODES"
    echo "Data Collection Nodes: $DATA_NODES"

    # Display validator status
    echo ""
    echo "VALIDATOR STATUS"
    echo "---------------"
    ACTIVE_VALIDATORS=$(jq -r '.activeValidators | select(. != null) // "N/A"' ./monitoring/logs/validators.json)
    TOTAL_STAKE=$(jq -r '.totalStake | select(. != null) // "N/A"' ./monitoring/logs/validators.json)
    LATEST_PROPOSER=$(jq -r '.latestProposer | select(. != null) // "N/A"' ./monitoring/logs/validators.json)
    
    echo "Active Validators: $ACTIVE_VALIDATORS"
    echo "Total Stake: $TOTAL_STAKE WRF"
    echo "Latest Proposer: $LATEST_PROPOSER"

    # Display slashing events if any
    SLASHING_EVENTS=$(jq -r '.recentSlashingEvents | select(. != null) // []' ./monitoring/logs/validators.json)
    if [ "$SLASHING_EVENTS" != "[]" ]; then
        echo ""
        echo "RECENT SLASHING EVENTS"
        echo "---------------------"
        jq -r '.recentSlashingEvents | select(. != null) // [] | .[] | "\(.validator): \(.amount) WRF (\(.reason))"' ./monitoring/logs/validators.json
    fi

    # Display weather data submissions
    echo ""
    echo "WEATHER DATA SUBMISSIONS"
    echo "----------------------"
    SUBMISSIONS=$(jq -r '.submissionsLastMinute | select(. != null) // "N/A"' ./monitoring/logs/weather_data.json)
    TEMP_READINGS=$(jq -r '.temperatureReadings | select(. != null) // "N/A"' ./monitoring/logs/weather_data.json)
    HUMIDITY_READINGS=$(jq -r '.humidityReadings | select(. != null) // "N/A"' ./monitoring/logs/weather_data.json)
    PRESSURE_READINGS=$(jq -r '.pressureReadings | select(. != null) // "N/A"' ./monitoring/logs/weather_data.json)
    WIND_READINGS=$(jq -r '.windReadings | select(. != null) // "N/A"' ./monitoring/logs/weather_data.json)
    
    echo "Submissions (last minute): $SUBMISSIONS"
    echo "Temperature Readings: $TEMP_READINGS"
    echo "Humidity Readings: $HUMIDITY_READINGS"
    echo "Pressure Readings: $PRESSURE_READINGS"
    echo "Wind Readings: $WIND_READINGS"

    # Display computational tasks
    echo ""
    echo "COMPUTATIONAL TASKS"
    echo "-----------------"
    PENDING_TASKS=$(jq -r '.pending | select(. != null) // "N/A"' ./monitoring/logs/computational_tasks.json)
    RUNNING_TASKS=$(jq -r '.running | select(. != null) // "N/A"' ./monitoring/logs/computational_tasks.json)
    COMPLETED_TASKS=$(jq -r '.completed | select(. != null) // "N/A"' ./monitoring/logs/computational_tasks.json)
    FAILED_TASKS=$(jq -r '.failed | select(. != null) // "N/A"' ./monitoring/logs/computational_tasks.json)
    
    echo "Pending Tasks: $PENDING_TASKS"
    echo "Running Tasks: $RUNNING_TASKS"
    echo "Completed Tasks: $COMPLETED_TASKS"
    echo "Failed Tasks: $FAILED_TASKS"

    # Display recent task completions if any
    RECENT_TASKS=$(jq -r '.recentTaskCompletions | select(. != null) // []' ./monitoring/logs/computational_tasks.json)
    if [ "$RECENT_TASKS" != "[]" ]; then
        echo ""
        echo "RECENT TASK COMPLETIONS"
        echo "---------------------"
        jq -r '.recentTaskCompletions | select(. != null) // [] | .[:2] | .[] | "\(.id): \(.type), completed \(.timeAgo) ago"' ./monitoring/logs/computational_tasks.json
    fi

    echo ""
    echo "=========================================================="
    echo "Press Ctrl+C to exit"
    echo "=========================================================="
}

# Step 6: Set up continuous monitoring for dashboard
echo "Starting monitoring dashboard..."
echo "Press Ctrl+C to exit"
sleep 2

# Check if 'watch' is available for real-time updates
if command -v watch &> /dev/null; then
    # Using watch for continuous updates
    watch -n 5 "bash -c 'source $0; update_dashboard'"
else
    # Fallback to manual loop
    while true; do
        update_dashboard
        sleep 5
    done
fi

# Step 7: Export monitoring data (this would execute after Ctrl+C in a real scenario)
echo "Exporting monitoring data..."

# Export blockchain data
wrfcoin monitor export blockchain \
  --output ./monitoring/exports/blockchain_$(date +"%Y%m%d_%H%M%S").json \
  --duration 3600 \
  --interval 60 \
  --format json &

# Export node data
wrfcoin monitor export nodes \
  --output ./monitoring/exports/nodes_$(date +"%Y%m%d_%H%M%S").json \
  --duration 3600 \
  --interval 60 \
  --format json &

# Export weather data
wrfcoin monitor export weather-data \
  --output ./monitoring/exports/weather_data_$(date +"%Y%m%d_%H%M%S").json \
  --duration 3600 \
  --interval 60 \
  --format json &

# Export computational tasks data
wrfcoin monitor export computational-tasks \
  --output ./monitoring/exports/computational_tasks_$(date +"%Y%m%d_%H%M%S").json \
  --duration 3600 \
  --interval 60 \
  --format json &

# Wait for exports to complete
echo "Waiting for exports to complete..."
wait

echo "Monitoring dashboard demonstration complete!"
echo "Monitoring data exported to ./monitoring/exports/"