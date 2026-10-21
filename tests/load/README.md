# WRFCoin K6 Load Testing Infrastructure

## Overview

Comprehensive K6-based load testing infrastructure for the WRFCoin platform, designed to handle
high-throughput sensor data and validate system performance under realistic load conditions. This
infrastructure supports testing of 15+ microservices with a focus on **2,000 concurrent sensor posts
per minute**.

## 🎯 Key Features

- **High-Volume Sensor Testing**: Simulate 2k+ concurrent IoT sensor posts
- **Multi-Service Integration**: Test all 15+ WRFCoin microservices
- **Real-Time Monitoring**: Integration with Prometheus, Grafana, and InfluxDB
- **Performance Baselines**: Comprehensive thresholds and SLI/SLO tracking
- **Automated Reporting**: HTML and JSON reports with detailed metrics
- **Progressive Load Testing**: Find system breaking points systematically

## 🏗️ Architecture

```
tests/load/
├── k6.config.js                    # Main K6 configuration
├── run-load-tests.sh               # Execution script
├── scenarios/                      # Test scenarios
│   ├── sensor-flood-test.js         # 2k sensor posts/minute
│   ├── cross-service-integration.js # Multi-service workflows
│   ├── database-stress.js           # Database performance
│   └── progressive-load.js          # Breaking point analysis
├── data/                           # Test data generators
│   └── sensor-data-generator.js     # Realistic weather data
├── utils/                          # Helper utilities
│   └── test-helpers.js              # Common functions
├── monitoring/                     # Monitoring integration
│   ├── k6-prometheus-config.js      # Prometheus integration
│   └── performance-monitoring.js    # APM and system monitoring
└── reports/                        # Generated reports
```

## 🚀 Quick Start

### Prerequisites

1. **K6 Installation**

   ```bash
   # Install K6
   curl -sSfL https://github.com/grafana/k6/releases/latest/download/k6-linux-amd64.tar.gz | tar -xz --strip-components=1 -C /usr/local/bin/

   # Verify installation
   k6 version
   ```

2. **WRFCoin Services Running**

   ```bash
   # Start all WRFCoin services
   cd $HOME/wrfcoin
   docker-compose up -d

   # Verify services are healthy
   curl http://localhost:4001/health  # Main API
   curl http://localhost:3002/health  # IoT Sensors
   curl http://localhost:4000/health  # Wallet
   ```

3. **Monitoring Stack (Optional)**
   ```bash
   # Start monitoring
   docker-compose -f infra/monitoring/docker-compose.monitoring.yml up -d
   ```

### Basic Usage

```bash
# Navigate to load testing directory
cd $HOME/wrfcoin/tests/load

# Run sensor flood test (2k posts/minute)
./run-load-tests.sh sensor-flood development

# Run all test scenarios
./run-load-tests.sh all development

# Run smoke test (basic health check)
./run-load-tests.sh smoke
```

## 📊 Test Scenarios

### 1. Sensor Flood Test (`sensor-flood-test.js`)

**Primary objective**: Validate 2,000 concurrent sensor posts per minute

```bash
# Execution
./run-load-tests.sh sensor-flood

# Target metrics
- Rate: 2,000 sensor posts/minute
- Duration: 15 minutes sustained
- Response time: P95 < 100ms
- Error rate: < 0.5%
```

**Key Features**:

- Realistic weather data generation with geographic distribution
- Progressive ramp-up to target load
- Quality validation of sensor data under load
- Database insertion and Redis cache testing

### 2. Cross-Service Integration (`cross-service-integration.js`)

**Objective**: Test multi-service workflows under load

```bash
# Execution
./run-load-tests.sh cross-service

# Workflow stages
1. Weather data submission (IoT → Database)
2. Real-time reward calculation (Weather Earnings)
3. Wallet transaction processing (Main API → Wallet)
4. Cross-chain bridge operations (Cross-Chain Protocol)
5. Carbon credit processing (if applicable)
6. Distributed computing jobs (Compute Networks)
```

### 3. Database Stress Test (`database-stress.js`)

**Objective**: PostgreSQL and Redis performance under high load

```bash
# Execution
./run-load-tests.sh database-stress

# Test areas
- Connection pool stress (up to 150 concurrent)
- Read-heavy operations (weather data queries)
- Write-intensive operations (sensor data ingestion)
- Redis cache effectiveness
- Transaction integrity validation
```

### 4. Progressive Load Test (`progressive-load.js`)

**Objective**: Find system breaking points systematically

```bash
# Execution
./run-load-tests.sh progressive-load

# Load progression
Phase 1: 10 VUs (baseline)
Phase 2: 25 VUs (light load)
Phase 3: 50 VUs (medium load)
Phase 4: 100 VUs (high load)
Phase 5: 200 VUs (stress load)
Phase 6: 300 VUs (breaking point)
Phase 7: 400 VUs (failure load)
```

## ⚙️ Configuration

### Environment Variables

Create `.env.k6.local` for custom configuration:

```bash
# Test Configuration
ENVIRONMENT=development
BASE_URL=http://localhost:4001
K6_SCENARIO=sensor-flood

# Service Endpoints
IOT_ENDPOINT=http://localhost:3002
WALLET_ENDPOINT=http://localhost:4000
BRIDGE_ENDPOINT=http://localhost:3000

# Load Test Parameters
SENSOR_FLOOD_RATE=2000
MAX_VUS=200
RAMP_UP_DURATION=2m

# Monitoring
MONITORING_ENABLED=true
K6_PROMETHEUS_ENABLED=true
K6_INFLUXDB_ENABLED=false
```

### Performance Thresholds

```javascript
// Core thresholds in k6.config.js
export const PERFORMANCE_THRESHOLDS = {
  http_req_duration: ['p(95)<800', 'p(99)<2000'],
  http_req_failed: ['rate<0.01'],
  iot_sensor_response: ['p(95)<100'],
  main_api_response: ['p(95)<200'],
  sensor_posts_per_minute: ['rate>=2000'],
};
```

## 📈 Monitoring Integration

### Prometheus Integration

```bash
# Enable Prometheus pushgateway
export K6_PROMETHEUS_ENABLED=true
export K6_PROMETHEUS_PUSHGATEWAY=http://localhost:9091

# Custom metrics automatically pushed:
- k6_sensor_posts_per_minute
- k6_database_query_duration_ms
- k6_service_health_score
- k6_system_cpu_usage_percent
```

### Grafana Dashboards

Access real-time monitoring at `http://localhost:3004`:

1. **K6 Load Testing Dashboard**

   - HTTP request duration trends
   - Virtual user count
   - Sensor post rate
   - Error rate percentage
   - Database performance
   - Service health scores

2. **System Resource Monitoring**
   - CPU, memory, disk usage
   - Network I/O
   - Database connections
   - Redis cache performance

### InfluxDB Time-Series Data

```bash
# Enable InfluxDB integration
export K6_INFLUXDB_ENABLED=true
export K6_INFLUXDB_URL=http://localhost:8086
export K6_INFLUXDB_DATABASE=k6_metrics

# Stored metrics:
- Request response times with percentiles
- System resource utilization over time
- Service-specific performance metrics
- Custom business metrics
```

## 🎛️ Advanced Usage

### Custom Test Scenarios

Create custom scenarios by extending base configuration:

```javascript
// Custom scenario example
import { LOAD_SCENARIOS } from '../k6.config.js';

export let options = {
  scenarios: {
    custom_sensor_test: {
      executor: 'constant-arrival-rate',
      rate: 3000, // 3k posts/minute
      timeUnit: '1m',
      duration: '20m',
      preAllocatedVUs: 150,
      maxVUs: 250,
    },
  },
};
```

### Multi-Environment Testing

```bash
# Development environment
./run-load-tests.sh sensor-flood development

# Staging environment
./run-load-tests.sh sensor-flood staging

# Production environment (with reduced load)
./run-load-tests.sh smoke production
```

### Debugging and Troubleshooting

```bash
# Enable debug logging
export K6_LOG_LEVEL=debug

# Run single iteration for debugging
k6 run scenarios/sensor-flood-test.js --iterations 1 --vus 1

# Check service health before testing
./run-load-tests.sh smoke
```

## 📋 Performance Baselines

### IoT Sensor Networks (Port 3002)

- **Target throughput**: 2,000 posts/minute
- **Response time**: P95 < 100ms, P99 < 200ms
- **Error rate**: < 0.5%
- **Data quality**: > 99% valid sensor readings

### Main API Server (Port 4001)

- **Response time**: P95 < 200ms, P99 < 500ms
- **Throughput**: 100+ requests/second
- **Error rate**: < 1%

### Database Performance

- **Query time**: P95 < 500ms
- **Connection pool**: < 90% utilization
- **Transaction success**: > 99%

### System Resources

- **CPU usage**: < 80%
- **Memory usage**: < 85%
- **Disk usage**: < 90%

## 🔧 Maintenance

### Regular Monitoring

1. **Weekly**: Run full test suite to establish performance trends
2. **Pre-deployment**: Run smoke tests to validate changes
3. **Post-deployment**: Run sensor flood test to verify performance
4. **Monthly**: Review and update performance baselines

### Report Analysis

```bash
# View latest reports
ls -la reports/

# Generate trend analysis
./scripts/analyze-performance-trends.sh

# Compare test runs
./scripts/compare-test-results.sh run1.json run2.json
```

### Updating Test Data

```bash
# Refresh test weather stations
node data/generate-test-stations.js --count 200

# Update sensor data patterns
node data/update-weather-patterns.js --seasons all
```

## 🚨 Alerting

### Performance Alerts

- **High Error Rate**: > 5% failed requests
- **Slow Response**: P95 response time > 2 seconds
- **System Overload**: CPU > 90% for 2+ minutes
- **Memory Pressure**: Memory usage > 90%

### Service Health Alerts

- **Service Down**: Health check failures
- **Database Issues**: Connection pool exhausted
- **Cache Problems**: Redis hit rate < 50%

## 📊 Sample Reports

### Sensor Flood Test Results

```
Test Duration: 15 minutes
Target Load: 2,000 sensor posts/minute
Actual Throughput: 2,047 posts/minute (102.4%)
Success Rate: 99.2%
Response Time P95: 87ms
Response Time P99: 143ms
Peak CPU Usage: 67%
Peak Memory Usage: 71%
Database Connections Peak: 45/100
Status: PASS ✅
```

### Performance Trend Analysis

```
7-Day Performance Trend:
- Sensor throughput: Stable at 2k+/min
- Response times: Slight increase (+5ms P95)
- Error rates: Consistent < 1%
- System resources: Stable utilization
- Recommendations: Monitor response time trend
```

## 🤝 Contributing

### Adding New Test Scenarios

1. Create scenario file in `scenarios/`
2. Add configuration to `SCENARIOS` in `run-load-tests.sh`
3. Update documentation
4. Test with smoke test first

### Extending Monitoring

1. Add custom metrics to `monitoring/k6-prometheus-config.js`
2. Update Grafana dashboard configuration
3. Document new metrics in this README

## 📞 Support

- **Issues**: Check service health and logs first
- **Performance**: Review resource utilization and thresholds
- **Configuration**: Verify environment variables and service endpoints
- **Monitoring**: Ensure Prometheus/Grafana are accessible

---

**Version**: 1.0.0  
**Last Updated**: $(date)  
**Maintained by**: WRFCoin Infrastructure Team
