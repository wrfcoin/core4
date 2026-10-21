# WRFCoin Testing Framework Documentation

## Overview

WRFCoin implements a comprehensive testing framework covering unit tests, integration tests, smart
contract tests, and end-to-end testing. This documentation provides guidelines for running, writing,
and maintaining tests across the platform.

## Table of Contents

- [Quick Start](#quick-start)
- [Test Types](#test-types)
- [Running Tests](#running-tests)
- [Writing Tests](#writing-tests)
- [Test Configuration](#test-configuration)
- [CI/CD Integration](#cicd-integration)
- [Best Practices](#best-practices)
- [Troubleshooting](#troubleshooting)

## Quick Start

### Prerequisites

- Node.js 18+
- Python 3.9+ (for Python SDK tests)
- Git
- Docker (for integration tests)

### Setup

```bash
# Clone repository
git clone https://github.com/wrfcoin/core.git
cd wrfcoin

# Install dependencies
npm install

# Setup test environment
npm run setup:test

# Install browsers for E2E tests
cd test/e2e
npm install
npx playwright install
```

### Run All Tests

```bash
# Run complete test suite
npm run test:all

# Run specific test types
npm run test:unit
npm run test:integration
npm run test:e2e
npm run test:smart-contracts
```

## Test Types

### 1. Unit Tests

**Location**: `sdk/js/__tests__/`, `sdk/python/tests/`

Unit tests verify individual functions and components in isolation.

**Coverage**:

- JavaScript SDK core functionality
- Python SDK modules (when available)
- Utility functions
- Data validation
- Error handling

**Examples**:

```javascript
// sdk/js/__tests__/core/WrfcoinClient.test.js
describe('WrfcoinClient', () => {
  it('should connect to blockchain network', async () => {
    const client = new WrfcoinClient();
    await expect(client.connect()).resolves.toBeTruthy();
  });
});
```

### 2. Smart Contract Tests

**Location**: `test/templates/`

Tests for smart contract templates using Hardhat and Mocha.

**Coverage**:

- ERC20 token functionality
- DAO governance mechanisms
- AMM operations
- NFT marketplace contracts
- Cross-chain bridge contracts

**Example**:

```javascript
// test/templates/BaseERC20Token.template.test.js
describe('BaseERC20Token', function () {
  it('Should mint tokens correctly', async function () {
    const { token, owner } = await loadFixture(deployTokenFixture);
    await token.mint(owner.address, ethers.parseEther('1000'));
    expect(await token.balanceOf(owner.address)).to.equal(ethers.parseEther('1000'));
  });
});
```

### 3. Integration Tests

**Location**: `backend/graphql/__tests__/integration/`

Tests for API interactions and data flow between components.

**Coverage**:

- GraphQL queries and mutations
- Database operations
- External service integrations
- Authentication flows

### 4. End-to-End Tests

**Location**: `test/e2e/workflows/`

Browser-based tests using Playwright covering complete user workflows.

**Coverage**:

- User onboarding and registration
- Wallet operations
- NFT marketplace interactions
- DeFi operations
- Governance workflows
- Weather data integration
- Computational jobs
- Carbon credit trading
- Cross-chain bridging

**Example**:

```javascript
// test/e2e/workflows/user-onboarding.test.js
test('Complete user registration and account setup', async ({ page }) => {
  await page.click('[data-testid="get-started-button"]');
  await helpers.fillForm({
    'first-name': 'John',
    'last-name': 'Doe',
    email: helpers.generateRandomData('email'),
  });
  // ... complete onboarding flow
});
```

## Running Tests

### Local Development

```bash
# Run specific test files
npm test sdk/js/__tests__/core/Wallet.test.js
npx hardhat test test/templates/BaseERC20Token.template.test.js

# Run test suites
npm run test:sdk:js
npm run test:smart-contracts
npm run test:api:integration

# Run E2E tests
cd test/e2e
npm run test:workflows
npm run test:user-onboarding
npm run test:wallet-operations
```

### Browser-Specific E2E Tests

```bash
cd test/e2e

# Run on specific browsers
npm run test:chrome
npm run test:firefox
npm run test:safari

# Run on mobile devices
npm run test:mobile

# Debug mode
npm run test:debug
npm run test:headed
```

### Coverage Reports

```bash
# Generate coverage reports
npm run test:coverage

# View coverage reports
npm run coverage:open

# Coverage thresholds (currently 80%)
npm run coverage:check
```

### Performance Tests

```bash
# Load testing
npm run test:load

# Performance benchmarks
npm run test:performance
```

## Writing Tests

### Test Structure

Follow the Arrange-Act-Assert pattern:

```javascript
describe('Feature', () => {
  beforeEach(async () => {
    // Arrange - Setup test environment
  });

  it('should perform expected behavior', async () => {
    // Arrange - Setup specific test data
    const testData = createTestData();

    // Act - Execute the functionality
    const result = await performAction(testData);

    // Assert - Verify the results
    expect(result).toBe(expectedValue);
  });

  afterEach(async () => {
    // Cleanup
  });
});
```

### Test Data Management

Use the test helpers and fixtures:

```javascript
// E2E tests
const { TestHelpers } = require('../utils/test-helpers');

test('example test', async ({ page }) => {
  const helpers = new TestHelpers(page);
  await helpers.loginAs('user1');
  await helpers.connectWallet('user1');

  // Use predefined test data
  const userData = helpers.testData.users.user1;
  await helpers.fillForm({
    email: userData.email,
    address: userData.walletAddress,
  });
});
```

### Smart Contract Test Patterns

```javascript
// Use loadFixture for gas optimization
async function deployTokenFixture() {
  const [owner, addr1, addr2] = await ethers.getSigners();
  const Token = await ethers.getContractFactory('BaseERC20Token');
  const token = await Token.deploy('Test Token', 'TEST', 18);
  return { token, owner, addr1, addr2 };
}

describe('Token Tests', function () {
  it('should deploy correctly', async function () {
    const { token } = await loadFixture(deployTokenFixture);
    expect(await token.name()).to.equal('Test Token');
  });
});
```

### API Integration Tests

```javascript
// Mock external dependencies
const mockDataSource = createMockDataSource();

beforeAll(async () => {
  server = new ApolloServer({
    typeDefs,
    resolvers,
    dataSources: () => mockDataSource,
  });
});

it('should handle GraphQL mutations', async () => {
  const result = await mutate({
    mutation: CREATE_WALLET,
    variables: { password: 'secure-password' },
  });

  expect(result.errors).toBeUndefined();
  expect(result.data.createWallet).toBeDefined();
});
```

## Test Configuration

### Jest Configuration

Located in `jest.config.js`:

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
  collectCoverageFrom: ['sdk/**/*.js', 'backend/**/*.js', '!**/node_modules/**', '!**/test/**'],
};
```

### Playwright Configuration

Located in `test/e2e/playwright.config.js`:

```javascript
module.exports = defineConfig({
  testDir: './workflows',
  fullyParallel: true,
  retries: process.env.CI ? 2 : 0,
  use: {
    baseURL: 'http://localhost:5000',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});
```

### Hardhat Configuration

Smart contract testing configuration in `hardhat.config.js`:

```javascript
module.exports = {
  solidity: '0.8.19',
  networks: {
    hardhat: {
      chainId: 31337,
    },
  },
  mocha: {
    timeout: 60000,
    recursive: true,
  },
  gasReporter: {
    enabled: true,
    currency: 'USD',
  },
};
```

## CI/CD Integration

### GitHub Actions Workflows

#### Main Testing Workflow

`.github/workflows/e2e-testing.yml` runs:

- Smart contract tests
- SDK unit tests
- API integration tests
- E2E workflow tests (matrix across browsers)
- Mobile responsiveness tests
- Performance tests
- Security and accessibility tests

#### Coverage Workflow

`.github/workflows/test-coverage.yml` provides:

- Coverage analysis across all test types
- Coverage threshold enforcement
- Security vulnerability scanning
- Coverage reporting and badges

### Running Tests in CI

Tests automatically run on:

- Push to main/develop branches
- Pull requests to main/develop
- Nightly scheduled runs
- Manual workflow dispatch

### Test Artifacts

CI generates and stores:

- Test results and reports
- Coverage reports
- Screenshots/videos of failed E2E tests
- Performance benchmarks
- Security scan results

## Best Practices

### General Guidelines

1. **Write Descriptive Test Names**

   ```javascript
   // Good
   it('should reject transfer when insufficient balance');

   // Bad
   it('should fail');
   ```

2. **Use Proper Test Data**

   ```javascript
   // Good - Use test helpers
   const testUser = helpers.createTestUser();

   // Bad - Hardcode values
   const user = { name: 'John' };
   ```

3. **Test Error Conditions**

   ```javascript
   it('should throw error when invalid input provided', async () => {
     await expect(functionCall(invalidInput)).rejects.toThrow();
   });
   ```

4. **Clean Up After Tests**
   ```javascript
   afterEach(async () => {
     await helpers.cleanup();
     await resetDatabase();
   });
   ```

### E2E Test Guidelines

1. **Use Data Test IDs**

   ```html
   <!-- Good -->
   <button data-testid="submit-button">Submit</button>

   <!-- Bad -->
   <button class="btn primary">Submit</button>
   ```

2. **Wait for Elements**

   ```javascript
   // Good
   await page.waitForSelector('[data-testid="loading-complete"]');

   // Bad
   await page.waitForTimeout(5000);
   ```

3. **Test Accessibility**

   ```javascript
   // Include accessibility checks
   const violations = await helpers.checkAccessibility();
   expect(violations).toHaveLength(0);
   ```

4. **Test Mobile Responsiveness**
   ```javascript
   // Test mobile viewport
   await page.setViewportSize({ width: 375, height: 667 });
   await expect(page.locator('[data-testid="mobile-nav"]')).toBeVisible();
   ```

### Smart Contract Test Guidelines

1. **Use Fixtures for Gas Optimization**
2. **Test Access Control**
3. **Test Edge Cases and Boundary Conditions**
4. **Verify Events are Emitted**
5. **Test Gas Usage**

### Performance Guidelines

1. **Set Appropriate Timeouts**
2. **Use Parallel Execution When Possible**
3. **Mock External Dependencies**
4. **Optimize Test Data Setup**

## Troubleshooting

### Common Issues

#### E2E Tests Failing

```bash
# Check if services are running
npm run dev &
npm run start:api &

# Clear browser cache
npx playwright cache clear

# Update browsers
npx playwright install
```

#### Smart Contract Tests Failing

```bash
# Clean and recompile contracts
npx hardhat clean
npx hardhat compile

# Reset local blockchain
npx hardhat node --detach
```

#### Coverage Issues

```bash
# Clear coverage cache
npm run coverage:clean

# Regenerate coverage
npm run test:coverage
```

### Debug Mode

#### E2E Tests

```bash
# Run in headed mode
npm run test:headed

# Run in debug mode
npm run test:debug

# Generate trace viewer
npx playwright trace show trace.zip
```

#### Smart Contract Tests

```bash
# Enable verbose logging
npm run test:smart-contracts -- --verbose

# Run specific test with logs
npx hardhat test test/templates/Token.test.js --logs
```

### Environment Variables

```bash
# Test environment configuration
NODE_ENV=test
DATABASE_URL=postgresql://localhost/wrfcoin_test
REDIS_URL=redis://localhost:6379
E2E_HEADLESS=true
E2E_CLEANUP=false
```

## Test Maintenance

### Regular Tasks

1. **Update Test Dependencies**

   ```bash
   npm update
   npx playwright update
   ```

2. **Review and Update Test Data**
3. **Monitor Test Performance**
4. **Update Documentation**

### Adding New Tests

1. **Create test file in appropriate directory**
2. **Follow naming conventions**: `*.test.js` or `*.spec.js`
3. **Add to test scripts in package.json**
4. **Update CI configuration if needed**
5. **Document test purpose and usage**

### Test Metrics

Track these key metrics:

- Test coverage percentage
- Test execution time
- Test failure rate
- Code quality metrics
- Performance benchmarks

## Resources

- [Jest Documentation](https://jestjs.io/docs/getting-started)
- [Playwright Documentation](https://playwright.dev/docs/intro)
- [Hardhat Testing](https://hardhat.org/tutorial/testing-contracts)
- [Testing Best Practices](https://github.com/goldbergyoni/javascript-testing-best-practices)

## Additional Documentation

- **[Testing Guide](./TESTING_GUIDE.md)** - Comprehensive developer guide for using the testing
  framework
- **[Contribution Guide](./CONTRIBUTION_GUIDE.md)** - Standards and best practices for contributing
  tests
- **[Troubleshooting Guide](./TROUBLESHOOTING.md)** - Solutions to common testing issues and
  problems

## Support

For testing-related questions:

1. Check this documentation and the additional guides above
2. Review existing test examples in the codebase
3. Search GitHub issues for similar problems
4. Create new issue with appropriate testing tags

## Contributing

When contributing to the testing framework:

1. Follow the [Contribution Guide](./CONTRIBUTION_GUIDE.md)
2. Ensure all tests pass locally before submitting PRs
3. Include tests for new features and bug fixes
4. Update documentation when adding new test patterns
5. Maintain high code coverage standards
