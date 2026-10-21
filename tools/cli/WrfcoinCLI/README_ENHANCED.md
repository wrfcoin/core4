# Wrfcoin Enhanced CLI

The Wrfcoin CLI has been enhanced with comprehensive support for all of Wrfcoin's advanced
integrations, including cross-chain capabilities, computational providers, IoT device management,
and data providers.

## New Integration Features

### Cross-Chain Integrations

- **Polkadot/Parity Integration**: Connect with the Polkadot ecosystem for cross-chain transfers and
  communication
- **Near Protocol Support**: Transfer assets and interact with the Near Protocol blockchain
- **Multi-Provider Bridge Management**: Configure and use multiple bridge providers (LayerZero,
  Axelar, Wormhole)
- **Cross-Chain Transaction Monitoring**: Track and verify cross-chain transactions

### Computational Providers

- **Golem Network Integration**: Submit computational tasks to the distributed Golem Network
- **Bacalhau Support**: Leverage Bacalhau's distributed computing capabilities
- **Task Management**: Monitor, retrieve results, and manage computational tasks
- **Provider Registration**: Register as a compute provider on the network

### IoT Device Management

- **FOAM Protocol Integration**: Verify location and register points of interest
- **Open Geospatial Consortium Support**: Interact with OGC-compatible services
- **Streamr Network Integration**: Create and manage data streams for IoT devices
- **Device Registration and Data Collection**: Register devices and collect/submit sensor data

### Data Providers

- **BigchainDB Integration**: Store, retrieve, and manage datasets using BigchainDB
- **Dataset Publishing**: Publish datasets to various data providers
- **Asset Management**: Create, transfer, and query data assets
- **Dataset Search**: Find relevant datasets across multiple providers

## Installation

Follow the standard installation instructions in the main README.md file.

## Usage Examples

### Cross-Chain Operations

```bash
# Setup Polkadot/Parity integration
wrfcoin crosschain setup-polkadot --node-url "wss://rpc.polkadot.io" --account "YOUR_POLKADOT_ACCOUNT"

# Transfer tokens to Near Protocol
wrfcoin crosschain transfer --to-chain "near" --to-address "recipient.testnet" --amount 10 --provider "axelar"

# List all configured bridges
wrfcoin crosschain list-bridges

# Check a cross-chain transaction
wrfcoin crosschain transaction-info --hash "0x1234567890abcdef"
```

### Computational Operations

```bash
# Setup Golem Network integration
wrfcoin compute setup-golem --node-address "YOUR_GOLEM_ADDRESS" --api-key "YOUR_API_KEY"

# Submit a weather forecast task
wrfcoin compute golem-task --spec-file "weather_forecast_spec.json" --budget 5

# Check task status
wrfcoin compute task-status --id "golem-task-123456"

# Get computation results
wrfcoin compute get-results --id "golem-task-123456" --output "results.json"
```

### IoT Operations

```bash
# Setup FOAM Protocol integration
wrfcoin iot setup-foam --endpoint "https://api.foam.space" --api-key "YOUR_FOAM_API_KEY"

# Register a weather station device
wrfcoin iot register-device --name "Weather Station Alpha" --device-type "weather" \
  --location "40.7128,-74.0060" \
  --sensors '{"temperature":true,"humidity":true,"pressure":true,"wind":true}' \
  --platform "foam"

# Query FOAM points of interest
wrfcoin iot foam-query --center "40.7128,-74.0060" --radius 5 --category "weather"

# Get data from a device
wrfcoin iot get-data --device-id "device-123" --start-time "2025-01-01T00:00:00Z"
```

### Data Provider Operations

```bash
# Setup BigchainDB integration
wrfcoin data setup-bigchaindb --url "https://test.bigchaindb.com" --api-key "YOUR_API_KEY"

# Search for datasets
wrfcoin data search-datasets --query "climate change" --category "climate"

# Create a BigchainDB asset
wrfcoin data bigchaindb-create-asset \
  --data '{"title":"Weather Dataset 2025","description":"Daily weather measurements"}' \
  --metadata '{"source":"NOAA","license":"CC-BY-4.0"}'

# Publish a dataset
wrfcoin data publish-dataset --data "climate_data.csv" --name "Climate Data 2025" \
  --description "Annual climate measurements" --provider "bigchaindb"
```

### Governance Operations

```bash
# List active proposals
wrfcoin governance list-proposals

# Create a new proposal
wrfcoin governance create-proposal --title "Enable FOAM Integration" \
  --description "Proposal to enable FOAM Protocol integration" \
  --proposal-type "parameter" --changes '{"foam_integration_enabled":true}'

# Vote on a proposal
wrfcoin governance vote --id 123 --vote "yes" \
  --justification "Enhances geospatial capabilities"
```

## Example Scripts

The `examples/` directory contains several reference scripts demonstrating how to use the enhanced
CLI:

- `examples/integration_operations.sh`: Comprehensive examples of all integration features
- `examples/cross_chain_operations.sh`: Detailed cross-chain transfer examples
- `examples/computational_tasks.sh`: Examples for running computational tasks
- `examples/iot_management.sh`: IoT device management examples
- `examples/data_management.sh`: Data provider integration examples

## Advanced Configuration

For advanced configuration options, see the configuration documentation in
`docs/advanced_configuration.md`.

## Security Considerations

- API keys and credentials should be stored securely
- Use environment variables for sensitive information
- Review governance proposals carefully before voting
- Verify cross-chain destinations before transferring assets

## Troubleshooting

If you encounter issues with any of the integrations:

1. Ensure you have the latest CLI version
2. Verify your network connection
3. Check your API keys and credentials
4. Review the logs with `wrfcoin dev monitor-performance`

For detailed troubleshooting, run with debug output:

```bash
WRFCOIN_LOG_LEVEL=debug wrfcoin <command>
```

## Integration-Specific Notes

### Polkadot/Parity

- Requires a funded Polkadot account
- Supports both Polkadot and Kusama networks

### Golem Network

- Tasks are priced in GLM tokens
- GPU-accelerated tasks require higher budget allocations

### FOAM Protocol

- Location assertions require stakes for validation
- Designed for geospatial data verification

### BigchainDB

- Assets are immutable once created
- Supports complex metadata structures

## Contributing

Contributions to enhance these integrations are welcome. Please see `CONTRIBUTING.md` for details.
