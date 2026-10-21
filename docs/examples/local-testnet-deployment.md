# Local Testnet Deployment Guide

Deploy a complete Wrfcoin testnet locally for development and testing.

## Overview

The local testnet includes:

- 4 validator nodes
- 2 data contributor nodes
- 1 storage host node
- 1 bridge operator node
- Complete API infrastructure
- Monitoring and metrics

## Prerequisites

```bash
# System requirements
- Docker & Docker Compose
- 8GB+ RAM
- 50GB+ disk space
- Linux/macOS/WSL2

# Install dependencies
npm install -g @wrfcoin/testnet-cli
cargo install wrfcoin-cli
```

## Quick Start

```bash
# Clone testnet configuration
git clone https://github.com/wrfcoin/local-testnet.git
cd local-testnet

# Initialize testnet
./scripts/init-testnet.sh

# Start all nodes
docker-compose up -d

# Check status
wrfcoin-cli network status
```

## Testnet Configuration

### Genesis Configuration

```json
{
  "genesis": {
    "chain_id": "wrfcoin-testnet-local",
    "genesis_time": "2025-01-01T00:00:00Z",
    "consensus_params": {
      "block_time": "6s",
      "block_size_limit": "1MB",
      "finality_blocks": 16
    },
    "initial_supply": "1000000000",
    "validators": [
      {
        "name": "validator-1",
        "public_key": "034f355bdcb7cc0af728ef3cceb9615d90684bb5b2ca5f859ab0f0b704075871aa",
        "stake": "250000",
        "commission": "0.05"
      },
      {
        "name": "validator-2",
        "public_key": "04b5df4b68bc65b5ff48e1ef6cf11c52bd44e53d1f4f9e4b3a7c9f3b55e8a3d1c",
        "stake": "250000",
        "commission": "0.05"
      },
      {
        "name": "validator-3",
        "public_key": "025476c2e83188368da1ff3e292e7acafcdb3566bb0ad253f62fc70f07aeee6357",
        "stake": "250000",
        "commission": "0.05"
      },
      {
        "name": "validator-4",
        "public_key": "02f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9",
        "stake": "250000",
        "commission": "0.05"
      }
    ],
    "initial_accounts": [
      {
        "address": "wrf1developer1234567890abcdefghijklmnopqr",
        "balance": "10000000"
      },
      {
        "address": "wrf1testuser1234567890abcdefghijklmnopqrs",
        "balance": "1000000"
      }
    ]
  }
}
```

### Docker Compose Setup

```yaml
# docker-compose.testnet.yml
version: '3.8'

services:
  # Validator Nodes
  validator-1:
    image: wrfcoin/node:latest
    container_name: validator-1
    ports:
      - '9001:9000' # P2P
      - '8081:8080' # API
    environment:
      - NODE_TYPE=validator
      - NODE_ID=validator-1
      - GENESIS_FILE=/config/genesis.json
      - PRIVATE_KEY_FILE=/keys/validator-1.key
    volumes:
      - ./config:/config:ro
      - ./keys:/keys:ro
      - validator-1-data:/data
    command: ['wrfcoin-node', '--config', '/config/validator-1.toml']

  validator-2:
    image: wrfcoin/node:latest
    container_name: validator-2
    ports:
      - '9002:9000'
      - '8082:8080'
    environment:
      - NODE_TYPE=validator
      - NODE_ID=validator-2
      - GENESIS_FILE=/config/genesis.json
      - PRIVATE_KEY_FILE=/keys/validator-2.key
    volumes:
      - ./config:/config:ro
      - ./keys:/keys:ro
      - validator-2-data:/data
    command: ['wrfcoin-node', '--config', '/config/validator-2.toml']

  validator-3:
    image: wrfcoin/node:latest
    container_name: validator-3
    ports:
      - '9003:9000'
      - '8083:8080'
    environment:
      - NODE_TYPE=validator
      - NODE_ID=validator-3
      - GENESIS_FILE=/config/genesis.json
      - PRIVATE_KEY_FILE=/keys/validator-3.key
    volumes:
      - ./config:/config:ro
      - ./keys:/keys:ro
      - validator-3-data:/data
    command: ['wrfcoin-node', '--config', '/config/validator-3.toml']

  validator-4:
    image: wrfcoin/node:latest
    container_name: validator-4
    ports:
      - '9004:9000'
      - '8084:8080'
    environment:
      - NODE_TYPE=validator
      - NODE_ID=validator-4
      - GENESIS_FILE=/config/genesis.json
      - PRIVATE_KEY_FILE=/keys/validator-4.key
    volumes:
      - ./config:/config:ro
      - ./keys:/keys:ro
      - validator-4-data:/data
    command: ['wrfcoin-node', '--config', '/config/validator-4.toml']

  # Data Contributor Nodes
  data-contributor-1:
    image: wrfcoin/node:latest
    container_name: data-contributor-1
    ports:
      - '9005:9000'
      - '8085:8080'
    environment:
      - NODE_TYPE=data_contributor
      - NODE_ID=data-contributor-1
      - WEATHER_API_KEY=${WEATHER_API_KEY}
    volumes:
      - ./config:/config:ro
      - ./keys:/keys:ro
      - data-contributor-1-data:/data
    command: ['wrfcoin-node', '--config', '/config/data-contributor-1.toml']

  data-contributor-2:
    image: wrfcoin/node:latest
    container_name: data-contributor-2
    ports:
      - '9006:9000'
      - '8086:8080'
    environment:
      - NODE_TYPE=data_contributor
      - NODE_ID=data-contributor-2
      - WEATHER_API_KEY=${WEATHER_API_KEY}
    volumes:
      - ./config:/config:ro
      - ./keys:/keys:ro
      - data-contributor-2-data:/data
    command: ['wrfcoin-node', '--config', '/config/data-contributor-2.toml']

  # Storage Host Node
  storage-host-1:
    image: wrfcoin/node:latest
    container_name: storage-host-1
    ports:
      - '9007:9000'
      - '8087:8080'
      - '5001:5001' # IPFS API
    environment:
      - NODE_TYPE=storage_host
      - NODE_ID=storage-host-1
      - STORAGE_CAPACITY=100GB
    volumes:
      - ./config:/config:ro
      - ./keys:/keys:ro
      - storage-host-1-data:/data
      - ipfs-storage:/ipfs
    command: ['wrfcoin-node', '--config', '/config/storage-host-1.toml']

  # Bridge Operator Node
  bridge-operator-1:
    image: wrfcoin/node:latest
    container_name: bridge-operator-1
    ports:
      - '9008:9000'
      - '8088:8080'
    environment:
      - NODE_TYPE=bridge_operator
      - NODE_ID=bridge-operator-1
      - ETHEREUM_RPC_URL=${ETHEREUM_RPC_URL}
      - POLYGON_RPC_URL=${POLYGON_RPC_URL}
    volumes:
      - ./config:/config:ro
      - ./keys:/keys:ro
      - bridge-operator-1-data:/data
    command: ['wrfcoin-node', '--config', '/config/bridge-operator-1.toml']

  # Database
  postgres:
    image: postgres:16
    container_name: testnet-postgres
    environment:
      POSTGRES_DB: wrfcoin_testnet
      POSTGRES_USER: wrfcoin
      POSTGRES_PASSWORD: testnet123
    ports:
      - '5432:5432'
    volumes:
      - postgres-testnet-data:/var/lib/postgresql/data

  # API Gateway
  api-gateway:
    image: wrfcoin/api-gateway:latest
    container_name: api-gateway
    ports:
      - '3000:3000'
    environment:
      - DATABASE_URL=postgres://wrfcoin:testnet123@postgres:5432/wrfcoin_testnet
      - VALIDATOR_ENDPOINTS=http://validator-1:8080,http://validator-2:8080,http://validator-3:8080,http://validator-4:8080
    depends_on:
      - postgres
      - validator-1
      - validator-2
    volumes:
      - ./config:/config:ro

  # Block Explorer
  explorer:
    image: wrfcoin/explorer:latest
    container_name: block-explorer
    ports:
      - '3001:3000'
    environment:
      - API_URL=http://api-gateway:3000
      - NETWORK_NAME=Local Testnet
    depends_on:
      - api-gateway

  # Monitoring
  prometheus:
    image: prom/prometheus:latest
    container_name: prometheus
    ports:
      - '9090:9090'
    volumes:
      - ./monitoring/prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - prometheus-data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'

  grafana:
    image: grafana/grafana:latest
    container_name: grafana
    ports:
      - '3002:3000'
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=testnet123
    volumes:
      - grafana-data:/var/lib/grafana
      - ./monitoring/grafana:/etc/grafana/provisioning

volumes:
  validator-1-data:
  validator-2-data:
  validator-3-data:
  validator-4-data:
  data-contributor-1-data:
  data-contributor-2-data:
  storage-host-1-data:
  bridge-operator-1-data:
  postgres-testnet-data:
  ipfs-storage:
  prometheus-data:
  grafana-data:

networks:
  default:
    name: wrfcoin-testnet
```

## Node Configuration Files

### Validator Configuration

```toml
# config/validator-1.toml
[node]
id = "validator-1"
type = "validator"
data_dir = "/data"

[network]
listen_address = "0.0.0.0:9000"
external_address = "validator-1:9000"
bootstrap_peers = [
    "validator-2:9000",
    "validator-3:9000",
    "validator-4:9000"
]
max_peers = 50

[consensus]
block_time = "6s"
timeout_propose = "3s"
timeout_prevote = "1s"
timeout_precommit = "1s"
timeout_commit = "5s"

[validator]
private_key_file = "/keys/validator-1.key"
commission_rate = 0.05
max_delegators = 1000

[api]
enabled = true
listen_address = "0.0.0.0:8080"
cors_origins = ["*"]

[metrics]
enabled = true
listen_address = "0.0.0.0:9100"

[logging]
level = "info"
format = "json"
```

### Data Contributor Configuration

```toml
# config/data-contributor-1.toml
[node]
id = "data-contributor-1"
type = "data_contributor"
data_dir = "/data"

[network]
listen_address = "0.0.0.0:9000"
external_address = "data-contributor-1:9000"
bootstrap_peers = [
    "validator-1:9000",
    "validator-2:9000"
]

[data_contributor]
station_id = "TEST-STATION-001"
location = { lat = 40.7128, lon = -74.0060 }
data_sources = ["synthetic", "api"]
submission_interval = "60s"
quality_threshold = 0.8

[weather_apis]
openweather = { api_key = "${WEATHER_API_KEY}", enabled = true }
weatherapi = { api_key = "${WEATHER_API_KEY_2}", enabled = false }

[synthetic_data]
enabled = true
temperature_range = { min = -10.0, max = 35.0 }
humidity_range = { min = 30.0, max = 90.0 }
pressure_range = { min = 990.0, max = 1030.0 }
```

## Initialization Scripts

### Generate Keys and Configuration

```bash
#!/bin/bash
# scripts/init-testnet.sh

set -e

echo "Initializing Wrfcoin Local Testnet..."

# Create directories
mkdir -p config keys logs data

# Generate validator keys
for i in {1..4}; do
    echo "Generating keys for validator-$i..."
    wrfcoin-cli keys generate --output-file "keys/validator-$i.key"
done

# Generate data contributor keys
for i in {1..2}; do
    echo "Generating keys for data-contributor-$i..."
    wrfcoin-cli keys generate --output-file "keys/data-contributor-$i.key"
done

# Generate other node keys
wrfcoin-cli keys generate --output-file "keys/storage-host-1.key"
wrfcoin-cli keys generate --output-file "keys/bridge-operator-1.key"

# Generate genesis file
echo "Generating genesis configuration..."
cat > config/genesis.json << 'EOF'
{
  "genesis": {
    "chain_id": "wrfcoin-testnet-local",
    "genesis_time": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "consensus_params": {
      "block_time": "6s",
      "block_size_limit": "1MB",
      "finality_blocks": 16
    },
    "initial_supply": "1000000000"
  }
}
EOF

# Set permissions
chmod 600 keys/*.key
chmod 644 config/*

echo "Testnet initialization complete!"
echo "Run 'docker-compose -f docker-compose.testnet.yml up -d' to start the testnet"
```

### Health Check Script

```bash
#!/bin/bash
# scripts/health-check.sh

echo "Checking Wrfcoin Testnet Health..."

# Check all validator nodes
for i in {1..4}; do
    echo -n "Validator $i: "
    if curl -s -f "http://localhost:808$i/health" > /dev/null; then
        echo "✅ Online"
    else
        echo "❌ Offline"
    fi
done

# Check data contributors
for i in {1..2}; do
    port=$((8084 + i))
    echo -n "Data Contributor $i: "
    if curl -s -f "http://localhost:$port/health" > /dev/null; then
        echo "✅ Online"
    else
        echo "❌ Offline"
    fi
done

# Check storage host
echo -n "Storage Host: "
if curl -s -f "http://localhost:8087/health" > /dev/null; then
    echo "✅ Online"
else
    echo "❌ Offline"
fi

# Check bridge operator
echo -n "Bridge Operator: "
if curl -s -f "http://localhost:8088/health" > /dev/null; then
    echo "✅ Online"
else
    echo "❌ Offline"
fi

# Check consensus
echo -n "Network Consensus: "
latest_block=$(curl -s "http://localhost:8081/api/blockchain/latest-block" | jq -r '.height // 0')
if [ "$latest_block" -gt 0 ]; then
    echo "✅ Active (Block #$latest_block)"
else
    echo "❌ Stalled"
fi

# Check database
echo -n "Database: "
if pg_isready -h localhost -p 5432 -U wrfcoin > /dev/null 2>&1; then
    echo "✅ Connected"
else
    echo "❌ Disconnected"
fi
```

## Development Workflows

### Testing Climate Data Submission

```bash
#!/bin/bash
# scripts/test-data-submission.sh

echo "Testing climate data submission..."

# Submit test weather data
curl -X POST http://localhost:8085/api/data/submit \
  -H "Content-Type: application/json" \
  -d '{
    "station_id": "TEST-STATION-001",
    "timestamp": "'$(date -u +%Y-%m-%dT%H:%M:%SZ)'",
    "temperature": 22.5,
    "humidity": 65.0,
    "pressure": 1013.25,
    "wind_speed": 5.2,
    "wind_direction": "NW",
    "quality_score": 0.95
  }'

echo "Data submitted. Check block explorer at http://localhost:3001"
```

### Cross-Chain Testing

`NOT_AVAILABLE`. A local testnet does not include a transfer bridge.

## Monitoring and Debugging

### View Logs

```bash
# All services
docker-compose -f docker-compose.testnet.yml logs -f

# Specific validator
docker-compose -f docker-compose.testnet.yml logs -f validator-1

# Filter by error level
docker-compose -f docker-compose.testnet.yml logs | grep ERROR
```

### Performance Metrics

```bash
# Network statistics
curl http://localhost:8081/api/network/stats

# Validator performance
curl http://localhost:8081/api/validators/performance

# Transaction pool
curl http://localhost:8081/api/mempool/status
```

### Database Queries

```sql
-- Connect to testnet database
psql -h localhost -p 5432 -U wrfcoin -d wrfcoin_testnet

-- View recent blocks
SELECT height, hash, timestamp, validator FROM blocks ORDER BY height DESC LIMIT 10;

-- View transactions
SELECT hash, from_address, to_address, amount, block_height FROM transactions ORDER BY id DESC LIMIT 10;

-- Weather data submissions
SELECT station_id, temperature, humidity, timestamp FROM weather_data ORDER BY timestamp DESC LIMIT 10;
```

## Cleanup and Reset

```bash
#!/bin/bash
# scripts/reset-testnet.sh

echo "Resetting testnet..."

# Stop all services
docker-compose -f docker-compose.testnet.yml down -v

# Remove data volumes
docker volume prune -f

# Clean up containers
docker container prune -f

# Regenerate configuration
./scripts/init-testnet.sh

echo "Testnet reset complete. Ready to restart."
```

## Troubleshooting

### Common Issues

1. **Nodes not connecting**: Check network configuration and firewall rules
2. **Consensus stalled**: Verify at least 3 validators are online
3. **Database errors**: Ensure PostgreSQL is running and accessible
4. **Out of memory**: Increase Docker memory allocation
5. **Port conflicts**: Check if ports are already in use

### Debug Commands

```bash
# Check node connectivity
wrfcoin-cli network peers --endpoint http://localhost:8081

# Validate configuration
wrfcoin-cli config validate --config config/validator-1.toml

# Check consensus state
wrfcoin-cli consensus status --endpoint http://localhost:8081

# Monitor real-time logs
docker-compose -f docker-compose.testnet.yml logs -f --tail=100
```

This local testnet provides a complete development environment for testing all Wrfcoin features
before mainnet deployment.

---

## Phase 3C TDD Gate

After the testnet is running, verify the Phase 3C quality-slashing and reputation-reward contracts
with the canonical gate script:

```bash
# From the core4/ repo root
# Rust-only (TDD-12.2–12.5 — no running node required)
bash scripts/run-phase3c-security-api.sh --rust-only

# Full gate including API tests (TDD-12.1–12.5 — requires testnet at :8556)
export WRFCOIN_OPERATOR_TOKEN=<your-64-hex-token>
bash scripts/run-phase3c-security-api.sh
```

### Expected output

```
[PASS] TDD-12.1  API slashing gate end-to-end
[PASS] TDD-12.2  Low-quality streak slashing determinism
[PASS] TDD-12.3  Reputation-weighted reward scaling
[PASS] TDD-12.4  Low-band reveal quorum (verify path)
[PASS] TDD-12.5  Low-band reveal quorum (mining path)
Phase 3C gate: ALL PASS
```

### CI coverage

TDD-12.2–12.5 (pure Rust) run automatically on every PR via the
`.github/workflows/tdd-phase3c.yml` workflow — no testnet required.
TDD-12.1 requires a live node and is triggered manually via `workflow_dispatch`.

See `TESTNET_PHASE_3C_TDD_NEXT_WAVE.md` for the full Phase 3C test matrix and implementation
backlog for TDD-12.6–12.10.
