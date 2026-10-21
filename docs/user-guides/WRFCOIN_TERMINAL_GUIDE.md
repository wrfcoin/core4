# Wrfcoin Terminal Operations Guide

## Overview

This guide provides terminal commands for operating the Wrfcoin blockchain platform, including
weather station integration, cross-chain operations, governance, and testing.

## Quick Start

### Start All Services

```bash
# Start all microservices in parallel
npm run start:all

# Or start individual services
node backend/server/cross_chain_protocol_server.js &
cd frontend && npm run dev -- --host 0.0.0.0 --port 5000 &
npx tsx backend/server/iot_sensor_networks_server_fixed.ts &
node backend/server/cross_chain_websocket_server.js &
node backend/server/wallet_server.js &
cd backend/server && PORT=3001 npx tsx enhanced_unified_api.ts &
cd backend/server && node governance_server_stable.js &
```

### Health Check All Services

```bash
# Check all service health
curl http://localhost:3000/health  # Cross-Chain Protocol
curl http://localhost:3001/health  # Enhanced API
curl http://localhost:4000/health  # Wallet Server
curl http://localhost:5001/health  # WebSocket Server
curl http://localhost:8000/health  # Governance Server
curl http://localhost:3002/health  # IoT Sensor Networks
```

## Command Discovery

Use these commands to discover what is actually runnable in this repository at any time:

```bash
# List all npm workspace scripts (top-level app commands)
npm run

# Filter to MVP/dev/test commands quickly
npm run | rg -i "mvp|dev|test|start|docs|ci"

# Show Rust workspace packages
cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name'

# Show available binaries in native-chain package
cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name=="wrfcoin-native-chain") | .targets[] | select(.kind[]=="bin") | .name'
```

Current launch-related commands:

```bash
# MVP product mode (frontend + simple backend)
npm run dev:mvp

# Native-chain launch API runtime (commit/reveal/oracle flow)
cargo run -p wrfcoin-native-chain --bin launch_api

# Launch API integration test (non-skipped when API is running)
WRFCOIN_API_BASE_URL=http://127.0.0.1:8556 \
  npx jest tests/integration/native-chain-launch-api.test.js --runInBand --forceExit
```

## Testing Commands

### Comprehensive Test Suites

```bash
# Run the ultimate 69-test suite (100% success rate)
cd consolidated-tests/utils && node ultimate-test-suite.js

# Run unified comprehensive tests (95.7% success rate)
cd consolidated-tests/utils && node unified-69-test-suite.js

# Run fast comprehensive testing
cd consolidated-tests/utils && node fast-comprehensive-test.js

# Run specific test categories
cd consolidated-tests/utils && node test-blockchain-integration.js
cd consolidated-tests/utils && node test-cross-chain-integration.js
cd consolidated-tests/utils && node test-governance-system.js
cd consolidated-tests/utils && node test-iot-devices.js
cd consolidated-tests/utils && node test-load-testing.js
cd consolidated-tests/utils && node test-governance-security.js
cd consolidated-tests/utils && node test-storage-networks.js
cd consolidated-tests/utils && node test-environmental-data-validation.js
cd consolidated-tests/utils && node test-layer2-scaling.js
cd consolidated-tests/utils && node test-streamr.js
```

### Weather Station Integration Testing

```bash
# Test weather station protocols
cd consolidated-tests/utils && node test-weather-station-protocol.js

# Test environmental data validation (100% success rate)
cd consolidated-tests/utils && node test-environmental-data-validation.js

# Test weather-to-blockchain integration
cd consolidated-tests && npx jest integration/weather-to-blockchain.test.js
```

### Performance and Load Testing

```bash
# Load testing (handles 20,000 TPS)
cd consolidated-tests/utils && node test-load-testing.js

# Layer 2 scaling tests (50,000 TPS sustained)
cd consolidated-tests/utils && node test-layer2-scaling.js

# Artillery load testing
cd consolidated-tests && npx artillery run performance/load-test-config.yml
```

### Security Testing

```bash
# Governance security testing
cd consolidated-tests/utils && node test-governance-security.js

# Security infrastructure tests
cd consolidated-tests/security && node test-runner.js

# JWT and authentication tests
cd consolidated-tests && npx jest unit/middleware/auth.test.js
```

## Weather Data Operations

### Manual Weather Data Submission

```bash
# Submit weather data via API
curl -X POST http://localhost:3001/api/weather/submit \
  -H "Content-Type: application/json" \
  -d '{
    "temperature": 22.5,
    "humidity": 65,
    "pressure": 1013.25,
    "wind_speed": 12.3,
    "timestamp": "'$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ)'",
    "station_id": "STATION_001",
    "coordinates": {
      "latitude": 40.7128,
      "longitude": -74.0060
    }
  }'

# Get current weather readings
curl http://localhost:3001/api/weather/current

# Get weather earnings
curl http://localhost:3001/api/weather/earnings
```

### Weather Station Protocol Testing

```bash
# Test WeatherXM integration
cd consolidated-tests/utils && node test-weather-station-protocol.js

# Test specific weather protocols
curl http://localhost:3002/api/protocols/weatherxm/status
curl http://localhost:3002/api/protocols/davis/status
curl http://localhost:3002/api/protocols/netatmo/status
```

## Blockchain Operations

### Wallet Management

```bash
# Generate new mnemonic
curl -X POST http://localhost:4000/api/wallet/generate-mnemonic

# Validate mnemonic
curl -X POST http://localhost:4000/api/wallet/validate-mnemonic \
  -H "Content-Type: application/json" \
  -d '{"mnemonic": "your twelve word mnemonic phrase here..."}'

# Get wallet balance
curl http://localhost:4000/api/wallet/balance

# Send transaction
curl -X POST http://localhost:4000/api/wallet/send \
  -H "Content-Type: application/json" \
  -d '{
    "to": "recipient_address",
    "amount": "10.5",
    "token": "WRF"
  }'
```

### Blockchain Status and Mining

```bash
# Get blockchain status
curl http://localhost:3000/api/blockchain/status

# Get mining information
curl http://localhost:3001/api/mining/stats

# Start mining
curl -X POST http://localhost:3001/api/mining/start

# Stop mining
curl -X POST http://localhost:3001/api/mining/stop

# Get block information
curl http://localhost:3001/api/blockchain/block/latest
curl http://localhost:3001/api/blockchain/block/123
```

## Cross-Chain Operations

`NOT_AVAILABLE` in the public release. No bridge transfer commands or APIs are
supported.

## Governance Operations

### Proposals and Voting

```bash
# List active proposals
curl http://localhost:8000/api/governance/proposals

# Create new proposal
curl -X POST http://localhost:8000/api/governance/proposals \
  -H "Content-Type: application/json" \
  -d '{
    "title": "Weather Station Upgrade Proposal",
    "description": "Proposal to upgrade weather station protocols",
    "category": "technical",
    "voting_period": 604800
  }'

# Vote on proposal
curl -X POST http://localhost:8000/api/governance/vote \
  -H "Content-Type: application/json" \
  -d '{
    "proposal_id": "1",
    "vote": "yes",
    "voting_power": "1000"
  }'

# Get voting results
curl http://localhost:8000/api/governance/proposals/1/results

# Delegate voting power
curl -X POST http://localhost:8000/api/governance/delegate \
  -H "Content-Type: application/json" \
  -d '{
    "delegate_address": "0x...",
    "amount": "5000"
  }'
```

### Treasury Management

```bash
# Get treasury status
curl http://localhost:8000/api/governance/treasury

# Create spending proposal
curl -X POST http://localhost:8000/api/governance/treasury/propose \
  -H "Content-Type: application/json" \
  -d '{
    "amount": "10000",
    "recipient": "0x...",
    "purpose": "Weather station maintenance",
    "category": "infrastructure"
  }'

# Get treasury transactions
curl http://localhost:8000/api/governance/treasury/transactions
```

## NFT and Computational Operations

### NFT Management

```bash
# Get NFT marketplace
curl http://localhost:3001/api/nft/marketplace

# Create computational NFT
curl -X POST http://localhost:3001/api/nft/create \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Weather Oracle NFT",
    "computational_power": "150",
    "accuracy": "99.8",
    "collection": "weather_stations"
  }'

# Get NFT details
curl http://localhost:3001/api/nft/1001

# List user NFTs
curl http://localhost:3001/api/nft/user/0x.../collection
```

## IoT and Sensor Networks

### Sensor Network Status

```bash
# Get IoT network overview (170,340 sensors accessible)
curl http://localhost:3002/api/networks/overview

# Get specific network status
curl http://localhost:3002/api/networks/ttn/status      # The Things Network
curl http://localhost:3002/api/networks/helium/status   # Helium Network
curl http://localhost:3002/api/networks/sensor/status   # Sensor.Community

# Get sensor data streams
curl http://localhost:3002/api/sensors/data/realtime

# Get network statistics
curl http://localhost:3002/api/networks/stats
```

## Database Operations

### Database Management

```bash
# Push database schema changes
npm run db:push

# View database status
npm run db:status

# Seed database with test data
npm run db:seed

# Backup database
npm run db:backup

# Run database migrations (using Drizzle ORM)
npx drizzle-kit push:pg
```

### Database Queries

```bash
# Execute SQL queries safely
npm run db:query "SELECT COUNT(*) FROM weather_data;"
npm run db:query "SELECT * FROM governance_proposals LIMIT 10;"
npm run db:query "SELECT status, COUNT(*) FROM bridge_transfers GROUP BY status;"
```

## Development and Debugging

### Log Management

```bash
# View real-time logs
tail -f logs/app.log
tail -f logs/blockchain.log
tail -f logs/weather.log
tail -f logs/cross-chain.log

# View specific service logs
pm2 logs cross-chain
pm2 logs weather-api
pm2 logs governance

# Clear logs
npm run logs:clear
```

### Development Tools

```bash
# TypeScript compilation check
npm run type-check

# Run linting
npm run lint

# Run prettier formatting
npm run format

# Generate API documentation
npm run docs:generate

# Build for production
npm run build

# Start production server
npm run start:prod
```

### Monitoring and Analytics

```bash
# Get system performance metrics
curl http://localhost:3001/api/analytics/performance

# Get network health
curl http://localhost:3001/api/health/network

# Get detailed system stats
curl http://localhost:3001/api/stats/detailed

# Monitor WebSocket connections
curl http://localhost:5001/api/websocket/status
```

## Environment Management

### Configuration

```bash
# Set environment variables
export NODE_ENV=development
export DATABASE_URL="postgresql://..."
export BLOCKCHAIN_NETWORK=testnet

# Load environment from .env
source .env

# Validate configuration
npm run config:validate
```

### Service Management

```bash
# Stop all services
pkill -f "node.*server"
pkill -f "npm run dev"

# Restart specific service
pm2 restart cross-chain-server
pm2 restart weather-api

# View service status
pm2 status
pm2 monit
```

## Security Operations

### Key Management

```bash
# Generate new keys
curl -X POST http://localhost:4000/api/security/generate-keys

# Rotate secrets
npm run security:rotate-secrets

# Audit security
npm run security:audit

# Check HSM status
curl http://localhost:8300/api/vault/status
```

## Performance Optimization

### Cache Management

```bash
# Clear all caches
npm run cache:clear

# Warm up caches
npm run cache:warmup

# Cache statistics
curl http://localhost:3001/api/cache/stats
```

### Resource Monitoring

```bash
# Monitor resource usage
htop
iostat 1
netstat -tulpn

# Memory usage
free -h
cat /proc/meminfo

# Disk usage
df -h
du -sh logs/
```

## Troubleshooting

### Common Issues

```bash
# Fix WebSocket connection issues
curl http://localhost:5001/health

# Check database connectivity
npm run db:test-connection

# Verify API endpoints
curl -I http://localhost:3001/health

# Test cross-chain bridges
cd consolidated-tests/utils && node test-cross-chain-integration.js

# Debug weather station connectivity
cd consolidated-tests/utils && node test-weather-station-protocol.js
```

### System Recovery

```bash
# Restart all services safely
npm run restart:all

# Emergency shutdown
npm run emergency:shutdown

# System health check
npm run health:full-check

# Restore from backup
npm run restore:from-backup
```

## Notes

- All APIs respond with JSON format
- Authentication required for write operations
- Use `--testTimeout=10000` for longer running tests
- Monitor logs in `logs/` directory for debugging
- Database operations use Drizzle ORM for safety
- WebSocket connections auto-reconnect with exponential backoff
- Cross-chain operations may take 2-15 seconds for finality
- Weather data validation includes physics-based checks
- System handles 20,000+ TPS with auto-scaling
- 170,340+ global sensors accessible for data collection

## Support

For issues or questions:

1. Check logs: `tail -f logs/app.log`
2. Run health checks: `npm run health:check`
3. Validate configuration: `npm run config:validate`
4. Test connectivity: Run appropriate test suite from `consolidated-tests/utils/`
