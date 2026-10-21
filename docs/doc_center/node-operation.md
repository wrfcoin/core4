# Node Operation Guide

**Date Created:** May 20, 2025  
**Last Edited:** May 26, 2025

This comprehensive guide provides instructions for setting up and operating a Wrfcoin node,
including requirements, installation steps, configuration options, and troubleshooting advice.

## Requirements

### Hardware Requirements

To run a Wrfcoin node efficiently, your system should meet these minimum requirements:

- **CPU**: 4 cores (8 cores recommended for validator nodes)
- **RAM**: 8GB (16GB recommended for validator nodes)
- **Storage**: 500GB SSD (1TB+ recommended for archive nodes)
- **Network**: Stable broadband connection with minimum 10Mbps upload/download
- **Power**: Uninterrupted power supply recommended

### Software Requirements

- **Operating System**: Ubuntu 22.04 LTS, Debian 11, or newer (Linux recommended)
- **Dependencies**:
  - Rust 1.65+
  - NodeJS 18+
  - Docker (optional, for containerized deployment)

## Installation

### Installing from Binary Release

The simplest way to install a Wrfcoin node is through our pre-built binaries:

```bash
# Download the latest release
curl -L -o wrfcoin-node.tar.gz https://releases.wrfcoin.org/latest/wrfcoin-node.tar.gz

# Extract the files
tar -xzf wrfcoin-node.tar.gz

# Move to a suitable location
sudo mv wrfcoin-node /usr/local/bin/

# Verify the installation
wrfcoin-node --version
```

### Building from Source

For those who prefer to build from source:

```bash
# Clone the repository
git clone https://github.com/wrfcoin/wrfcoin-core.git
cd wrfcoin-core

# Install dependencies
sudo apt update
sudo apt install -y build-essential libssl-dev pkg-config

# Build the node software
cargo build --release

# Copy the binary to a suitable location
sudo cp target/release/wrfcoin-node /usr/local/bin/
```

### Docker Installation

For a containerized deployment:

```bash
# Pull the official Docker image
docker pull wrfcoin/node:latest

# Create a configuration directory
mkdir -p ~/.wrfcoin/config

# Run the container
docker run -d --name wrfcoin-node \
  -v ~/.wrfcoin:/data \
  -p 9944:9944 -p 9933:9933 \
  wrfcoin/node:latest
```

## Configuration

### Basic Configuration

The basic configuration is defined in the `config.yaml` file:

```yaml
# Node identity
node_name: 'MyWrfcoinNode'
node_type: 'full' # Options: full, validator, api, computational, light

# Network settings
network: 'testnet' # Options: mainnet, testnet
p2p_port: 9944
rpc_port: 9933

# Storage settings
data_dir: '/data'
storage_engine: 'rocksdb'

# Logging
log_level: 'info' # Options: debug, info, warn, error
```

### Validator Configuration

If you're running a validator node, additional settings are required:

```yaml
# Validator specific settings
validator:
  stake_amount: 10000 # Amount of WRF tokens staked
  commission_rate: 0.05 # 5% commission
  public_key: '0x...' # Your validator public key

  # For PoLocation validators
  location:
    latitude: 45.123
    longitude: -122.456
    altitude: 150
    accuracy: 10
    proof_method: 'gnss+attestation'
```

### Performance Tuning

For optimal performance:

```yaml
# Performance settings
performance:
  max_peers: 50
  block_production_threads: 4
  transaction_pool_size: 8192
  state_cache_size_mb: 1024
  block_cache_size_mb: 512
```

## Node Operations

### Starting the Node

To start your Wrfcoin node:

```bash
# Start with default config
wrfcoin-node

# Start with custom config file
wrfcoin-node --config-path /path/to/config.yaml

# Start in specific network mode
wrfcoin-node --network testnet
```

### Monitoring

Monitor your node's status:

```bash
# Check node status
wrfcoin-node status

# View logs
tail -f ~/.wrfcoin/logs/node.log

# Monitor performance metrics
wrfcoin-node metrics
```

### Backup and Recovery

Regular backups are essential:

```bash
# Backup your node data
wrfcoin-node backup --output /backup/wrfnode-backup.zip

# Restore from backup
wrfcoin-node restore --input /backup/wrfnode-backup.zip
```

## Troubleshooting

### Common Issues

Here are solutions to common problems:

#### Node Won't Start

If your node fails to start:

1. Check logs for specific errors: `cat ~/.wrfcoin/logs/node.log`
2. Verify network connectivity: `ping bootstrap.wrfcoin.org`
3. Ensure sufficient disk space: `df -h`
4. Check for port conflicts: `netstat -tuln | grep 9944`

#### Synchronization Issues

If your node isn't synchronizing:

1. Check your internet connection
2. Verify firewall settings allow required ports
3. Try connecting to specific peers: `wrfcoin-node --seed-nodes 1.2.3.4:9944,5.6.7.8:9944`
4. Consider a fresh resync if database may be corrupted

#### Validator Problems

For validator-specific issues:

1. Ensure you have sufficient stake
2. Verify your location proofs are valid
3. Check for slashing conditions
4. Make sure your system time is accurate (run NTP)

### Getting Support

If you need additional help:

- Join our Discord community: [discord.gg/wrfcoin](https://discord.gg/wrfcoin)
- File an issue on GitHub:
  [github.com/wrfcoin/wrfcoin-core/issues](https://github.com/wrfcoin/wrfcoin-core/issues)
- Email support: support@wrfcoin.org

## Maintenance

### Updating Your Node

Keep your node updated:

```bash
# Check for updates
wrfcoin-node update-check

# Apply updates
wrfcoin-node update

# Or update manually
git pull
cargo build --release
sudo cp target/release/wrfcoin-node /usr/local/bin/
```

### Security Best Practices

Protect your node:

1. Keep your system updated with security patches
2. Use a firewall to restrict access
3. Run the node as a non-root user
4. Use SSH key authentication only
5. Implement monitoring and alerts

## Advanced Topics

### Custom Blockchain Parameters

For advanced users, you can customize blockchain parameters:

```yaml
# Advanced chain configuration
chain_params:
  block_time_seconds: 6
  epoch_length_blocks: 10000
  maximum_transaction_size: 16384
  minimum_transaction_fee: 0.00001
```

### API Integration

Integrate with the node's API:

```javascript
// Example API usage
const api = new WrfcoinApi('http://localhost:9933');
api.getBlockHeight().then(height => console.log(`Current height: ${height}`));
```

### Consensus Participation

Learn how your node participates in consensus:

- Proof of Stake: Economic security through token staking
- Proof of Location: Validates data based on geographic position
- Proof of Computation: Validates computational task results
