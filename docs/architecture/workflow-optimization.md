# Wrfcoin Core Workflow Optimization

This document explains the implementation of the workflow optimization strategies outlined in the
workflow-architecture.md document. We've implemented Phase 1 and Phase 2 of the optimization plan to
manage our system of 58+ distinct workflow components.

## Phase 1: Dependency Management and Health Checks

This phase focused on:

- Basic dependency graph modeling
- Health check implementations
- Conditional startup logic

## Phase 2: Structural Improvements

This phase added:

- Advanced component grouping
- Parallel execution framework
- Detailed metrics collection and visualization

## Implemented Files

### Phase 1 Files

1. **utils/workflow-manager.js**

   - Core dependency graph management
   - Component grouping logic
   - Health check utilities
   - Conditional service startup

2. **scripts/optimized-startup.js**

   - Main entry point for the optimized workflow
   - Dynamic execution plan based on system state
   - Parallel execution of independent components
   - Startup metrics collection

3. **server/health.js**

   - Health check endpoints for the API server
   - Database connectivity verification
   - System resource monitoring
   - Metrics collection for service health

4. **server/server-with-health.js**

   - Enhanced API server with health endpoints
   - Graceful shutdown handling
   - Startup/shutdown metrics recording
   - Resource usage tracking

5. **server/ws_server_with_health.js**
   - Enhanced WebSocket server with health endpoints
   - Connection tracking and management
   - Graceful shutdown for WebSocket connections
   - Client status monitoring

### Phase 2 Files

1. **utils/component-scheduler.js**

   - Advanced component grouping system
   - Parallel execution framework
   - Real-time metrics collection
   - Resource-aware task scheduling
   - Critical path analysis

2. **scripts/advanced-startup.js**

   - Enhanced startup with component grouping
   - Intelligent resource allocation
   - Metrics-driven execution planning
   - Timeout and error handling

3. **utils/metrics-dashboard.js**

   - Real-time metrics visualization
   - Interactive web dashboard for monitoring
   - Historical data comparison
   - Performance analysis tools

4. **scripts/workflow-monitor.js**
   - Interactive CLI for managing workflows
   - Component status monitoring
   - Command-based workflow management
   - Integration with metrics dashboard

## How to Use

### Standard Optimization (Phase 1)

Instead of manually starting each component, you can use the optimized startup script:

```bash
node scripts/optimized-startup.js [mode]
```

Where `mode` is either `testnet` (default) or `mainnet`.

This will:

1. Check what components are already running
2. Create an optimal execution plan
3. Start only the necessary components
4. Monitor health of all services
5. Record metrics for analysis

### Advanced Optimization (Phase 2)

For enhanced performance with component grouping and metrics:

```bash
node scripts/advanced-startup.js [options]
```

Options include:

- `--mode=testnet|mainnet`: Network mode (default: testnet)
- `--skip-tests`: Skip running test components
- `--metrics-only`: Only collect metrics, don't start components
- `--groups=a,b,c`: Only start specified component groups

### Interactive Workflow Monitor

For a comprehensive CLI-based management interface:

```bash
node scripts/workflow-monitor.js
```

This provides an interactive shell with commands:

- `status`: Show status of all components
- `start <component>`: Start a component or group
- `stop <component>`: Stop a component or group
- `restart <component>`: Restart a component or group
- `advanced`: Start advanced scheduler
- `dashboard`: Open metrics dashboard
- And more...

### Metrics Dashboard

Access the real-time metrics dashboard at:

```
http://localhost:5002
```

This web interface shows:

- Real-time component status
- Startup performance metrics
- Historical comparison data
- Critical path analysis
- Performance bottlenecks

### Health Monitoring

You can check the health of various components:

**API Server Health:**

- Basic: http://localhost:3000/health
- Database: http://localhost:3000/health/database
- Complete: http://localhost:3000/health/complete

**WebSocket Server Health:**

- Basic: http://localhost:5001/health
- Connections: http://localhost:5001/health/connections

### Startup Metrics

Metrics are recorded in the `logs` directory:

- `api-server-startup.jsonl`: API server startup events
- `ws-server-startup.jsonl`: WebSocket server startup events
- `startup-metrics-[timestamp].json`: Comprehensive startup metrics
- `health-checks.jsonl`: Health check request logs

## Benefits

This optimization provides several benefits:

1. **Faster Startup**: By only starting what's needed and parallelizing independent components
2. **Improved Reliability**: Health checks ensure services are truly ready before dependent services
   start
3. **Better Monitoring**: Comprehensive health data and metrics for troubleshooting
4. **Resource Efficiency**: Skip unnecessary work when components are already running
5. **Graceful Shutdown**: Proper cleanup when services are stopped

## Phase 3: Category-Based Workflow Organization

We've successfully implemented Phase 3 of our optimization plan by organizing our 58+ workflows into
7 logical categories. This organization enables more efficient startup, better resource allocation,
and clearer dependency management.

### Workflow Category Structure

Our 7 core workflow categories are:

#### 1. Core System (`coreSystem`)

- **Purpose**: Manages critical infrastructure services necessary for basic platform operation
- **Components**:
  - API Server
  - WebSocket Server
  - Blockchain Core
  - Mainnet/Testnet Node
  - RPC Services
- **Dependencies**: Minimal external dependencies; most components in this category are foundational
- **Startup Priority**: Highest (always starts first)

#### 2. Database Management (`databaseManagement`)

- **Purpose**: Handles schema changes, migrations, and database connectivity
- **Components**:
  - Database Push
  - DB Migrate
  - DB Migrate Apply
  - Schema Generation
  - Migration Creation
- **Dependencies**: Core System components
- **Startup Priority**: Second (after Core System)

#### 3. Data Seeding (`dataSeeding`)

- **Purpose**: Populates essential data for the system to function properly
- **Components**:
  - Seed Achievements
  - Seed Governance
  - Seed Rewards
  - Seed Validators
  - Setup Governance DB
- **Dependencies**: Database Management components
- **Startup Priority**: Third (after database is ready)

#### 4. Specialized Testing (`specializedTesting`)

- **Purpose**: Tests blockchain-specific features and operations
- **Components**:
  - Test Governance
  - Test Incentives
  - Test PoP (Proof of Purpose)
  - Test Privacy
  - Test DB Migration
- **Dependencies**: Core System and Database Management
- **Startup Priority**: Moderate (can be skipped in production)

#### 5. Cross-Chain Testing (`crossChainTesting`)

- **Purpose**: Tests integration with external blockchains and protocols
- **Components**:
  - Test Cross-Chain
  - Test Cross-Chain Providers
  - Test LayerZero
  - Test Axelar
  - Test Wormhole
  - Test Near Protocol
- **Dependencies**: Core System
- **Startup Priority**: Lower (typically only needed during development)

#### 6. Computational and Storage (`computationalAndStorage`)

- **Purpose**: Tests computation-focused blockchain features
- **Components**:
  - Test Computational DB
  - Test Fixed Database
  - Test TypeScript DB
  - Test Computational Blocks
  - Test Bacalhau
  - Test BigchainDB
  - Test Golem
  - Test SONM
  - Test Storj
- **Dependencies**: Core System and Database Management
- **Startup Priority**: Low (typically only during development or specialized testing)

#### 7. Weather and IoT (`weatherAndIoT`)

- **Purpose**: Tests weather data processing and IoT device integration
- **Components**:
  - Test Weather Data Validation
  - Test Weather Station Protocol
  - Test Forecast Compute Node
  - Test IoT Devices
  - Test Sensor.Community
  - Test FOAM
  - Test TTN
  - Test Luftdaten
- **Dependencies**: Core System, Database Management, and Computational systems
- **Startup Priority**: Lowest (typically only for specialized testing)

### Benefits of Category-Based Organization

1. **Logical Grouping**: Components with similar functions are grouped together
2. **Simplified Startup**: Categories can be started as units rather than individual components
3. **Clear Dependencies**: Dependencies between categories are well-defined
4. **Resource Optimization**: Similar components can share resources efficiently
5. **Testing Flexibility**: Test categories can be skipped in production environments
6. **Startup Prioritization**: Critical systems start first, tests start last

## Next Steps

As we continue evolving our workflow optimization, the next phases include:

### Phase 4: Enterprise Scaling

- Global distribution capabilities
- Multi-region test orchestration
- Edge computing optimization for IoT and weather station workflows
- Load-balanced computational workflow distribution
- Auto-scaling for validator and forecast compute workloads

## Technical Notes

### Adding New Components

When adding new components to the system:

1. Add the component to the dependency graph in `utils/workflow-manager.js`
2. Map the component to its workflow name
3. Add the component script command
4. Add any necessary port information if it's a service
5. Assign the component to one of the 7 established categories:
   - Core System Workflows
   - Database Management
   - Data Seeding
   - Specialized Testing
   - Cross-Chain Testing
   - Computational and Storage Network Testing
   - Weather and IoT Testing

### Modifying Existing Components

When modifying components:

1. Update any dependencies in the dependency graph
2. Ensure health check endpoints are implemented
3. Update the metrics collection if needed
4. Test parallel startup with related components
5. Update component categorization if functionality has changed
6. Verify proper operation within workflow groups

### Troubleshooting

If components fail to start:

1. Check the health endpoints for detailed status
2. Examine the startup metrics in the logs directory
3. Verify that ports are available for services
4. Ensure all dependencies are satisfied before starting a component
5. Check category-specific issues:
   - **Core System**: Verify database connections and blockchain state
   - **Database Management**: Test database availability and migration status
   - **Cross-Chain**: Check external network configurations
   - **Computational**: Verify data-aware block structure compatibility
   - **Weather/IoT**: Test sensor data ingestion endpoints
6. Use the workflow group status commands to see failures by category
7. Check for cross-component dependency issues between workflow groups
