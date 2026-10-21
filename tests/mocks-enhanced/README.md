# WRFCoin Enhanced Mock Services Harness

A comprehensive testing environment providing advanced mock services for the WRFCoin platform. This
enhanced harness simulates PostgreSQL, Blockchain RPC, Weather Oracle, Carbon Credit Oracle, and
additional services with realistic data, failure simulation, and comprehensive monitoring
capabilities.

## 🏗️ Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                Enhanced Mock Services Harness               │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   Blockchain │  │    Weather   │  │    Carbon    │      │
│  │   RPC Mock   │  │ Oracle Mock  │  │ Oracle Mock  │      │
│  │   Enhanced   │  │   Enhanced   │  │   Enhanced   │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  PostgreSQL  │  │    Redis     │  │ MockServer   │      │
│  │ Primary/Rep  │  │    Cache     │  │   Advanced   │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                             │
│  ┌──────────────┐  ┌──────────────┐                        │
│  │    Health    │  │ Integration  │                        │
│  │  Dashboard   │  │ Test Runner  │                        │
│  └──────────────┘  └──────────────┘                        │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│         Docker Compose Orchestration & Networking          │
└─────────────────────────────────────────────────────────────┘
```

## 📋 Features

### Core Mock Services

- **Enhanced Blockchain RPC**: Full Ethereum JSON-RPC implementation with WebSocket support
- **Enhanced Weather Oracle**: 50 weather stations across 8 climate zones with real-time data
- **Enhanced Carbon Credit Oracle**: 25 carbon projects, trading platform, and offset calculations
- **Health Dashboard**: Real-time monitoring with auto-refresh and analytics
- **Integration Test Runner**: Comprehensive test suite automation

### Advanced Capabilities

- **Failure Simulation**: Controlled chaos engineering and error injection
- **WebSocket Support**: Real-time data streaming for weather and blockchain
- **Historical Data**: Time-series data for pricing and weather patterns
- **Performance Testing**: Load testing and scalability validation
- **Multi-Environment**: Development, staging, and production profiles

## 🚀 Quick Start

### Prerequisites

- Docker 20.10+ and Docker Compose 2.0+
- Node.js 18+ (for local development)
- Available ports: 3010, 3020, 4000, 5000, 8545, 8546

### Launch Full Environment

```bash
# Navigate to project root
cd /path/to/wrfcoin

# Start all enhanced mock services
docker-compose -f tests/docker-compose.test.yml up -d

# Verify services are healthy
curl http://localhost:4000/api/health

# Access health dashboard
open http://localhost:4000
```

### Launch Specific Profiles

```bash
# Testing profile (core services only)
docker-compose -f tests/docker-compose.test.yml --profile testing up -d

# Full profile (all services + monitoring)
docker-compose -f tests/docker-compose.test.yml --profile full up -d

# Blockchain profile (blockchain services only)
docker-compose -f tests/docker-compose.test.yml --profile blockchain up -d

# Weather profile (weather services only)
docker-compose -f tests/docker-compose.test.yml --profile weather up -d

# Carbon profile (carbon services only)
docker-compose -f tests/docker-compose.test.yml --profile carbon up -d

# Failure simulation profile
docker-compose -f tests/docker-compose.test.yml --profile failure up -d

# Chaos engineering profile
docker-compose -f tests/docker-compose.test.yml --profile chaos up -d

# Monitoring profile
docker-compose -f tests/docker-compose.test.yml --profile monitoring up -d
```

## 🔧 Service Configuration

### Environment Variables

```bash
# Blockchain RPC Configuration
BLOCKCHAIN_PORT=8545
BLOCKCHAIN_WS_PORT=8546
ENABLE_MINING=true
BLOCK_TIME=2000
ENABLE_FAILURE_SIMULATION=false

# Weather Oracle Configuration
WEATHER_PORT=3010
WEATHER_WS_PORT=3011
STATION_COUNT=50
CLIMATE_ZONES=8
ENABLE_EXTREME_EVENTS=true

# Carbon Oracle Configuration
CARBON_PORT=3020
CARBON_TRACKING_PORT=3021
PROJECT_COUNT=25
CARBON_PRICE_VOLATILITY=medium

# Health Dashboard Configuration
HEALTH_PORT=4000
MONITORING_INTERVAL=30000

# Integration Test Runner Configuration
TEST_RUNNER_PORT=5000
```

## 📡 API Endpoints

### Health Dashboard (Port 4000)

- **GET /** - Web dashboard with real-time monitoring
- **GET /api/health** - Overall system health status
- **GET /api/health/:serviceName** - Specific service health
- **GET /api/health/:serviceName/history** - Service health history

### Enhanced Blockchain RPC (Port 8545)

```bash
# Standard Ethereum JSON-RPC
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# WebSocket connection
wscat -c ws://localhost:8546

# Health check
curl http://localhost:8545/health

# Mining control
curl -X POST http://localhost:8545/mining/start
curl -X POST http://localhost:8545/mining/stop

# Failure simulation
curl -X POST http://localhost:8545/simulate/network-delay \
  -H "Content-Type: application/json" \
  -d '{"delay": 1000}'
```

### Enhanced Weather Oracle (Port 3010)

```bash
# List all weather stations
curl http://localhost:3010/api/weather/stations

# Get station details
curl http://localhost:3010/api/weather/stations/WS001

# Get current weather data
curl http://localhost:3010/api/weather/stations/WS001/current

# Get historical data
curl http://localhost:3010/api/weather/stations/WS001/historical?days=7

# WebSocket real-time updates
wscat -c ws://localhost:3011

# Simulate extreme weather event
curl -X POST http://localhost:3010/api/weather/simulate/extreme \
  -H "Content-Type: application/json" \
  -d '{"type": "hurricane", "stationId": "WS001", "duration": 3600}'
```

### Enhanced Carbon Credit Oracle (Port 3020)

```bash
# List carbon projects
curl http://localhost:3020/api/carbon/projects

# Get project details
curl http://localhost:3020/api/carbon/projects/CCProject-0001

# Calculate carbon footprint
curl -X POST http://localhost:3020/api/carbon/calculate \
  -H "Content-Type: application/json" \
  -d '{
    "activities": [
      {"type": "electricity", "fuel": "grid_average_us", "amount": 1000, "unit": "kWh"},
      {"type": "transportation", "fuel": "car_gasoline", "amount": 500, "unit": "km"}
    ]
  }'

# Purchase carbon credits
curl -X POST http://localhost:3020/api/carbon/credits/purchase \
  -H "Content-Type: application/json" \
  -d '{
    "projectId": "CCProject-0001",
    "quantity": 10,
    "buyerInfo": {"name": "Test Buyer", "email": "test@example.com"}
  }'

# Track carbon credit by serial number
curl http://localhost:3021/api/track/WRF123ABC456

# Market analytics
curl http://localhost:3020/api/carbon/market/analytics?period=30d
```

### Integration Test Runner (Port 5000)

```bash
# Get available test suites
curl http://localhost:5000/api/test/suites

# Run smoke tests
curl -X POST http://localhost:5000/api/test/run/smoke

# Run integration tests
curl -X POST http://localhost:5000/api/test/run/integration

# Run load tests
curl -X POST http://localhost:5000/api/test/run/load

# Run chaos tests
curl -X POST http://localhost:5000/api/test/run/chaos

# Run end-to-end tests
curl -X POST http://localhost:5000/api/test/run/e2e

# Run all test suites
curl -X POST http://localhost:5000/api/test/run-all

# Get test status
curl http://localhost:5000/api/test/status

# Get test results
curl http://localhost:5000/api/test/results/run-1-1234567890

# Get test history
curl http://localhost:5000/api/test/history
```

## 🧪 Testing Scenarios

### Smoke Tests

Basic connectivity and health validation:

- Service health checks
- API endpoint accessibility
- Basic functionality verification

### Integration Tests

Cross-service communication validation:

- Weather data to blockchain integration
- Carbon credit purchase flow
- Cross-service data consistency

### Load Tests

Performance and scalability validation:

- Concurrent request handling
- Response time analysis
- Throughput measurement

### Chaos Tests

Failure simulation and resilience validation:

- Error handling verification
- Data validation resilience
- Service recovery testing

### End-to-End Tests

Complete user journey simulation:

- Weather data collection
- Carbon footprint calculation
- Credit purchase and blockchain recording

## 📊 Monitoring and Analytics

### Health Dashboard Features

- Real-time service status monitoring
- Historical health data
- Response time analytics
- Auto-refresh every 30 seconds
- Mobile-responsive design

### Test Analytics

- Test execution history
- Performance metrics
- Failure analysis
- Trend identification

### Service Metrics

- Request/response statistics
- Error rates and patterns
- Resource utilization
- Performance benchmarks

## 🔧 Development Usage

### Local Development

```bash
# Install dependencies
cd tests/mocks-enhanced
npm install

# Start individual services
npm run start:blockchain    # Blockchain RPC on 8545
npm run start:weather      # Weather Oracle on 3010
npm run start:carbon       # Carbon Oracle on 3020
npm run start:health       # Health Dashboard on 4000
npm run start:tests        # Test Runner on 5000

# Start all services concurrently
npm run dev

# Run specific tests
npm run test:smoke
npm run test:integration
npm run test:load
npm run test:chaos
npm run test:e2e
npm run test:all
```

### Docker Development

```bash
# Build and start services
docker-compose -f tests/docker-compose.test.yml build
docker-compose -f tests/docker-compose.test.yml up -d

# View logs
docker-compose -f tests/docker-compose.test.yml logs -f

# Scale specific services
docker-compose -f tests/docker-compose.test.yml up -d --scale weather-oracle-mock-enhanced=3

# Stop services
docker-compose -f tests/docker-compose.test.yml down
```

## 🎯 Test Profiles

### Development Profile

Basic services for development:

- PostgreSQL primary
- Redis cache
- Core mock services

### Testing Profile

Comprehensive testing environment:

- All mock services
- Health monitoring
- Test runner

### Full Profile

Complete environment with monitoring:

- All services
- Advanced monitoring
- Analytics

### Failure Profile

Chaos engineering environment:

- Failure simulation enabled
- Error injection
- Resilience testing

## 🔍 Troubleshooting

### Common Issues

**Services not starting:**

```bash
# Check port availability
lsof -i :8545 -i :3010 -i :3020 -i :4000 -i :5000

# Check Docker logs
docker-compose -f tests/docker-compose.test.yml logs service-name
```

**Health checks failing:**

```bash
# Verify service endpoints
curl http://localhost:4000/health
curl http://localhost:8545/health
curl http://localhost:3010/health

# Check service status
docker-compose -f tests/docker-compose.test.yml ps
```

**Test failures:**

```bash
# Check test runner status
curl http://localhost:5000/api/test/status

# View detailed test results
curl http://localhost:5000/api/test/history

# Check service health
curl http://localhost:4000/api/health
```

### Debug Mode

Enable verbose logging:

```bash
# Set debug environment variables
export DEBUG=wrfcoin:*
export LOG_LEVEL=debug

# Start services with debug output
docker-compose -f tests/docker-compose.test.yml up --build
```

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Implement changes with tests
4. Run the test suite: `npm run test:all`
5. Submit a pull request

### Development Guidelines

- Follow existing code style
- Add comprehensive tests
- Update documentation
- Ensure all services pass health checks

## 📄 License

MIT License - see LICENSE file for details

## 🔗 Related Documentation

- [WRFCoin Platform Architecture](../../docs/architecture.md)
- [Backend Services Guide](../../backend/README.md)
- [Smart Contracts Documentation](../../smart-contracts/README.md)
- [Frontend Development Guide](../../frontend/README.md)

---

**WRFCoin Enhanced Mock Services Harness v1.0.0** _Comprehensive testing environment for blockchain
weather forecasting platform_
