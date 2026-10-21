# WRFCoin E2E Testing with Playwright

This directory contains the comprehensive end-to-end testing suite for the WRFCoin platform,
implemented using Playwright.

## 🏗️ Architecture

The E2E testing framework is designed to test the complete WRFCoin ecosystem across multiple
browsers and devices:

### Service Endpoints Tested

- **Frontend**: http://localhost:5001
- **Main API Server**: http://localhost:4001
- **Cross-Chain Protocol**: http://localhost:3000
- **Wallet Server**: http://localhost:4000
- **Compute Networks**: http://localhost:6000
- **IoT Sensor Networks**: http://localhost:3002
- **Governance Server**: http://localhost:8000

## 📁 Directory Structure

```
tests/e2e/
├── playwright.config.ts          # Main Playwright configuration
├── tsconfig.json                  # TypeScript configuration
├── package.json                   # E2E-specific dependencies
├── .env.test                      # Test environment variables
├──
├── workflows/                     # Test workflows organized by feature
│   ├── wallet/                    # Wallet operations tests
│   ├── trading/                   # Trading functionality tests
│   ├── weather-data/              # Weather data integration tests
│   ├── carbon-credits/            # Carbon credit trading tests
│   ├── defi/                      # DeFi operations tests
│   ├── governance/                # Governance workflow tests
│   ├── nft-marketplace/           # NFT marketplace tests
│   ├── cross-chain-bridge/        # Cross-chain bridge tests
│   ├── computational-jobs/        # Computational job tests
│   └── user-onboarding/           # User onboarding flow tests
│
├── page-objects/                  # Page Object Model implementation
│   ├── common/                    # Shared components
│   │   └── base-page.ts           # Base page object class
│   ├── wallet/                    # Wallet-specific page objects
│   │   └── wallet-page.ts         # Wallet page implementation
│   ├── dashboard/                 # Dashboard page objects
│   ├── nft/                      # NFT page objects
│   ├── defi/                     # DeFi page objects
│   ├── governance/               # Governance page objects
│   └── settings/                 # Settings page objects
│
├── fixtures/                      # Test fixtures and setup
│   └── wallet-fixtures.ts         # Wallet test fixtures
│
├── test-data/                     # Test data and constants
│   └── test-constants.ts          # Shared test constants
│
├── utils/                         # Utility functions and helpers
│   ├── global-setup.ts           # Global test setup
│   ├── global-teardown.ts        # Global test teardown
│   └── test-helpers.ts           # Test utility functions
│
└── reports/                       # Test reports and artifacts
    ├── html-report/              # HTML test reports
    ├── junit-report.xml          # JUnit XML reports
    └── results.json              # JSON test results
```

## 🚀 Getting Started

### Prerequisites

- Node.js 18+
- NPM or Yarn
- Docker (for services)

### Installation

1. **Install dependencies:**

   ```bash
   cd tests/e2e
   npm install
   ```

2. **Install Playwright browsers:**

   ```bash
   npm run install:browsers
   ```

3. **Set up environment variables:**
   ```bash
   cp ../../.env.test.example ../../.env.test
   # Edit .env.test with your specific configuration
   ```

## 🧪 Running Tests

### Basic Test Execution

```bash
# Run all E2E tests
npm run test:e2e

# Run tests with UI (interactive mode)
npm run test:e2e:ui

# Run tests in headed mode (see browser)
npm run test:e2e:headed

# Debug tests step by step
npm run test:e2e:debug
```

### Feature-Specific Tests

```bash
# Test specific workflows
npm run test:e2e:wallet           # Wallet operations
npm run test:e2e:defi             # DeFi functionality
npm run test:e2e:nft              # NFT marketplace
npm run test:e2e:governance       # Governance workflows
npm run test:e2e:carbon           # Carbon credits
npm run test:e2e:bridge           # Cross-chain bridge
npm run test:e2e:weather          # Weather data integration
npm run test:e2e:computational    # Computational jobs
npm run test:e2e:onboarding       # User onboarding
```

EOF < /dev/null
