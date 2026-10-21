# WRFCoin Contract Testing & Version Management

This directory contains the comprehensive Pact-based contract testing infrastructure for WRFCoin,
featuring advanced version management, compatibility testing, and automated workflow orchestration.

## 🏗️ Architecture Overview

The system is built on Agent 1's enhanced Pact framework and extends it with production-ready
versioning and compatibility testing capabilities:

```
┌─────────────────────────────────────────────────────────────────┐
│                 Contract Testing Architecture                    │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────────┐    ┌─────────────────┐    ┌──────────────┐ │
│  │   Consumer      │    │  Pact Broker    │    │  Providers   │ │
│  │  (WRFCoin API)  │◄──►│  (Versioning)   │◄──►│ (Services)   │ │
│  └─────────────────┘    └─────────────────┘    └──────────────┘ │
│           │                       │                       │     │
│           ▼                       ▼                       ▼     │
│  ┌─────────────────┐    ┌─────────────────┐    ┌──────────────┐ │
│  │ Contract Tests  │    │ Version Matrix  │    │ Verification │ │
│  │ Generation      │    │ Compatibility   │    │ & Validation │ │
│  └─────────────────┘    └─────────────────┘    └──────────────┘ │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## 📁 Directory Structure

```
tests/contract/
├── README.md                          # This documentation
├── docker-compose.pact.yml           # Pact Broker infrastructure
├── jest.contract.config.js           # Jest configuration for contract tests
│
├── setup/                            # Test environment setup
│   ├── global.setup.js               # Global test setup
│   ├── global.teardown.js            # Global test teardown
│   └── pact.setup.js                 # Enhanced Pact configuration
│
├── scripts/                          # Version management & workflow scripts
│   ├── publish-pacts.js               # Contract publishing with versioning
│   ├── can-i-deploy.js               # Deployment compatibility validation
│   ├── verify-pacts.js               # Provider verification with state management
│   ├── compatibility-matrix.js       # Cross-version compatibility testing
│   └── workflow-orchestrator.js      # End-to-end workflow automation
│
├── mocks/                            # Mock service implementations
│   ├── mock-blockchain-rpc.js        # Blockchain RPC mock
│   ├── mock-weather-api.js          # Weather API mock
│   └── mock-carbon-oracle.js        # Carbon oracle mock
│
├── integration/                      # Integration test scenarios
│   └── enhanced-contract-validation.test.js
│
├── consumers/                        # Consumer test implementations
├── providers/                        # Provider test implementations
├── config/                          # Test configuration
├── test-data/                       # SQL seed data
├── pacts/                           # Generated Pact files
├── reports/                         # Test reports & analytics
└── artifacts/                       # Workflow artifacts & logs
```

## 🚀 Quick Start

### 1. Start the Pact Infrastructure

```bash
# Start Pact Broker and supporting services
npm run pact:start-broker

# Verify services are healthy
curl http://localhost:9292/diagnostic/status/heartbeat
```

### 2. Generate Contract Tests

```bash
# Run contract tests to generate Pact files
npm run test:contract

# View generated contracts
ls tests/contract/pacts/
```

### 3. Publish Contracts with Versioning

```bash
# Publish contracts to Pact Broker with semantic versioning
npm run test:contract:publish

# Publish specific strategy (git-semver, git-sha, package-json, env)
PACT_VERSION_STRATEGY=git-semver npm run test:contract:publish
```

### 4. Run Compatibility Matrix Testing

```bash
# Test compatibility across all API versions
npm run test:contract:compatibility

# Use specific strategy
npm run test:contract:compatibility -- --strategy=full-matrix

# Parallel execution for faster testing
npm run test:contract:compatibility -- --parallelism=8
```

### 5. Validate Deployment Readiness

```bash
# Check if current version can be deployed
npm run test:contract:can-i-deploy

# Check deployment to specific environment
npm run test:contract:can-i-deploy -- --environment=production
```

### 6. Run Complete Workflow

```bash
# Execute end-to-end contract testing workflow
npm run test:contract:workflow

# Dry run (validation only)
npm run test:contract:workflow:dry-run

# Single stage execution
npm run test:contract:workflow -- --stage=compatibility-testing
```

## 🔧 Configuration

### Environment Variables

```bash
# Pact Broker Configuration
PACT_BROKER_BASE_URL=http://localhost:9292
PACT_BROKER_USERNAME=admin
PACT_BROKER_PASSWORD=          # Set via .env — NEVER hardcode

# Versioning Strategy
PACT_VERSION_STRATEGY=git-semver  # git-semver, git-sha, package-json, env
PACT_CONSUMER_VERSION=1.2.0      # Explicit version (when using env strategy)
PACT_CONSUMER_TAG=staging        # Environment tag

# Compatibility Testing
PACT_MATRIX_STRATEGY=supported-matrix  # full-matrix, supported-matrix, breaking-change-matrix
PACT_WORKFLOW_FAIL_FAST=true
PACT_WORKFLOW_RETRIES=2

# CI/CD Integration
CI=true                          # Enable CI mode features
PACT_NOTIFICATIONS=true          # Enable notifications
SLACK_WEBHOOK_URL=               # Slack notifications
TEAMS_WEBHOOK_URL=               # Microsoft Teams notifications

# Artifact Management
ARTIFACT_UPLOAD=true
ARTIFACT_DESTINATION=s3://wrfcoin-pact-artifacts
```

### Package.json Scripts

```bash
# Core Contract Testing
npm run test:contract              # Run all contract tests
npm run test:contract:consumer     # Consumer-focused tests
npm run test:contract:provider     # Provider-focused tests

# Version Management
npm run test:contract:publish      # Publish contracts with versioning
npm run test:contract:verify       # Verify provider contracts
npm run test:contract:can-i-deploy # Deployment compatibility check

# Advanced Testing
npm run test:contract:compatibility # Cross-version compatibility matrix
npm run test:contract:workflow     # Complete automated workflow

# Infrastructure Management
npm run pact:start-broker          # Start Pact Broker
npm run pact:stop-broker           # Stop Pact Broker
npm run pact:reset-broker          # Restart Pact Broker
```

## 📊 Version Management

### Semantic Versioning Strategy

The system supports multiple versioning strategies:

#### Git-Based Semver (Recommended)

```bash
PACT_VERSION_STRATEGY=git-semver
# Uses git tags with fallback to commit-based versioning
# Examples: 1.2.0, 1.2.0-5-g7a8b9c, 2.0.0-beta.1
```

#### Git SHA-Based

```bash
PACT_VERSION_STRATEGY=git-sha
# Uses git commit SHA with timestamp
# Examples: 2024-07-22T06-30-15-g7a8b9c
```

#### Package.json

```bash
PACT_VERSION_STRATEGY=package-json
# Uses version from package.json
# Examples: 1.2.0 (from package.json)
```

#### Environment Variable

```bash
PACT_VERSION_STRATEGY=env
PACT_CONSUMER_VERSION=1.2.0
# Uses explicit version from environment
```

### Environment Tagging

Automatic environment detection based on Git branch:

```bash
main/master     → production  (stable, prod)
release/*       → staging     (staging, test, release)
feature/*       → development (dev, feature)
develop         → development (dev, develop)
```

### Version Compatibility Rules

```javascript
// Semantic versioning rules
{
  allowPatchUpdates: true,     // 1.0.0 → 1.0.1 ✅
  allowMinorUpdates: true,     // 1.0.0 → 1.1.0 ✅
  allowMajorUpdates: false,    // 1.0.0 → 2.0.0 ❌
  allowPreReleaseUpdates: false // 1.0.0 → 1.1.0-beta ❌
}
```

## 🔄 Compatibility Testing

### Matrix Testing Strategies

#### Full Matrix

Tests all consumer versions against all provider versions:

```bash
npm run test:contract:compatibility -- --strategy=full-matrix
```

#### Supported Matrix (Default)

Tests only supported version combinations:

```bash
npm run test:contract:compatibility -- --strategy=supported-matrix
```

#### Breaking Change Matrix

Focuses on major version compatibility:

```bash
npm run test:contract:compatibility -- --strategy=breaking-change-matrix
```

### Compatibility Matrix Example

```
Consumer\Provider  │ PG 1.0.0 │ PG 1.1.0 │ BC 1.0.0 │ BC 1.1.0 │
─────────────────────────────────────────────────────────────────
API 1.0.0         │    ✅     │    ✅     │    ✅     │    ⚠️     │
API 1.1.0         │    ✅     │    ✅     │    ✅     │    ✅     │
API 2.0.0-beta    │    ❌     │    ⚠️     │    ❌     │    ✅     │
API 2.0.0         │    ❌     │    ❌     │    ⚠️     │    ✅     │

✅ Compatible  ⚠️ Compatible with warnings  ❌ Incompatible
```

### Breaking Change Detection

The system automatically detects and classifies breaking changes:

- **Missing Field**: Required field removed from response
- **Type Change**: Field data type modified
- **Status Change**: HTTP status code changed
- **Format Change**: Response structure modified
- **Endpoint Change**: URL path modified

## 🔍 Provider Verification

### State Management

Each provider supports state-based testing:

```javascript
// Database states
'database is empty'     → Clean database setup
'user exists'          → User creation with test data
'weather data exists'  → Weather data seeding

// Blockchain states
'blockchain is operational' → Mock blockchain setup
'wallet has balance'       → Wallet funding
'contract is deployed'     → Smart contract deployment

// Service states
'weather service is operational' → Weather API availability
'carbon data is available'      → Carbon oracle data setup
```

### Provider Configuration

```javascript
const providers = {
  'wrfcoin-postgres-db': {
    baseUrl: 'http://localhost:5434',
    stateChangeUrl: 'http://localhost:5434/pact/provider-states',
  },
  'wrfcoin-blockchain-rpc': {
    baseUrl: 'http://localhost:8545',
    stateChangeUrl: 'http://localhost:8545/pact/provider-states',
  },
  // ... additional providers
};
```

## 📈 Workflow Orchestration

### Automated Pipeline Stages

1. **Setup** - Environment validation and service startup
2. **Contract Generation** - Run tests to generate Pact files
3. **Contract Publishing** - Publish with version management
4. **Provider Verification** - Verify all provider contracts
5. **Compatibility Testing** - Cross-version matrix validation
6. **Deployment Validation** - Can-I-Deploy safety checks
7. **Reporting** - Comprehensive reports and notifications

### Workflow Configuration

```javascript
const stages = {
  setup: { timeout: 30000, required: true },
  'contract-generation': { timeout: 120000, required: true },
  'contract-publishing': { timeout: 60000, required: true },
  'provider-verification': { timeout: 300000, required: true, parallel: true },
  'compatibility-testing': { timeout: 600000, required: true },
  'deployment-validation': { timeout: 120000, required: true },
  reporting: { timeout: 60000, required: false },
};
```

### Rollback Capabilities

Automatic rollback on critical failures:

- Contract unpublishing (if supported)
- Service cleanup
- Artifact preservation
- Failure notifications

## 📊 Reports & Analytics

### Generated Reports

- **Compatibility Matrix Report**: Cross-version compatibility analysis
- **Verification Report**: Provider verification results
- **Deployment Validation Report**: Safety assessment
- **Workflow Report**: End-to-end execution summary
- **Trend Analysis**: Historical compatibility trends

### Report Formats

- **JSON**: Machine-readable detailed data
- **HTML**: Human-readable visual reports
- **Artifacts**: Logs, configurations, and metadata

### Report Locations

```
tests/contract/reports/
├── compatibility-matrix-1.2.0.html         # Visual compatibility report
├── compatibility-matrix-1.2.0.json         # Detailed JSON data
├── verify-postgres-1.2.0.json             # Provider verification results
├── can-i-deploy-1.2.0.json                # Deployment validation
└── workflow-report-workflow-123456.json    # Complete workflow summary
```

## 🔗 CI/CD Integration

### GitHub Actions Example

```yaml
name: Contract Testing
on: [push, pull_request]

jobs:
  contract-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '20'

      - name: Install dependencies
        run: npm ci

      - name: Start Pact infrastructure
        run: npm run pact:start-broker

      - name: Run contract workflow
        run: npm run test:contract:workflow
        env:
          CI: true
          PACT_BROKER_BASE_URL: http://localhost:9292
          PACT_NOTIFICATIONS: true
          SLACK_WEBHOOK_URL: ${{ secrets.SLACK_WEBHOOK_URL }}

      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        if: always()
        with:
          name: contract-test-reports
          path: tests/contract/reports/
```

### Jenkins Pipeline Example

```groovy
pipeline {
  agent any

  environment {
    PACT_BROKER_BASE_URL = 'http://pact-broker:9292'
    CI = 'true'
  }

  stages {
    stage('Contract Testing') {
      steps {
        sh 'npm run pact:start-broker'
        sh 'npm run test:contract:workflow'
      }

      post {
        always {
          archiveArtifacts artifacts: 'tests/contract/reports/**/*'
          publishHTML([
            allowMissing: false,
            alwaysLinkToLastBuild: true,
            keepAll: true,
            reportDir: 'tests/contract/reports',
            reportFiles: '*.html',
            reportName: 'Contract Test Report'
          ])
        }
      }
    }
  }
}
```

## 🔧 Troubleshooting

### Common Issues

#### Pact Broker Connection Failed

```bash
# Check broker status
curl http://localhost:9292/diagnostic/status/heartbeat

# Restart broker
npm run pact:reset-broker

# Check Docker containers
docker ps --filter "name=pact-broker"
```

#### Contract Generation Failed

```bash
# Run tests with verbose output
npm run test:contract -- --verbose

# Check for missing mock services
docker ps --filter "name=mock"

# Validate test configuration
node -e "console.log(require('./tests/contract/jest.contract.config.js'))"
```

#### Version Resolution Issues

```bash
# Check Git repository status
git status
git describe --tags --always

# Test version resolution
PACT_VERSION_STRATEGY=git-semver node -e "
  const { VersionManager } = require('./tests/contract/scripts/publish-pacts.js');
  console.log('Version:', VersionManager.getCurrentVersion());
"
```

#### Compatibility Test Failures

```bash
# Run single compatibility test
npm run test:contract:compatibility -- --strategy=supported-matrix --parallelism=1

# Check service availability
npm run test:contract:workflow -- --stage=setup

# Review compatibility report
open tests/contract/reports/compatibility-matrix-*.html
```

### Debug Mode

Enable debug logging:

```bash
# Verbose output
PACT_VERBOSE=true npm run test:contract:workflow

# Debug mode
DEBUG=pact:* npm run test:contract:workflow

# Workflow dry run
npm run test:contract:workflow:dry-run
```

### Log Locations

```
tests/contract/artifacts/
├── workflow-123456.log              # Workflow execution log
├── pact.log                        # Pact framework logs
└── compatibility-failure-*.json    # Error diagnostics
```

## 📚 Additional Resources

### Pact Documentation

- [Pact Foundation](https://docs.pact.io/)
- [Contract Testing Best Practices](https://docs.pact.io/getting_started/how_pact_works)
- [Version Management](https://docs.pact.io/pact_broker/branches)

### WRFCoin-Specific

- [Agent 1's Enhanced Pact Framework](./setup/pact.setup.js)
- [Mock Service Implementations](./mocks/)
- [Integration Test Examples](./integration/)

### Support

- Check [Issues](https://github.com/wrfcoin/wrfcoin/issues) for known problems
- Review [Workflow Logs](./artifacts/) for debugging
- Contact the development team for assistance

---

**Note**: This contract testing system was implemented as part of Task 42 (Subtasks 8-9) by Agent 2,
building on Agent 1's enhanced Pact framework foundation. The system provides production-ready
contract versioning, compatibility testing, and automated workflow orchestration for the WRFCoin
platform.
