# Testing Framework Troubleshooting Guide

## Overview

This guide provides solutions to common issues encountered when working with the WRFCoin testing
framework. Use this as your first resource when facing testing problems.

## Table of Contents

1. [Environment Setup Issues](#environment-setup-issues)
2. [Unit Test Problems](#unit-test-problems)
3. [Smart Contract Test Issues](#smart-contract-test-issues)
4. [E2E Test Problems](#e2e-test-problems)
5. [CI/CD Issues](#cicd-issues)
6. [Performance Problems](#performance-problems)
7. [Debugging Tools](#debugging-tools)
8. [Getting Help](#getting-help)

## Environment Setup Issues

### Issue: "Cannot find module" errors

**Symptoms:**

```bash
Error: Cannot find module '@playwright/test'
Module not found: 'ethers'
```

**Solutions:**

1. **Install all dependencies:**

```bash
# Root project dependencies
npm install

# E2E test dependencies
cd test/e2e
npm install

# Return to root
cd ../..
```

2. **Clear node_modules and reinstall:**

```bash
rm -rf node_modules package-lock.json
rm -rf test/e2e/node_modules test/e2e/package-lock.json
npm install
cd test/e2e && npm install
```

3. **Check Node.js version:**

```bash
node --version  # Should be 16+
npm --version   # Should be 8+
```

### Issue: Hardhat compilation failures

**Symptoms:**

```bash
Error HH600: Compilation failed
Could not resolve "OpenZeppelin/contracts"
```

**Solutions:**

1. **Install missing dependencies:**

```bash
npm install --save-dev @openzeppelin/contracts
npm install --save-dev @nomicfoundation/hardhat-toolbox
```

2. **Clean and recompile:**

```bash
npx hardhat clean
npx hardhat compile
```

3. **Check Hardhat configuration:**

```javascript
// hardhat.config.js
require('@nomicfoundation/hardhat-toolbox');

module.exports = {
  solidity: '0.8.19',
  paths: {
    sources: './contracts',
    tests: './test/templates',
    cache: './cache',
    artifacts: './artifacts',
  },
};
```

### Issue: Database connection problems

**Symptoms:**

```bash
Error: connect ECONNREFUSED 127.0.0.1:5432
Database connection failed
```

**Solutions:**

1. **Start PostgreSQL:**

```bash
# MacOS with Homebrew
brew services start postgresql

# Ubuntu/Debian
sudo systemctl start postgresql

# Docker
docker run --name postgres-test -e POSTGRES_PASSWORD=testpass -p 5432:5432 -d postgres
```

2. **Check environment variables:**

```bash
# Set test database URL
export TEST_DATABASE_URL="postgresql://postgres:testpass@localhost:5432/wrfcoin_test"

# Or create .env file
echo "TEST_DATABASE_URL=postgresql://postgres:testpass@localhost:5432/wrfcoin_test" >> .env
```

3. **Create test database:**

```bash
createdb wrfcoin_test
# Or via SQL
psql -c "CREATE DATABASE wrfcoin_test;"
```

### Issue: Port conflicts

**Symptoms:**

```bash
Error: listen EADDRINUSE :::3000
Port 3000 is already in use
```

**Solutions:**

1. **Find and kill conflicting processes:**

```bash
# Find process using port
lsof -i :3000
netstat -tulpn | grep 3000

# Kill process
kill -9 <PID>
```

2. **Use different ports:**

```bash
# Set different port in environment
export PORT=3001
export API_PORT=4001

# Or update test configuration
```

3. **Stop all development servers:**

```bash
pkill -f "node.*3000"
pkill -f "npm.*start"
```

## Unit Test Problems

### Issue: Tests failing with timeout errors

**Symptoms:**

```bash
Timeout - Async callback was not invoked within the 5000 ms timeout
```

**Solutions:**

1. **Increase Jest timeout:**

```javascript
// jest.config.js
module.exports = {
  testTimeout: 30000, // 30 seconds
};

// Or per test
jest.setTimeout(30000);
```

2. **Fix async/await usage:**

```javascript
// ❌ Wrong
test('should create user', () => {
  const result = createUser(data); // Missing await
  expect(result.id).toBeDefined();
});

// ✅ Correct
test('should create user', async () => {
  const result = await createUser(data);
  expect(result.id).toBeDefined();
});
```

3. **Check for hanging promises:**

```bash
# Run with detectOpenHandles
npm run test:unit -- --detectOpenHandles --forceExit
```

### Issue: Mock functions not working

**Symptoms:**

```bash
Expected mock function to have been called, but it was not called
```

**Solutions:**

1. **Check mock setup:**

```javascript
// ✅ Correct mock setup
jest.mock('../services/weatherService', () => ({
  getWeatherData: jest.fn().mockResolvedValue(mockData),
}));

// Import after mock
const { getWeatherData } = require('../services/weatherService');
```

2. **Clear mocks between tests:**

```javascript
beforeEach(() => {
  jest.clearAllMocks();
});
```

3. **Verify mock is being called correctly:**

```javascript
test('should call weather service', async () => {
  await weatherController.getData('STATION_001');

  expect(getWeatherData).toHaveBeenCalledWith('STATION_001');
  expect(getWeatherData).toHaveBeenCalledTimes(1);
});
```

### Issue: Coverage reports missing files

**Symptoms:**

```bash
Coverage report shows 0% but tests are passing
```

**Solutions:**

1. **Check coverage configuration:**

```javascript
// jest.config.js
module.exports = {
  collectCoverage: true,
  collectCoverageFrom: [
    'backend/**/*.{js,ts}',
    'frontend/src/**/*.{js,jsx,ts,tsx}',
    '!**/node_modules/**',
    '!**/*.test.js',
  ],
};
```

2. **Run coverage explicitly:**

```bash
npm run test:coverage
# Or
npx jest --coverage
```

3. **Check file patterns:**

```bash
# Verify files match coverage patterns
find backend -name "*.js" | grep -v test | head -10
```

## Smart Contract Test Issues

### Issue: Contract deployment failures

**Symptoms:**

```bash
Error: VM Exception while processing transaction: reverted with reason string 'Initializable: contract is already initialized'
```

**Solutions:**

1. **Use proper fixtures:**

```javascript
const { loadFixture } = require('@nomicfoundation/hardhat-network-helpers');

async function deployContractFixture() {
  const [owner, user1] = await ethers.getSigners();
  const Contract = await ethers.getContractFactory('MyContract');
  const contract = await Contract.deploy();
  await contract.waitForDeployment();

  return { contract, owner, user1 };
}

describe('Contract Tests', function () {
  it('should work', async function () {
    const { contract, owner } = await loadFixture(deployContractFixture);
    // Test implementation
  });
});
```

2. **Check constructor parameters:**

```javascript
// If contract requires initialization parameters
const contract = await Contract.deploy('TokenName', 'TKN', 18);
```

3. **Verify contract compilation:**

```bash
npx hardhat compile --force
npx hardhat test --verbose
```

### Issue: Gas estimation errors

**Symptoms:**

```bash
Error: cannot estimate gas; transaction may fail or may require manual gas limit
```

**Solutions:**

1. **Set explicit gas limits:**

```javascript
const tx = await contract.transfer(to, amount, { gasLimit: 100000 });
await tx.wait();
```

2. **Check transaction parameters:**

```javascript
// Verify address formats
expect(ethers.isAddress(contract.target)).toBe(true);

// Check balance before transfer
const balance = await token.balanceOf(sender.address);
expect(balance).to.be.gte(amount);
```

3. **Use Hardhat network debugging:**

```javascript
// hardhat.config.js
module.exports = {
  networks: {
    hardhat: {
      loggingEnabled: true,
      chainId: 1337,
    },
  },
};
```

### Issue: Time-dependent test failures

**Symptoms:**

```bash
AssertionError: expected '1640995200' to equal '1640995260'
```

**Solutions:**

1. **Use Hardhat time helpers:**

```javascript
const { time } = require('@nomicfoundation/hardhat-network-helpers');

test('should handle time locks', async () => {
  const lockDuration = 86400; // 1 day

  await contract.lock(amount, lockDuration);

  // Move time forward
  await time.increase(lockDuration);

  // Now unlock should work
  await expect(contract.unlock()).to.not.be.reverted;
});
```

2. **Get consistent timestamps:**

```javascript
const timestamp = await time.latest();
const futureTime = timestamp + 86400;
```

## E2E Test Problems

### Issue: Browser not launching

**Symptoms:**

```bash
Error: Browser has been closed
Failed to launch browser
```

**Solutions:**

1. **Install browser binaries:**

```bash
npx playwright install
npx playwright install-deps
```

2. **Check system dependencies:**

```bash
# Ubuntu/Debian
sudo apt-get install -y \
  libnss3 \
  libatk-bridge2.0-0 \
  libdrm2 \
  libxkbcommon0 \
  libxcomposite1 \
  libxdamage1 \
  libxrandr2 \
  libgbm1 \
  libxss1 \
  libasound2
```

3. **Use different browser or headless mode:**

```javascript
// playwright.config.js
module.exports = {
  use: {
    headless: true, // Ensure headless mode
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    // Try chromium only first
  ],
};
```

### Issue: Element not found errors

**Symptoms:**

```bash
Error: Locator expected to be visible but was not
Timeout 30000ms exceeded
```

**Solutions:**

1. **Use proper wait strategies:**

```javascript
// ❌ Wrong
await page.click('[data-testid="submit-button"]');

// ✅ Correct
await page.waitForSelector('[data-testid="submit-button"]');
await page.click('[data-testid="submit-button"]');

// ✅ Even better
await expect(page.locator('[data-testid="submit-button"]')).toBeVisible();
await page.click('[data-testid="submit-button"]');
```

2. **Check element visibility:**

```javascript
// Debug element state
const element = page.locator('[data-testid="submit-button"]');
console.log('Element count:', await element.count());
console.log('Element visible:', await element.isVisible());
console.log('Element enabled:', await element.isEnabled());
```

3. **Use more robust selectors:**

```javascript
// ❌ Brittle
await page.click('#btn-123');

// ✅ Robust
await page.click('[data-testid="submit-button"]');
await page.click('text="Submit"');
await page.click('button:has-text("Submit")');
```

### Issue: Network request failures

**Symptoms:**

```bash
Error: net::ERR_CONNECTION_REFUSED
API calls failing in E2E tests
```

**Solutions:**

1. **Ensure backend is running:**

```bash
# Start development server
npm run dev

# Or start specific services
npm run start:api
npm run start:frontend
```

2. **Check base URL configuration:**

```javascript
// playwright.config.js
module.exports = {
  use: {
    baseURL: 'http://localhost:3000',
  },
  webServer: {
    command: 'npm run dev',
    port: 3000,
    reuseExistingServer: !process.env.CI,
  },
};
```

3. **Mock API responses if needed:**

```javascript
test('should handle API failure gracefully', async ({ page }) => {
  // Mock failed API response
  await page.route('/api/weather-data', route => {
    route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ error: 'Server error' }),
    });
  });

  await page.goto('/dashboard');

  await expect(page.locator('[data-testid="error-message"]')).toContainText(
    'Unable to load weather data'
  );
});
```

### Issue: Flaky test results

**Symptoms:**

```bash
Tests pass locally but fail in CI
Tests fail intermittently
```

**Solutions:**

1. **Increase timeouts:**

```javascript
// playwright.config.js
module.exports = {
  timeout: 60000, // 60 seconds
  expect: {
    timeout: 10000, // 10 seconds for assertions
  },
};
```

2. **Add retry logic:**

```javascript
// playwright.config.js
module.exports = {
  retries: process.env.CI ? 2 : 0,
};

// Or per-test retries
test('flaky test', async ({ page }) => {
  test.slow(); // Mark as slow test

  await expect(async () => {
    await page.reload();
    await expect(page.locator('[data-testid="status"]')).toHaveText('Ready');
  }).toPass({ timeout: 30000 });
});
```

3. **Wait for network idle:**

```javascript
await page.goto('/dashboard');
await page.waitForLoadState('networkidle');
```

## CI/CD Issues

### Issue: GitHub Actions failing

**Symptoms:**

```bash
Error: Process completed with exit code 1
Tests pass locally but fail in CI
```

**Solutions:**

1. **Check CI environment:**

```yaml
# .github/workflows/test.yml
- name: Debug environment
  run: |
    node --version
    npm --version
    echo "NODE_ENV: $NODE_ENV"
    echo "DATABASE_URL: $DATABASE_URL"
```

2. **Use consistent Node.js version:**

```yaml
- name: Setup Node.js
  uses: actions/setup-node@v4
  with:
    node-version: '18'
    cache: 'npm'
```

3. **Install system dependencies:**

```yaml
- name: Install system dependencies
  run: |
    sudo apt-get update
    sudo apt-get install -y postgresql-client
```

### Issue: Coverage reports not uploading

**Symptoms:**

```bash
Warning: Failed to upload coverage reports
Codecov upload failed
```

**Solutions:**

1. **Check coverage file generation:**

```bash
# Verify coverage files exist
ls -la coverage/
cat coverage/lcov.info | head -20
```

2. **Update upload action:**

```yaml
- name: Upload coverage to Codecov
  uses: codecov/codecov-action@v3
  with:
    files: ./coverage/lcov.info
    flags: unittests
    name: codecov-umbrella
    fail_ci_if_error: false
```

3. **Check Codecov token:**

```yaml
env:
  CODECOV_TOKEN: ${{ secrets.CODECOV_TOKEN }}
```

## Performance Problems

### Issue: Tests running slowly

**Symptoms:**

```bash
Test suite takes > 10 minutes
Individual tests timing out
```

**Solutions:**

1. **Enable parallel execution:**

```javascript
// jest.config.js
module.exports = {
  maxWorkers: '50%', // Use half of CPU cores
  // or
  maxWorkers: 4, // Specific number
};
```

2. **Optimize test setup:**

```javascript
// Use beforeAll for expensive operations
let database;

beforeAll(async () => {
  database = await setupTestDatabase();
});

beforeEach(async () => {
  await database.clear(); // Fast operation
});

afterAll(async () => {
  await database.close();
});
```

3. **Use test splitting:**

```bash
# Split tests across CI jobs
npm run test:unit -- --shard=1/4
npm run test:unit -- --shard=2/4
npm run test:unit -- --shard=3/4
npm run test:unit -- --shard=4/4
```

### Issue: Memory leaks in tests

**Symptoms:**

```bash
FATAL ERROR: Ineffective mark-compacts near heap limit
JavaScript heap out of memory
```

**Solutions:**

1. **Check for unclosed resources:**

```javascript
afterEach(async () => {
  // Close database connections
  await db.close();

  // Close browser pages
  await page.close();

  // Clear large objects
  testData = null;
});
```

2. **Increase memory limit:**

```bash
# In package.json scripts
"test": "NODE_OPTIONS='--max-old-space-size=4096' jest"
```

3. **Use memory profiling:**

```bash
npm run test:unit -- --detectLeaks --forceExit
```

## Debugging Tools

### Jest Debugging

```bash
# Debug with Chrome DevTools
node --inspect-brk node_modules/.bin/jest --runInBand

# Debug specific test
npm run test:unit -- --testNamePattern="user creation" --verbose

# Run single test file
npm run test:unit test/unit/user.test.js
```

### Playwright Debugging

```bash
# Debug mode with browser UI
npm run test:e2e:debug

# Generate trace files
npm run test:e2e -- --trace on

# Show trace viewer
npx playwright show-trace trace.zip
```

### Hardhat Debugging

```bash
# Enable console logging in contracts
console.log("Debug value:", value);

# Use Hardhat network console
npx hardhat console --network hardhat

# Enable verbose logging
npx hardhat test --verbose --show-stack-traces
```

### General Debugging

```bash
# Enable debug output
DEBUG=* npm run test:unit
DEBUG=api:* npm run test:integration

# Check test output files
ls -la test-results/
ls -la reports/
```

## Getting Help

### 1. Check Documentation

- [Testing Guide](./TESTING_GUIDE.md)
- [Contribution Guide](./CONTRIBUTION_GUIDE.md)
- Framework-specific docs:
  - [Jest Documentation](https://jestjs.io/docs/getting-started)
  - [Playwright Documentation](https://playwright.dev/docs/intro)
  - [Hardhat Documentation](https://hardhat.org/docs)

### 2. Common Commands for Diagnosis

```bash
# Environment check
node --version
npm --version
npx playwright --version
npx hardhat --version

# Dependencies check
npm ls
npm audit

# Service status
lsof -i :3000 :4000 :5432
ps aux | grep node

# Test execution with verbose output
npm run test:unit -- --verbose --detectOpenHandles
npm run test:e2e -- --headed --debug
npx hardhat test --verbose
```

### 3. Create Minimal Reproduction

When reporting issues:

1. **Isolate the problem**: Create a minimal test case that reproduces the issue
2. **Include environment details**: Node.js version, OS, npm version
3. **Provide error logs**: Full error messages and stack traces
4. **Share configuration**: Relevant config files (jest.config.js, playwright.config.js, etc.)

### 4. Community Resources

- **Project Issues**: Create an issue in the GitHub repository
- **Stack Overflow**: Tag questions with relevant framework tags
- **Discord/Slack**: Join project communication channels

### 5. Emergency Recovery

If tests are completely broken:

```bash
# Nuclear option: Reset everything
rm -rf node_modules package-lock.json
rm -rf test/e2e/node_modules test/e2e/package-lock.json
rm -rf .next build dist coverage test-results

# Clean npm cache
npm cache clean --force

# Reinstall everything
npm install
cd test/e2e && npm install

# Rebuild everything
npm run build
npx hardhat compile
npx playwright install

# Run basic tests
npm run test:unit -- --testPathPattern="simple"
```

This troubleshooting guide should help resolve most common issues. If you encounter problems not
covered here, please document the solution and contribute back to this guide.
