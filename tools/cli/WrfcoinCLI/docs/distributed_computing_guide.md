# Wrfcoin Distributed Computing Integration Guide

This guide explains how to integrate and utilize Wrfcoin's distributed computing capabilities
through the CLI, enabling you to leverage the network's computational resources for weather and
climate modeling tasks.

## Overview

Wrfcoin's distributed computing framework allows for:

- Submission of computational tasks to the network
- Integration with multiple compute providers (SONM, Golem, Bacalhau)
- Deployment of specialized weather and climate models
- Monetization of computational resources
- Cross-chain computational task execution

## Prerequisites

- Wrfcoin CLI installed
- A funded Wrfcoin wallet
- Basic understanding of weather/climate modeling
- (Optional) Access to compute provider accounts

## Supported Compute Providers

Wrfcoin integrates with multiple distributed compute networks:

| Provider       | Specialization             | Best For                           |
| -------------- | -------------------------- | ---------------------------------- |
| Native Wrfcoin | Weather data processing    | Small to medium weather model runs |
| Golem Network  | General computing          | CPU-intensive workloads            |
| SONM           | GPU acceleration           | Machine learning models            |
| Bacalhau       | Data-aware computing       | Large dataset processing           |
| Akash Network  | Cloud container deployment | Long-running services              |

## Basic Commands

### Configure Compute Node

To configure your machine as a compute node that processes tasks:

```bash
wrfcoin node configure-compute \
  --name "My Compute Node" \
  --computational-resources "cpu:8,gpu:1,ram:16gb,storage:1tb" \
  --supported-models "WRF,MPAS,GFS" \
  --reward-address "Wrf1AbCdEfGhIjKlMnOpQrStUvWxYz12345678"
```

### Submit Basic Computational Task

To submit a simple computational task to the network:

```bash
wrfcoin compute submit-task \
  --title "Temperature Trend Analysis" \
  --description "Analyze temperature trends in NYC for the past 5 years" \
  --input-data "data://wrfcoin.network/public/nyc_temp_5y.nc" \
  --model "linear-regression" \
  --budget 10 \
  --timeout 3600
```

### Check Task Status

To check the status of a submitted task:

```bash
wrfcoin compute task-status --id "task_12345"
```

### Retrieve Results

To retrieve the results of a completed task:

```bash
wrfcoin compute get-results \
  --id "task_12345" \
  --output-file "temperature_analysis_results.nc"
```

## Advanced Usage

### Weather Model Execution

To run a WRF (Weather Research and Forecasting) model on the network:

```bash
wrfcoin compute weather-model \
  --model "WRF-ARW" \
  --region "North America" \
  --resolution "0.25deg" \
  --forecast-hours 72 \
  --input-data "gfs_latest" \
  --physics-options "physics_config.json" \
  --budget 50 \
  --timeout 7200
```

### Ensemble Forecast Computation

To submit an ensemble forecast computation:

```bash
wrfcoin compute ensemble-forecast \
  --model "WRF-ARW" \
  --members 10 \
  --perturbation-method "stochastic_physics" \
  --region "North America" \
  --resolution "0.25deg" \
  --forecast-hours 72 \
  --budget 100 \
  --timeout 14400
```

### Data-aware Block Creation

To create a data-aware block storing computational results:

```bash
wrfcoin compute create-data-block \
  --title "NYC Temperature Forecast" \
  --data-file "nyc_forecast.nc" \
  --metadata-file "nyc_forecast_metadata.json" \
  --hash-algorithm "sha256" \
  --compression "lz4" \
  --storage-redundancy 3
```

## Integration with External Compute Providers

### Golem Network Integration

```bash
# Setup Golem integration
wrfcoin compute setup-golem \
  --node-address "0x1234567890abcdef1234567890abcdef12345678" \
  --api-key "YOUR_GOLEM_API_KEY"

# Submit task to Golem
wrfcoin compute golem-task \
  --spec-file "golem_task_spec.json" \
  --budget 20 \
  --timeout 3600
```

### SONM Integration

```bash
# Setup SONM integration
wrfcoin compute setup-sonm \
  --token-file "sonm_token.json" \
  --node-address "$SONM_NODE_ADDRESS"

# Submit task to SONM
wrfcoin compute sonm-task \
  --spec-file "sonm_task_spec.json" \
  --budget 15 \
  --timeout 7200
```

### Bacalhau Integration

```bash
# Setup Bacalhau integration
wrfcoin compute setup-bacalhau \
  --api-endpoint "https://dashboard.bacalhau.org/api" \
  --key-file "bacalhau_key.json"

# Submit task to Bacalhau
wrfcoin compute bacalhau-task \
  --spec-file "bacalhau_job.json" \
  --budget 25 \
  --timeout 3600
```

## Creating Computational NFTs

Computational NFTs encapsulate computational assets and algorithms that can be traded and executed
on the network:

```bash
# Create a computational NFT
wrfcoin nft create \
  --title "Global Climate Model v1" \
  --description "High-resolution global climate model" \
  --type computational \
  --file "climate_model_spec.json" \
  --license "CC-BY-4.0" \
  --execution-fee 5.0 \
  --creator-address "Wrf1AbCdEfGhIjKlMnOpQrStUvWxYz12345678"
```

## Setting Up Reward Distribution

Configure reward distribution for compute nodes:

```bash
# Setup reward distribution
wrfcoin node configure-compute-rewards \
  --reward-model "proportional" \
  --payment-interval "hourly" \
  --minimum-task-completion 5 \
  --quality-weighted true \
  --interledger-enabled true
```

## Monitoring Compute Network

Monitor the distributed compute network:

```bash
# Monitor computational tasks
wrfcoin monitor computational-tasks \
  --interval 10 \
  --task-type "all" \
  --format text

# Check compute node statistics
wrfcoin monitor compute-nodes \
  --interval 30 \
  --region "global" \
  --show-resources \
  --format text
```

## Example Workflows

### Climate Modeling Workflow

```bash
# 1. Upload input data
DATA_ID=$(wrfcoin data upload --file "climate_input_data.nc" --json | jq -r '.id')

# 2. Create a computational task
TASK_ID=$(wrfcoin compute submit-task \
  --title "Climate Model Run" \
  --input-data "$DATA_ID" \
  --model "CMIP6" \
  --parameters "climate_params.json" \
  --budget 75 \
  --timeout 36000 \
  --json | jq -r '.id')

# 3. Wait for completion
wrfcoin compute wait-for-completion --id "$TASK_ID"

# 4. Get results
wrfcoin compute get-results --id "$TASK_ID" --output-file "climate_results.nc"

# 5. Verify results
wrfcoin compute verify-results --id "$TASK_ID" --verification-method "statistical"

# 6. Create NFT from results
wrfcoin nft create \
  --title "Climate Model Results" \
  --description "Results from CMIP6 climate model run" \
  --type computational_result \
  --file "climate_results.nc" \
  --linked-task "$TASK_ID" \
  --creator-address "Wrf1AbCdEfGhIjKlMnOpQrStUvWxYz12345678"
```

### Weather Data Processing Pipeline

```bash
# 1. Collect weather data
wrfcoin weather collect-data \
  --region "North America" \
  --parameters "temperature,humidity,pressure,wind" \
  --time-period "24h" \
  --output-file "weather_data_24h.nc"

# 2. Process weather data
TASK_ID=$(wrfcoin compute submit-task \
  --title "Weather Data Processing" \
  --input-file "weather_data_24h.nc" \
  --algorithm "anomaly_detection" \
  --parameters "processing_params.json" \
  --budget 15 \
  --timeout 1800 \
  --json | jq -r '.id')

# 3. Wait for completion
wrfcoin compute wait-for-completion --id "$TASK_ID"

# 4. Get and analyze results
wrfcoin compute get-results --id "$TASK_ID" --output-file "weather_anomalies.nc"

# 5. Create visualization
wrfcoin compute visualize \
  --data-file "weather_anomalies.nc" \
  --type "heatmap" \
  --variables "temperature_anomaly" \
  --output-file "anomaly_visualization.png"
```

## Integration with Data-Aware Blocks

Wrfcoin uses data-aware blocks to store computational task information on the blockchain:

```bash
# Query data-aware blocks
wrfcoin chain query-data-blocks \
  --parameter "temperature" \
  --region "North America" \
  --time-period "24h" \
  --format json
```

## Best Practices

1. **Task Sizing**: Break large tasks into smaller units for better distribution
2. **Resource Specification**: Clearly specify resource requirements for optimal node matching
3. **Budget Setting**: Set appropriate budgets based on computational complexity
4. **Timeout Values**: Set realistic timeout values to avoid wasted resources
5. **Data Verification**: Always verify results, especially for important computations
6. **Storage Strategy**: Store large datasets off-chain with references on-chain
7. **Platform Selection**: Choose the right compute platform for specific task types

## Troubleshooting

### Common Issues

- **Task Stuck in Pending**: Check if your budget is sufficient for the task
- **Computation Failed**: Verify your input data format and model parameters
- **Poor Performance**: Consider increasing resource requirements
- **High Costs**: Optimize your task to use fewer resources or reduce runtime

### Debugging Commands

```bash
# Check task logs
wrfcoin compute get-logs --id "task_12345"

# Verify node status
wrfcoin node status --address "Wrf1AbCdEfGhIjKlMnOpQrStUvWxYz12345678"

# Test compute capability
wrfcoin compute test-capability --provider "sonm" --resource-level "medium"
```

## Security Considerations

1. **Input Validation**: Always validate input data before submission
2. **Result Verification**: Verify results against expected outputs
3. **Budget Limits**: Set maximum budget limits to prevent overspending
4. **Provider Reputation**: Use providers with high reputation scores
5. **Encryption**: Use encryption for sensitive computational data

## Further Reading

- [Wrfcoin Computational White Paper](https://docs.wrfcoin.network/computational.pdf)
- [SONM Integration Details](https://docs.wrfcoin.network/integration/sonm)
- [Golem Network Integration](https://docs.wrfcoin.network/integration/golem)
- [Advanced Weather Modeling Guide](https://docs.wrfcoin.network/weather/modeling)
