# Wrfcoin Node Operation Guide

This guide provides detailed instructions for operating and maintaining a Wrfcoin node.

## Table of Contents

1. [Introduction](#introduction)
2. [Node Types and Roles](#node-types-and-roles)
3. [Hardware Requirements](#hardware-requirements)
4. [Installation](#installation)
5. [Configuration](#configuration)
6. [Starting and Stopping](#starting-and-stopping)
7. [Node Monitoring](#node-monitoring)
8. [Security Considerations](#security-considerations)
9. [Validator Operations](#validator-operations)
10. [Weather Station Integration](#weather-station-integration)
11. [Troubleshooting](#troubleshooting)
12. [Maintenance and Updates](#maintenance-and-updates)

## Introduction

Wrfcoin nodes form the backbone of the network, providing various functions including transaction
processing, block validation, weather data verification, and network security. This guide covers all
aspects of running and maintaining a Wrfcoin node.

## Node Types and Roles

Wrfcoin supports several node types, each with distinct roles and responsibilities:

### Full Node

A standard node that validates and relays transactions and blocks, but does not create new blocks.

**Primary Functions:**

- Maintains a complete copy of the blockchain
- Validates transactions and blocks
- Relays transactions and blocks to peers
- Serves blockchain data to light clients

### Validator Node

A specialized node that participates in the consensus process, creating and validating blocks.

**Primary Functions:**

- All functions of a full node
- Creates new blocks (block production)
- Participates in consensus voting
- Signs and validates weather data
- Earns rewards for validation services

### Weather Data Node

A node specialized in collecting, validating, and storing weather data.

**Primary Functions:**

- Verifies incoming weather data against scientific models
- Runs weather data validation algorithms
- Stores specialized weather datasets
- Participates in data consensus mechanisms

### Computational Node

A node designed for processing weather data and running computational models.

**Primary Functions:**

- Executes weather prediction models
- Performs computational tasks on blockchain data
- Validates computational results from other nodes
- Contributes to decentralized weather forecast generation

### Light Client

A minimal node suitable for resource-constrained environments.

**Primary Functions:**

- Verifies block headers
- Submits transactions to the network
- Queries other nodes for blockchain data
- Minimal storage and bandwidth requirements

## Hardware Requirements

Hardware requirements vary by node type:

### Full Node

- **CPU**: 4+ cores
- **RAM**: 8GB minimum, 16GB recommended
- **Storage**: 500GB SSD
- **Network**: 10Mbps+ connection, 1TB/month data allowance
- **Operating System**: Linux (Ubuntu 22.04+ recommended), macOS, Windows 10/11

### Validator Node

- **CPU**: 8+ cores
- **RAM**: 16GB minimum, 32GB recommended
- **Storage**: 1TB SSD
- **Network**: 50Mbps+ connection, 2TB/month data allowance
- **Operating System**: Linux (Ubuntu 22.04+ recommended)

### Weather Data Node

- **CPU**: 8+ cores
- **RAM**: 32GB minimum, 64GB recommended
- **Storage**: 2TB SSD
- **Network**: 100Mbps+ connection, 4TB/month data allowance
- **Operating System**: Linux (Ubuntu 22.04+ recommended)

### Computational Node

- **CPU**: 16+ cores or GPU support
- **RAM**: 64GB minimum
- **GPU**: NVIDIA with CUDA support (for some computational tasks)
- **Storage**: 2TB SSD
- **Network**: 100Mbps+ connection
- **Operating System**: Linux (Ubuntu 22.04+ recommended)

## Installation

### Package Installation

#### Linux (Ubuntu/Debian)

```bash
# Add Wrfcoin repository
curl -sSL https://dist.wrfcoin.network/setup.sh | sudo bash

# Install Wrfcoin node
sudo apt update
sudo apt install wrfcoin-node

# Install additional packages for specific node types
# For validator:
sudo apt install wrfcoin-validator-tools

# For weather data node:
sudo apt install wrfcoin-weather-tools

# For computational node:
sudo apt install wrfcoin-compute-tools
```

#### macOS

```bash
# Using Homebrew
brew tap wrfcoin/wrfcoin
brew install wrfcoin-node
```

#### Windows

Download the installer from [https://wrfcoin.network/downloads](https://wrfcoin.network/downloads)
and follow the installation wizard.

### Docker Installation

For containerized deployments:

```bash
# Pull the latest image
docker pull wrfcoin/node:latest

# Run a full node
docker run -d \
  --name wrfcoin-node \
  -p 8545:8545 -p 8546:8546 -p 30303:30303 \
  -v wrfcoin-data:/data \
  wrfcoin/node:latest

# For validator node
docker run -d \
  --name wrfcoin-validator \
  -p 8545:8545 -p 8546:8546 -p 30303:30303 \
  -v wrfcoin-validator-data:/data \
  -e NODE_TYPE=validator \
  -e VALIDATOR_KEY=/data/validator_key \
  wrfcoin/node:validator
```

### Building from Source

For advanced users who want to build from source:

```bash
# Clone the repository
git clone https://github.com/wrfcoin/wrfcoin.git
cd wrfcoin

# Build the node software
cargo build --release

# Install
sudo cp target/release/wrfcoin-node /usr/local/bin/
```

## Configuration

The main configuration file is located at `/etc/wrfcoin/config.toml` (Linux/macOS) or
`C:\Program Files\Wrfcoin\config.toml` (Windows).

### Basic Configuration

```toml
# Basic node configuration
[node]
# Node type: "full", "validator", "weather", "computational", "light"
type = "full"

# Network to connect to: "mainnet", "testnet"
network = "mainnet"

# Data directory
data_dir = "/var/lib/wrfcoin"

# Logging level: "error", "warn", "info", "debug", "trace"
log_level = "info"

[network]
# Listen address for P2P connections
listen_address = "0.0.0.0:30303"

# Maximum number of peers
max_peers = 50

# Enable NAT traversal
nat_enabled = true

# External IP (leave empty for auto-discovery)
external_ip = ""

[rpc]
# Enable HTTP-RPC server
http_enabled = true

# HTTP-RPC listening interface
http_listen_addr = "127.0.0.1"

# HTTP-RPC listening port
http_port = 8545

# Enable WebSocket-RPC server
ws_enabled = true

# WebSocket-RPC listening port
ws_port = 8546
```

### Validator Configuration

Additional settings for validator nodes:

```toml
[validator]
# Validator name (for identification in explorer)
name = "MyValidatorNode"

# Staking address
staking_address = "WRF1..."

# Path to validator key file
key_file = "/etc/wrfcoin/validator_key.json"

# Automatic restart on failure
auto_restart = true

# Rewards payout address
rewards_address = "WRF1..."
```

### Weather Data Node Configuration

Additional settings for weather data nodes:

```toml
[weather]
# Data sources to connect to
sources = ["noaa", "ecmwf", "wmo"]

# API keys for data sources
api_keys = { noaa = "key1", ecmwf = "key2" }

# Geographic regions to focus on (leave empty for global)
regions = ["north-america", "europe"]

# Data storage limits
storage_limit_gb = 1000

# Data retention policy (in days)
data_retention = 365
```

### Computational Node Configuration

Additional settings for computational nodes:

```toml
[computational]
# Enable GPU acceleration
gpu_enabled = true

# Computational resources to offer
cpu_cores = 8
memory_gb = 32
gpu_memory_gb = 8

# Models to support
models = ["simple-forecast", "advanced-forecast", "climate-analysis"]

# Task bidding strategy
min_bid = 10  # Minimum WRF tokens to accept
max_tasks = 5  # Maximum concurrent tasks
```

## Starting and Stopping

### Using Systemd (Linux)

The Wrfcoin node is installed as a systemd service:

```bash
# Start the node
sudo systemctl start wrfcoin-node

# Stop the node
sudo systemctl stop wrfcoin-node

# Restart the node
sudo systemctl restart wrfcoin-node

# Check status
sudo systemctl status wrfcoin-node

# Enable start on boot
sudo systemctl enable wrfcoin-node
```

### Using Command Line

```bash
# Start in foreground
wrfcoin-node

# Start with custom config file
wrfcoin-node --config /path/to/config.toml

# Start in background
wrfcoin-node --daemon

# Stop a running daemon
wrfcoin-cli shutdown
```

### Using Docker

```bash
# Start the container
docker start wrfcoin-node

# Stop the container
docker stop wrfcoin-node

# View logs
docker logs wrfcoin-node

# Execute commands inside the container
docker exec wrfcoin-node wrfcoin-cli status
```

## Node Monitoring

### Command Line Monitoring

Check node status:

```bash
wrfcoin-cli status
```

Get synchronization status:

```bash
wrfcoin-cli sync-status
```

List connected peers:

```bash
wrfcoin-cli peers
```

Check validator performance:

```bash
wrfcoin-cli validator-stats
```

### Metrics and Monitoring

Wrfcoin nodes expose Prometheus-compatible metrics on port 9090 by default.

Important metrics to monitor:

- `wrfcoin_block_height`: Current block height
- `wrfcoin_peers_connected`: Number of connected peers
- `wrfcoin_sync_status`: Synchronization status (0-100%)
- `wrfcoin_validator_uptime`: Validator uptime percentage
- `wrfcoin_system_memory_usage`: Node memory usage
- `wrfcoin_system_cpu_usage`: Node CPU usage
- `wrfcoin_disk_usage`: Node disk usage

Example Prometheus configuration:

```yaml
scrape_configs:
  - job_name: 'wrfcoin-node'
    scrape_interval: 15s
    static_configs:
      - targets: ['localhost:9090']
```

### Web Dashboard

The node includes a web-based dashboard accessible at `http://localhost:8080` by default. To enable:

```toml
[dashboard]
enabled = true
port = 8080
# Set to 0.0.0.0 to allow external access (use with caution)
listen_address = "127.0.0.1"
```

### Alert Setup

Configure alerts for critical conditions:

```bash
# Install alert client
sudo apt install wrfcoin-alerts

# Configure alerts (email example)
cat > /etc/wrfcoin/alerts.conf << EOF
[alerts]
enabled = true
methods = ["email", "telegram"]

[email]
smtp_server = "smtp.example.com"
smtp_port = 587
username = "alerts@example.com"
password = "your_password"
recipients = ["admin@example.com"]

[telegram]
bot_token = "your_bot_token"
chat_id = "your_chat_id"
EOF

# Start alert service
sudo systemctl enable wrfcoin-alerts
sudo systemctl start wrfcoin-alerts
```

## Security Considerations

### Network Security

1. **Firewall Configuration**: Only expose the necessary ports:

   - P2P port: 30303 (TCP/UDP)
   - RPC port: 8545 (restrict to localhost or VPN)
   - WebSocket port: 8546 (restrict to localhost or VPN)
   - Metrics port: 9090 (restrict to localhost or monitoring network)

   Example using ufw:

   ```bash
   sudo ufw allow 30303/tcp
   sudo ufw allow 30303/udp
   sudo ufw deny 8545/tcp
   sudo ufw deny 8546/tcp
   sudo ufw deny 9090/tcp
   ```

2. **DDoS Protection**: Configure rate limiting:

   ```toml
   [network.protection]
   # Enable DDoS protection
   enabled = true

   # Max connections per IP
   max_connections_per_ip = 5

   # Rate limiting (requests per second)
   rate_limit = 100
   ```

### Key Management

1. **Secure Key Storage**: Store validator keys securely:

   ```toml
   [validator.key_security]
   # Use hardware security module
   use_hsm = true

   # HSM device path (YubiKey example)
   hsm_path = "yubihsm://localhost:12345"

   # Alternative: encrypted key file
   encrypted_key = "/path/to/encrypted/key"
   ```

2. **Key Rotation**: Regularly rotate operator keys:

   ```bash
   # Generate new operator key
   wrfcoin-cli keys generate-operator --output new_operator.json

   # Register new key
   wrfcoin-cli validator register-key --key new_operator.json

   # Deregister old key (after confirmation)
   wrfcoin-cli validator deregister-key --key old_operator.json
   ```

### Access Control

1. **RPC Authentication**: Enable JWT authentication for RPC:

   ```toml
   [rpc.auth]
   enabled = true
   jwt_secret = "/path/to/jwt_secret"
   ```

2. **API Key Management**: For external services:

   ```toml
   [api]
   enable_auth = true

   # Key rotation interval (in days)
   key_rotation = 30

   # Allowed origins
   allowed_origins = ["https://example.com"]
   ```

## Validator Operations

### Staking and Delegation

1. **Stake Tokens**:

   ```bash
   # Stake tokens for validation
   wrfcoin-cli validator stake --amount 10000 --from my_wallet.json
   ```

2. **Manage Delegation**:

   ```bash
   # List delegators
   wrfcoin-cli validator list-delegators

   # Set commission rate
   wrfcoin-cli validator set-commission --rate 0.05
   ```

### Validator Performance

Monitor and optimize validator performance:

1. **Check Performance**:

   ```bash
   # View validator stats
   wrfcoin-cli validator stats

   # Check ranking
   wrfcoin-cli validator ranking
   ```

2. **Performance Tuning**:

   ```toml
   [validator.performance]
   # Optimize for block production
   optimize_for = "block_production"

   # Pre-validate transactions
   prevalidate_txs = true

   # Block production timeout (ms)
   block_timeout = 250
   ```

### Slashing Protection

Avoid slashing penalties:

```toml
[validator.slashing_protection]
# Enable double-signing protection
double_sign_protection = true

# Minimum peers before validating
min_peers_required = 3

# Back off from validation if system resources are low
resource_monitor = true
```

## Weather Station Integration

### Setting Up a Weather Station

1. **Register Station**:

   ```bash
   # Register a new weather station
   wrfcoin-cli weather register-station \
     --name "NYC Central Park" \
     --lat 40.7829 \
     --lon -73.9654 \
     --elevation 42.7 \
     --type "official"
   ```

2. **Configure Data Sources**:

   ```toml
   [weather.station]
   # Station ID (from registration)
   id = "WS-NYC-001"

   # Data collection interval (seconds)
   interval = 300

   # Connected sensors
   sensors = [
     { type = "temperature", model = "Davis Pro2", port = "/dev/ttyUSB0" },
     { type = "humidity", model = "Davis Pro2", port = "/dev/ttyUSB0" },
     { type = "pressure", model = "Davis Pro2", port = "/dev/ttyUSB0" },
     { type = "wind", model = "Davis Anemometer", port = "/dev/ttyUSB1" }
   ]

   # Data validation before submission
   validate_before_submit = true
   ```

3. **Submit Weather Data**:

   ```bash
   # Manual submission
   wrfcoin-cli weather submit-data \
     --station-id "WS-NYC-001" \
     --temperature 22.5 \
     --humidity 65 \
     --pressure 1013.2

   # Or set up automatic submission
   wrfcoin-cli weather enable-auto-submit --interval 300
   ```

### Weather Data Verification

Verify incoming weather data:

```toml
[weather.verification]
# Enable data verification
enabled = true

# Verification methods
methods = ["statistical", "spatial", "temporal"]

# Reference datasets
reference_datasets = ["NOAA", "ECMWF"]

# Outlier detection sensitivity (1-10)
outlier_sensitivity = 5
```

## Troubleshooting

### Common Issues

#### Node Won't Start

```bash
# Check system resources
free -h
df -h

# Check logs
sudo journalctl -u wrfcoin-node -n 100

# Verify config file
wrfcoin-cli verify-config --config /etc/wrfcoin/config.toml

# Try starting with verbose logging
wrfcoin-node --log-level debug
```

#### Synchronization Issues

```bash
# Check sync status
wrfcoin-cli sync-status

# Connect to specific peers
wrfcoin-cli connect-peer --addr "node.wrfcoin.network:30303"

# Reset peer database
wrfcoin-cli reset-peers

# Try fast sync mode
wrfcoin-node --sync-mode fast
```

#### Performance Problems

```bash
# Check system performance
wrfcoin-cli system-info

# Optimize database
wrfcoin-cli db optimize

# Limit resource usage
wrfcoin-cli set-resource-limits --cpu 4 --memory 8G
```

### Diagnostic Tools

1. **Network Diagnostics**:

   ```bash
   # Check network connectivity
   wrfcoin-cli network-diagnostics

   # Test bandwidth to known peers
   wrfcoin-cli test-bandwidth
   ```

2. **Database Integrity**:

   ```bash
   # Check database integrity
   wrfcoin-cli db verify

   # Repair database (if needed)
   wrfcoin-cli db repair
   ```

3. **Log Analysis**:

   ```bash
   # Analyze logs for errors
   wrfcoin-cli analyze-logs

   # Export logs for support
   wrfcoin-cli export-logs --days 3 --output logs.zip
   ```

## Maintenance and Updates

### Regular Maintenance

1. **Database Maintenance**:

   ```bash
   # Compact database (weekly)
   wrfcoin-cli db compact

   # Prune old data (monthly)
   wrfcoin-cli db prune --keep-days 90
   ```

2. **Log Rotation**:

   ```bash
   # Configure log rotation
   sudo nano /etc/logrotate.d/wrfcoin
   ```

   Example configuration:

   ```
   /var/log/wrfcoin/*.log {
       daily
       rotate 7
       compress
       delaycompress
       missingok
       notifempty
       create 640 wrfcoin wrfcoin
       postrotate
           systemctl reload wrfcoin-node
       endscript
   }
   ```

### Software Updates

1. **Check for Updates**:

   ```bash
   # Check current version
   wrfcoin-node --version

   # Check for updates
   wrfcoin-cli check-updates
   ```

2. **Automatic Updates**:

   ```toml
   [updates]
   # Enable automatic updates
   auto_update = true

   # Update channel: stable, beta, edge
   channel = "stable"

   # Update time (UTC)
   update_hour = 3  # 3:00 AM UTC
   ```

3. **Manual Updates**:

   ```bash
   # Update (package managers)
   sudo apt update
   sudo apt upgrade wrfcoin-node

   # Update (Docker)
   docker pull wrfcoin/node:latest
   docker stop wrfcoin-node
   docker rm wrfcoin-node
   docker run -d --name wrfcoin-node -v wrfcoin-data:/data wrfcoin/node:latest
   ```

### Backup Procedures

1. **Node Backup**:

   ```bash
   # Backup node data
   wrfcoin-cli backup create --output /backup/wrfcoin-backup.tar.gz

   # Scheduled backups
   wrfcoin-cli backup schedule --interval daily --keep 7
   ```

2. **Key Backup**:

   ```bash
   # Export encrypted key backup
   wrfcoin-cli keys export --encrypted --output /secure/keys-backup.json
   ```

3. **Recovery Procedure**:

   ```bash
   # Restore from backup
   wrfcoin-cli backup restore --input /backup/wrfcoin-backup.tar.gz

   # Import keys
   wrfcoin-cli keys import --input /secure/keys-backup.json
   ```

By following this guide, you'll be able to effectively operate and maintain a Wrfcoin node. For
additional information or support, visit the
[Wrfcoin documentation portal](https://docs.wrfcoin.network) or join the
[community forum](https://forum.wrfcoin.network).
