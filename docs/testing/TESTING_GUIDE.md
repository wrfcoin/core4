# WRFCoin Testing Framework - Developer Guide

## Overview

This guide provides comprehensive documentation for using the WRFCoin testing framework, including
best practices, conventions, and troubleshooting tips for developers working with the codebase.

## Table of Contents

1. [Testing Architecture](#testing-architecture)
2. [Test Types](#test-types)
3. [Quick Start](#quick-start)
4. [Writing Tests](#writing-tests)
5. [Running Tests](#running-tests)
6. [Test Configuration](#test-configuration)
7. [Best Practices](#best-practices)
8. [Debugging Tests](#debugging-tests)
9. [CI/CD Integration](#ci-cd-integration)
10. [Troubleshooting](#troubleshooting)

## Testing Architecture

The WRFCoin testing framework is designed with multiple layers of testing:

```
┌─────────────────────────────────────────┐
│           End-to-End Tests              │
│         (Playwright Browser)            │
├─────────────────────────────────────────┤
│          Integration Tests              │
│        (GraphQL API, Database)          │
├─────────────────────────────────────────┤
│           Unit Tests                    │
│     (SDK, Utilities, Components)        │
├─────────────────────────────────────────┤
│        Smart Contract Tests            │
│          (Hardhat, Mocha)               │
└─────────────────────────────────────────┘
```

### Directory Structure

```
test/
├── e2e/                     # End-to-end tests
│   ├── workflows/           # User workflow tests
│   ├── utils/              # E2E test utilities
│   ├── fixtures/           # Test data
│   └── config/             # E2E configuration
├── integration/            # API integration tests
├── unit/                   # Unit tests
├── templates/              # Smart contract tests
├── load/                   # Performance tests
├── jest.setup.js          # Jest global setup
└── README.md              # Testing overview
```

## Test Types

### 1. Unit Tests (Jest)

**Purpose**: Test individual functions, modules, and components in isolation.

**Location**: `test/unit/`, `sdk/js/__tests__/`, inline `*.test.js` files

**Framework**: Jest + custom matchers

**Coverage**: 80% minimum threshold

### 2. Smart Contract Tests (Hardhat + Mocha)

**Purpose**: Test smart contract functionality, deployment, and interactions.

**Location**: `test/templates/`

**Framework**: Hardhat + Mocha + Chai

**Network**: Local Hardhat network

### 3. Integration Tests (Jest + GraphQL)

**Purpose**: Test API endpoints, database interactions, and service integrations.

**Location**: `test/integration/`

**Framework**: Jest + GraphQL testing utilities

**Environment**: Test database and services

### 4. End-to-End Tests (Playwright)

**Purpose**: Test complete user workflows through the browser interface.

**Location**: `test/e2e/workflows/`

**Framework**: Playwright + custom utilities

**Browsers**: Chromium, Firefox, WebKit

### 5. Load Tests (Artillery)

**Purpose**: Test system performance under various load conditions.

**Location**: `test/load/`

**Framework**: Artillery

**Scenarios**: API load, user concurrency, data processing

## Quick Start

### Prerequisites

- Node.js 16+
- NPM packages installed
- Local development environment running

### Setup

```bash
# Install all dependencies
npm install

# Install E2E test dependencies
cd test/e2e && npm install

# Install Playwright browsers
npx playwright install

# Set up test environment
npm run setup:test
```

### Run All Tests

```bash
# Complete test suite
npm run test:all

# Specific test types
npm run test:unit
npm run test:smart-contracts
npm run test:api:integration
npm run test:e2e
```

## Writing Tests

### Unit Test Example

```javascript
// test/unit/utils/validation.test.js
const { validateWeatherData } = require('../../backend/utils/validation');

describe('Weather Data Validation', () => {
  test('should validate correct weather data', () => {
    const validData = {
      temperature: 25.5,
      humidity: 60,
      pressure: 1013,
      timestamp: Date.now(),
    };

    expect(validateWeatherData(validData)).toBe(true);
  });

  test('should reject invalid temperature range', () => {
    const invalidData = {
      temperature: -100, // Invalid
      humidity: 60,
      pressure: 1013,
      timestamp: Date.now(),
    };

    expect(() => validateWeatherData(invalidData)).toThrow('Temperature out of valid range');
  });
});
```

### Smart Contract Test Example

```javascript
// test/templates/MyToken.template.test.js
const { loadFixture } = require('@nomicfoundation/hardhat-network-helpers');
const { expect } = require('chai');

describe('MyToken Contract', function () {
  async function deployTokenFixture() {
    const [owner, user1, user2] = await ethers.getSigners();
    const Token = await ethers.getContractFactory('MyToken');
    const token = await Token.deploy('Test Token', 'TEST');

    return { token, owner, user1, user2 };
  }

  describe('Token Transfer', function () {
    it('Should transfer tokens between accounts', async function () {
      const { token, owner, user1 } = await loadFixture(deployTokenFixture);

      await token.transfer(user1.address, ethers.parseEther('100'));

      expect(await token.balanceOf(user1.address)).to.equal(ethers.parseEther('100'));
    });
  });
});
```

### E2E Test Example

```javascript
// test/e2e/workflows/basic-workflow.test.js
const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('User Onboarding Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.goto('/');
  });

  test('should complete user registration flow', async () => {
    // Navigate to registration
    await helpers.page.click('[data-testid="register-button"]');

    // Fill registration form
    await helpers.fillForm({
      email: 'test@example.com',
      password: 'SecurePass123!',
      confirmPassword: 'SecurePass123!',
    });

    // Submit and verify
    await helpers.page.click('[data-testid="submit-registration"]');
    await expect(helpers.page.locator('[data-testid="welcome-message"]')).toContainText(
      'Welcome to WRFCoin'
    );
  });
});
```

## Running Tests

### Command Reference

```bash
# Unit Tests
npm run test:unit                    # All unit tests
npm run test:unit -- --watch       # Watch mode
npm run test:unit -- --coverage    # With coverage

# Smart Contract Tests
npm run test:smart-contracts        # All contract tests
npm run test:contracts:token        # Specific contract
npm run test:contracts -- --grep "transfer"  # Specific functionality

# Integration Tests
npm run test:api:integration        # API integration tests
npm run test:integration:weather    # Weather API tests
npm run test:integration:carbon     # Carbon credit tests

# End-to-End Tests
npm run test:e2e                    # All E2E tests
npm run test:e2e:chrome            # Chrome only
npm run test:e2e:headed            # With browser UI
npm run test:e2e:debug             # Debug mode

# Specific Workflows
npm run test:user-onboarding       # User onboarding flow
npm run test:wallet-operations     # Wallet functionality
npm run test:nft-marketplace       # NFT marketplace

# Performance & Load Tests
npm run test:performance           # Performance tests
npm run test:load                  # Load testing
npm run test:accessibility        # Accessibility tests

# Coverage & Reporting
npm run test:coverage              # Generate coverage report
npm run test:report               # Show test reports
```

### Test Filtering

```bash
# Run specific test files
npm run test:unit -- user.test.js
npm run test:e2e -- workflows/wallet-operations.test.js

# Run tests matching pattern
npm run test:unit -- --testNamePattern="validation"
npm run test:e2e -- --grep "wallet connection"

# Run tests by tag
npm run test:e2e -- --grep "@smoke"
npm run test:e2e -- --grep "@critical"
```

## Test Configuration

### Jest Configuration

Located in `jest.config.js` - controls unit and integration test behavior:

```javascript
module.exports = {
  testEnvironment: 'node',
  coverageThreshold: {
    global: {
      branches: 80,
      functions: 80,
      lines: 80,
      statements: 80,
    },
  },
  setupFilesAfterEnv: ['<rootDir>/test/jest.setup.js'],
};
```

### Playwright Configuration

Located in `test/e2e/playwright.config.js` - controls E2E test behavior:

```javascript
module.exports = {
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
  use: {
    baseURL: 'http://localhost:3000',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
};
```

### Hardhat Configuration

Located in `hardhat.config.js` - controls smart contract test behavior:

```javascript
module.exports = {
  solidity: '0.8.19',
  networks: {
    hardhat: {
      chainId: 1337,
    },
  },
  mocha: {
    timeout: 60000,
  },
};
```

## Best Practices

### 1. Test Organization

- **Group related tests** using `describe` blocks
- **Use descriptive test names** that explain the expected behavior
- **Follow AAA pattern**: Arrange, Act, Assert
- **Keep tests independent** - each test should be able to run in isolation

### 2. Test Data Management

```javascript
// ✅ Good: Use test fixtures
const testUser = testFixtures.users.user1;

// ✅ Good: Generate dynamic test data
const email = testUtils.generateTestData.email();

// ❌ Avoid: Hardcoded test data
const email = 'test@example.com'; // May conflict with other tests
```

### 3. Async Testing

```javascript
// ✅ Good: Proper async/await usage
test('should create user', async () => {
  const user = await createUser(userData);
  expect(user.id).toBeDefined();
});

// ❌ Avoid: Missing await
test('should create user', () => {
  const user = createUser(userData); // Returns Promise, not user
  expect(user.id).toBeDefined(); // Will fail
});
```

### 4. Error Testing

```javascript
// ✅ Good: Test error conditions
test('should throw error for invalid input', async () => {
  await expect(validateInput('invalid')).rejects.toThrow('Input validation failed');
});

// ✅ Good: Test error properties
test('should return proper error code', async () => {
  try {
    await apiCall();
    fail('Expected error to be thrown');
  } catch (error) {
    expect(error.code).toBe('VALIDATION_ERROR');
    expect(error.status).toBe(400);
  }
});
```

### 5. Smart Contract Testing

```javascript
// ✅ Good: Use proper fixtures and helpers
async function deployContractFixture() {
  const [owner, user1] = await ethers.getSigners();
  const Contract = await ethers.getContractFactory('MyContract');
  const contract = await Contract.deploy();
  return { contract, owner, user1 };
}

// ✅ Good: Test events and state changes
await expect(contract.transfer(user1.address, amount))
  .to.emit(contract, 'Transfer')
  .withArgs(owner.address, user1.address, amount);
```

### 6. E2E Testing

```javascript
// ✅ Good: Use data-testid attributes
await page.click('[data-testid="submit-button"]');

// ✅ Good: Wait for elements properly
await page.waitForSelector('[data-testid="success-message"]');

// ❌ Avoid: Brittle CSS selectors
await page.click('.btn.btn-primary.submit'); // May break with styling changes
```

## Debugging Tests

### 1. Debug Unit Tests

```bash
# Debug with Node.js debugger
node --inspect-brk node_modules/.bin/jest --runInBand

# Debug specific test
npm run test:unit -- --testNamePattern="user creation" --verbose
```

### 2. Debug Smart Contract Tests

```bash
# Enable Hardhat console logging
console.log("Contract address:", contract.address);

# Debug with Hardhat network
npx hardhat test --network hardhat --verbose
```

### 3. Debug E2E Tests

```bash
# Run with browser UI visible
npm run test:e2e:headed

# Debug specific test
npm run test:e2e:debug -- workflows/wallet-operations.test.js

# Enable trace viewer
npm run test:e2e -- --trace on
```

### 4. Test Output Analysis

```bash
# Detailed test output
npm run test:unit -- --verbose --detectOpenHandles

# Coverage report details
npm run test:coverage -- --verbose
```

## CI/CD Integration

### GitHub Actions Workflows

The testing framework integrates with GitHub Actions for automated testing:

1. **CI Readiness** (`.github/workflows/ci-readiness.yml`)

   - Lint and format checks
   - Unit test execution
   - Security scanning
   - Coverage validation
   - Smart contract compilation and tests
   - Rust workspace build verification

### Local CI Simulation

```bash
# Run the same checks as CI
npm run ci:test

# Validate PR-ready status
npm run validate:pr
```

## Troubleshooting

### Common Issues

#### 1. "Cannot find module" errors

```bash
# Solution: Ensure all dependencies are installed
npm install
cd test/e2e && npm install
```

#### 2. Hardhat compilation failures

```bash
# Solution: Clean and recompile
npx hardhat clean
npx hardhat compile
```

#### 3. E2E test timeouts

```javascript
// Solution: Increase timeouts in playwright.config.js
module.exports = {
  timeout: 60000, // Increase from default 30s
  expect: {
    timeout: 10000, // Increase assertion timeout
  },
};
```

#### 4. Database connection issues

```bash
# Solution: Check test database configuration
export TEST_DATABASE_URL="postgresql://user:pass@localhost/test_db"
npm run setup:test:db
```

#### 5. Port conflicts

```bash
# Solution: Check for running services
lsof -i :3000 :4000 :5432
# Kill conflicting processes or use different ports
```

### Debug Checklist

When tests fail, check:

1. ✅ **Dependencies installed**: `npm install` and `cd test/e2e && npm install`
2. ✅ **Environment variables set**: Check `.env` and test configuration
3. ✅ **Services running**: Database, Redis, development server
4. ✅ **Port availability**: No conflicts on required ports
5. ✅ **Test data clean**: Fresh test database state
6. ✅ **Browser dependencies**: `npx playwright install` for E2E tests

### Getting Help

1. **Check logs**: Look for specific error messages in test output
2. **Enable verbose mode**: Run tests with `--verbose` flag
3. **Isolate the problem**: Run single test files to narrow down issues
4. **Review recent changes**: Check if recent code changes broke tests
5. **Consult documentation**: Check framework-specific docs (Jest, Playwright, Hardhat)

## Performance Considerations

### Test Execution Speed

1. **Parallel execution**: Tests run in parallel by default
2. **Test isolation**: Use proper setup/teardown to avoid dependencies
3. **Smart fixtures**: Reuse expensive setup operations when possible
4. **Selective testing**: Use test filtering to run only relevant tests during development

### Resource Management

```javascript
// ✅ Good: Proper cleanup
afterEach(async () => {
  await page.close();
  await context.close();
});

// ✅ Good: Resource limits
jest.setTimeout(30000); // Reasonable timeout
```

## Extending the Framework

### Adding New Test Types

1. Create directory under `test/`
2. Add configuration in appropriate config file
3. Update npm scripts in `package.json`
4. Add to CI/CD workflows
5. Document in this guide

### Custom Matchers

```javascript
// test/jest.setup.js
expect.extend({
  toBeValidBlockchainAddress(received) {
    const isValid = /^0x[a-fA-F0-9]{40}$/.test(received);
    return {
      message: () => `expected ${received} to be a valid blockchain address`,
      pass: isValid,
    };
  },
});
```

### Test Utilities

Add reusable utilities to:

- `test/utils/` for general utilities
- `test/e2e/utils/` for E2E-specific utilities
- `test/jest.setup.js` for Jest global utilities

---

## Quick Reference

### Essential Commands

```bash
npm run test:all              # All tests
npm run test:unit             # Unit tests only
npm run test:smart-contracts  # Smart contract tests
npm run test:e2e             # End-to-end tests
npm run test:coverage        # Coverage report
```

### File Locations

- Jest config: `jest.config.js`
- E2E config: `test/e2e/playwright.config.js`
- Hardhat config: `hardhat.config.js`
- Test setup: `test/jest.setup.js`

### Coverage Thresholds

- Minimum: 80% (branches, functions, lines, statements)
- Target: 90%+ for critical components
- Exclusions: Configuration files, test files, vendor code

This testing framework provides comprehensive coverage for the WRFCoin platform. Follow these
guidelines to maintain high code quality and reliable functionality across all components.
