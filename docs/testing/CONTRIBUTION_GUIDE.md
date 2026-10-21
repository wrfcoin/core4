# Testing Contribution Guide

## Overview

This guide provides standards and best practices for contributing tests to the WRFCoin project. All
contributors must follow these guidelines to ensure consistent, maintainable, and reliable test
coverage.

## Testing Standards

### 1. Test Coverage Requirements

**Minimum Coverage Thresholds:**

- **Unit Tests**: 85% coverage for new code
- **Integration Tests**: Cover all API endpoints and critical paths
- **E2E Tests**: Cover all major user workflows
- **Smart Contracts**: 100% line coverage for deployed contracts

**Coverage Enforcement:**

- PR checks will fail if coverage drops below 80%
- New features require accompanying tests
- Bug fixes must include regression tests

### 2. Code Review Requirements

**All test code must:**

- ✅ Follow project coding standards
- ✅ Include descriptive test names and comments
- ✅ Use appropriate test frameworks
- ✅ Include both positive and negative test cases
- ✅ Handle async operations properly
- ✅ Clean up resources appropriately

**Review Checklist:**

- [ ] Tests cover the main functionality
- [ ] Tests cover edge cases and error conditions
- [ ] Tests are independent and can run in any order
- [ ] Test data is properly managed (fixtures, mocks)
- [ ] Performance considerations are addressed
- [ ] Security aspects are tested where applicable

### 3. Test Types and When to Use Them

| Test Type                | When to Use                                              | Coverage Target |
| ------------------------ | -------------------------------------------------------- | --------------- |
| **Unit Tests**           | Individual functions, utilities, components              | 90%+            |
| **Integration Tests**    | API endpoints, database operations, service interactions | All endpoints   |
| **Smart Contract Tests** | Contract deployment, function calls, state changes       | 100%            |
| **E2E Tests**            | Complete user workflows, critical business processes     | Major workflows |
| **Performance Tests**    | Load testing, response time validation                   | Critical paths  |

## Writing Tests

### 1. Unit Test Standards

```javascript
// ✅ Good Example
describe('WeatherDataProcessor', () => {
  let processor;

  beforeEach(() => {
    processor = new WeatherDataProcessor();
  });

  describe('validateReading', () => {
    it('should accept valid weather reading with all required fields', () => {
      const validReading = {
        stationId: 'STATION_001',
        temperature: 25.5,
        humidity: 60,
        pressure: 1013.25,
        timestamp: Date.now(),
      };

      const result = processor.validateReading(validReading);

      expect(result.isValid).toBe(true);
      expect(result.errors).toHaveLength(0);
    });

    it('should reject reading with temperature outside valid range', () => {
      const invalidReading = {
        stationId: 'STATION_001',
        temperature: -273.16, // Below absolute zero
        humidity: 60,
        pressure: 1013.25,
        timestamp: Date.now(),
      };

      const result = processor.validateReading(invalidReading);

      expect(result.isValid).toBe(false);
      expect(result.errors).toContain('Temperature below absolute zero');
    });

    it('should handle missing required fields gracefully', () => {
      const incompleteReading = {
        stationId: 'STATION_001',
        // Missing temperature, humidity, pressure, timestamp
      };

      expect(() => processor.validateReading(incompleteReading)).toThrow(
        'Missing required fields: temperature, humidity, pressure, timestamp'
      );
    });
  });
});
```

### 2. Smart Contract Test Standards

```javascript
// ✅ Good Example
describe('WRFToken', function () {
  let token, owner, user1, user2;

  beforeEach(async function () {
    [owner, user1, user2] = await ethers.getSigners();
    const Token = await ethers.getContractFactory('WRFToken');
    token = await Token.deploy('Weather Reward Token', 'WRF');
    await token.waitForDeployment();
  });

  describe('Minting', function () {
    it('should mint tokens to weather station providers', async function () {
      const mintAmount = ethers.parseEther('100');

      await expect(token.mint(user1.address, mintAmount))
        .to.emit(token, 'Transfer')
        .withArgs(ethers.ZeroAddress, user1.address, mintAmount);

      expect(await token.balanceOf(user1.address)).to.equal(mintAmount);
      expect(await token.totalSupply()).to.equal(mintAmount);
    });

    it('should only allow authorized minters to mint tokens', async function () {
      const mintAmount = ethers.parseEther('100');

      await expect(token.connect(user1).mint(user2.address, mintAmount)).to.be.revertedWith(
        'AccessControl: account 0x... is missing role ...'
      );
    });

    it('should prevent minting when contract is paused', async function () {
      await token.pause();
      const mintAmount = ethers.parseEther('100');

      await expect(token.mint(user1.address, mintAmount)).to.be.revertedWith('Pausable: paused');
    });
  });

  describe('Transfer Restrictions', function () {
    beforeEach(async function () {
      await token.mint(user1.address, ethers.parseEther('1000'));
    });

    it('should enforce minimum holding period for weather rewards', async function () {
      const transferAmount = ethers.parseEther('100');

      // Immediate transfer should fail
      await expect(token.connect(user1).transfer(user2.address, transferAmount)).to.be.revertedWith(
        'Tokens locked: minimum holding period not met'
      );

      // Fast-forward time past holding period
      await time.increase(86400 * 30); // 30 days

      // Transfer should now succeed
      await expect(token.connect(user1).transfer(user2.address, transferAmount))
        .to.emit(token, 'Transfer')
        .withArgs(user1.address, user2.address, transferAmount);
    });
  });
});
```

### 3. E2E Test Standards

```javascript
// ✅ Good Example
const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('Carbon Credit Trading Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.goto('/dashboard');
    await helpers.connectWallet('carbonProvider');
  });

  test('should complete full carbon credit lifecycle @critical', async () => {
    // Step 1: Register carbon project
    await test.step('Register carbon project', async () => {
      await helpers.page.click('[data-testid="create-carbon-project"]');

      await helpers.fillForm({
        'project-name': 'Amazon Rainforest Conservation',
        'project-type': 'Forest Conservation',
        location: 'Amazon Basin, Brazil',
        methodology: 'REDD+',
        'estimated-credits': '50000',
      });

      await helpers.page.click('[data-testid="submit-project"]');

      await expect(helpers.page.locator('[data-testid="project-created-message"]')).toContainText(
        'Project registered successfully'
      );
    });

    // Step 2: Submit verification documents
    await test.step('Submit verification documents', async () => {
      const projectCard = helpers.page.locator('[data-testid="project-card"]').first();
      await projectCard.click('[data-testid="upload-verification"]');

      // Upload required documents
      await helpers.uploadFiles([
        'test/e2e/fixtures/carbon-project-methodology.pdf',
        'test/e2e/fixtures/environmental-impact-assessment.pdf',
        'test/e2e/fixtures/baseline-study.pdf',
      ]);

      await expect(helpers.page.locator('[data-testid="verification-status"]')).toContainText(
        'Under Review'
      );
    });

    // Step 3: Mock verification approval and mint credits
    await test.step('Mint verified carbon credits', async () => {
      // Simulate admin approval (in real scenario, this would be manual)
      await helpers.simulateAdminAction('approve-carbon-project', {
        projectId: await helpers.getProjectId(),
        verifiedCredits: 45000, // Slightly less than estimated
      });

      await helpers.page.reload();

      await expect(helpers.page.locator('[data-testid="project-status"]')).toContainText(
        'Verified'
      );

      await expect(helpers.page.locator('[data-testid="available-credits"]')).toContainText(
        '45,000 credits'
      );
    });

    // Step 4: List credits for sale
    await test.step('List credits for trading', async () => {
      await helpers.page.click('[data-testid="list-for-sale"]');

      await helpers.fillForm({
        'credits-amount': '10000',
        'price-per-credit': '25.50',
        'minimum-purchase': '100',
      });

      await helpers.page.click('[data-testid="create-listing"]');

      await expect(helpers.page.locator('[data-testid="listing-created"]')).toBeVisible();
    });

    // Step 5: Purchase credits as different user
    await test.step('Purchase carbon credits', async () => {
      await helpers.connectWallet('user1');
      await helpers.goto('/carbon-marketplace');

      const listing = helpers.page.locator('[data-testid="carbon-listing"]').first();
      await listing.click();

      await helpers.fillForm({
        'purchase-amount': '500',
      });

      await listing.click('[data-testid="purchase-credits"]');

      // Approve transaction
      await helpers.approveTransaction();

      await expect(helpers.page.locator('[data-testid="purchase-success"]')).toContainText(
        'Successfully purchased 500 carbon credits'
      );
    });

    // Step 6: Retire credits
    await test.step('Retire carbon credits', async () => {
      await helpers.goto('/my-carbon-credits');

      const creditsCard = helpers.page.locator('[data-testid="owned-credits"]').first();
      await creditsCard.click('[data-testid="retire-credits"]');

      await helpers.fillForm({
        'retire-amount': '100',
        'retirement-reason': 'Corporate carbon offset for Q4 2024 emissions',
      });

      await helpers.page.click('[data-testid="confirm-retirement"]');

      await expect(helpers.page.locator('[data-testid="retirement-certificate"]')).toBeVisible();

      // Verify credits are removed from tradeable balance
      await expect(helpers.page.locator('[data-testid="tradeable-balance"]')).toContainText(
        '400 credits'
      );
    });
  });

  test('should handle insufficient funds gracefully', async () => {
    await helpers.connectWallet('userWithNoFunds');
    await helpers.goto('/carbon-marketplace');

    const listing = helpers.page.locator('[data-testid="carbon-listing"]').first();
    await listing.click();

    await helpers.fillForm({
      'purchase-amount': '1000',
    });

    await listing.click('[data-testid="purchase-credits"]');

    await expect(helpers.page.locator('[data-testid="insufficient-funds-error"]')).toContainText(
      'Insufficient funds for this purchase'
    );
  });
});
```

### 4. Test Data Management

```javascript
// ✅ Good: Use structured test fixtures
const testFixtures = {
  users: {
    carbonProvider: {
      walletAddress: '0x1234567890123456789012345678901234567890',
      email: 'provider@carbon-test.com',
      role: 'carbon-provider',
    },
    trader: {
      walletAddress: '0x2345678901234567890123456789012345678901',
      email: 'trader@carbon-test.com',
      role: 'trader',
    },
  },
  carbonProjects: {
    forestConservation: {
      name: 'Amazon Rainforest Conservation',
      type: 'Forest Conservation',
      methodology: 'REDD+',
      area: 10000,
      estimatedCredits: 50000,
    },
  },
};

// ✅ Good: Use factories for dynamic data
const createTestUser = (overrides = {}) => ({
  id: `user-${Date.now()}-${Math.random().toString(36).substr(2, 8)}`,
  email: `test-${Date.now()}@example.com`,
  walletAddress: generateTestWalletAddress(),
  createdAt: new Date().toISOString(),
  ...overrides,
});
```

## Testing Workflow

### 1. Before Writing Tests

1. **Understand the requirements**: Read user stories, acceptance criteria
2. **Identify test scenarios**: Happy path, edge cases, error conditions
3. **Choose appropriate test type**: Unit, integration, E2E, or smart contract
4. **Plan test data needs**: Fixtures, mocks, dynamic generation

### 2. Test Development Process

1. **Write failing test first** (TDD approach recommended)
2. **Implement minimum code** to make test pass
3. **Refactor** while keeping tests green
4. **Add edge cases** and error handling tests
5. **Review coverage** and add missing tests

### 3. Code Review Process

1. **Self-review checklist**:

   - [ ] Tests cover main functionality
   - [ ] Edge cases and errors are tested
   - [ ] Test names are descriptive
   - [ ] No hardcoded values or brittle selectors
   - [ ] Proper async handling
   - [ ] Resource cleanup

2. **Peer review focus**:
   - Test completeness and quality
   - Code maintainability
   - Performance implications
   - Security considerations

## Common Patterns and Anti-Patterns

### ✅ Good Patterns

```javascript
// Descriptive test names
it('should reject weather readings with temperature below absolute zero', () => {});

// Proper async handling
test('should create user account', async () => {
  const user = await userService.create(userData);
  expect(user.id).toBeDefined();
});

// Resource cleanup
afterEach(async () => {
  await cleanupDatabase();
  await page.close();
});

// Meaningful assertions
expect(response.status).toBe(201);
expect(response.body.user.email).toBe(testData.email);
expect(response.body.user.password).toBeUndefined(); // Security check
```

### ❌ Anti-Patterns to Avoid

```javascript
// Vague test names
it('should work', () => {}); // What should work?

// Missing async/await
test('should create user', () => {
  const user = userService.create(userData); // Returns Promise
  expect(user.id).toBeDefined(); // Will fail
});

// Hardcoded test data
const email = 'test@example.com'; // May conflict with other tests

// Brittle selectors
await page.click('#submit-btn-2'); // May break with UI changes

// No error testing
// Always test both success and failure scenarios

// Dependent tests
// Tests should be independent and not rely on execution order
```

## Performance Considerations

### 1. Test Execution Speed

- **Parallel execution**: Keep tests independent to enable parallelization
- **Efficient setup**: Use `beforeAll` for expensive setup when possible
- **Smart fixtures**: Reuse database fixtures across similar tests
- **Minimal E2E**: Use E2E tests for critical workflows only

### 2. Resource Management

```javascript
// ✅ Good: Proper cleanup
afterEach(async () => {
  await database.cleanup();
  await browser.close();
});

// ✅ Good: Connection pooling
const dbPool = new Pool({ max: 10 });

beforeAll(async () => {
  await dbPool.connect();
});

afterAll(async () => {
  await dbPool.end();
});
```

### 3. CI/CD Optimization

- Use test splitting for parallel CI execution
- Cache dependencies and build artifacts
- Run unit tests before slower integration/E2E tests
- Use different test suites for different triggers (PR vs. merge)

## Security Testing Guidelines

### 1. Authentication and Authorization

```javascript
test('should require authentication for protected endpoints', async () => {
  const response = await request(app).get('/api/user/profile').expect(401);

  expect(response.body.error).toBe('Authentication required');
});

test('should prevent unauthorized access to admin functions', async () => {
  const userToken = await generateUserToken();

  const response = await request(app)
    .post('/api/admin/users')
    .set('Authorization', `Bearer ${userToken}`)
    .send(userData)
    .expect(403);
});
```

### 2. Input Validation

```javascript
test('should sanitize SQL injection attempts', async () => {
  const maliciousInput = "'; DROP TABLE users; --";

  const response = await request(app)
    .post('/api/weather-data')
    .send({ stationId: maliciousInput })
    .expect(400);

  expect(response.body.error).toContain('Invalid station ID format');
});

test('should prevent XSS in user inputs', async () => {
  const xssPayload = '<script>alert("xss")</script>';

  const response = await request(app)
    .post('/api/carbon-projects')
    .send({ name: xssPayload })
    .expect(400);
});
```

### 3. Smart Contract Security

```javascript
test('should prevent reentrancy attacks', async () => {
  // Deploy malicious contract that attempts reentrancy
  const MaliciousContract = await ethers.getContractFactory('ReentrancyAttacker');
  const attacker = await MaliciousContract.deploy(token.address);

  // Fund the attacker
  await token.mint(attacker.address, ethers.parseEther('100'));

  // Attempt reentrancy attack should fail
  await expect(attacker.attack()).to.be.revertedWith('ReentrancyGuard: reentrant call');
});

test('should prevent integer overflow in token calculations', async () => {
  const maxUint256 =
    '115792089237316195423570985008687907853269984665640564039457584007913129639935';

  await expect(token.mint(user1.address, maxUint256)).to.be.revertedWith(
    'SafeMath: addition overflow'
  );
});
```

## Documentation Requirements

### 1. Test Documentation

Every test file should include:

```javascript
/**
 * @fileoverview Tests for WeatherDataProcessor class
 *
 * This test suite covers:
 * - Weather reading validation
 * - Data transformation and normalization
 * - Error handling for invalid inputs
 * - Integration with external weather APIs
 *
 * @requires WeatherDataProcessor
 * @requires testUtils
 */
```

### 2. Complex Test Scenarios

Document complex scenarios:

```javascript
/**
 * Test Scenario: Multi-chain carbon credit trading
 *
 * This test simulates a complex workflow where:
 * 1. Carbon credits are minted on Ethereum mainnet
 * 2. Credits are bridged to Polygon for lower gas costs
 * 3. Trading occurs on Polygon-based marketplace
 * 4. Final retirement is recorded back on Ethereum
 *
 * Key considerations:
 * - Cross-chain message passing delays
 * - Gas price differences between networks
 * - Bridge security and state synchronization
 */
test('should handle cross-chain carbon credit lifecycle', async () => {
  // Test implementation...
});
```

## Maintenance and Updates

### 1. Test Maintenance Schedule

- **Weekly**: Review failing tests and flaky test reports
- **Monthly**: Update test dependencies and review performance metrics
- **Quarterly**: Review test coverage and identify gaps
- **Release cycle**: Update test documentation and examples

### 2. Handling Flaky Tests

```javascript
// ✅ Good: Make tests more robust
test('should load user dashboard', async () => {
  await page.goto('/dashboard');

  // Wait for specific content instead of arbitrary delays
  await page.waitForSelector('[data-testid="user-profile"]');
  await page.waitForLoadState('networkidle');

  // Use retry logic for unreliable operations
  await expect(async () => {
    const balance = await page.textContent('[data-testid="token-balance"]');
    expect(balance).toMatch(/\d+\.\d+ WRF/);
  }).toPass({ timeout: 10000 });
});

// ❌ Avoid: Arbitrary delays
test('should load user dashboard', async () => {
  await page.goto('/dashboard');
  await page.waitForTimeout(5000); // Arbitrary delay
  // This may still be flaky
});
```

### 3. Version Compatibility

- Keep test frameworks updated with project dependencies
- Test against multiple Node.js versions in CI
- Document version requirements in test setup guides
- Provide migration guides for major framework updates

## Conclusion

Following these contribution guidelines ensures that the WRFCoin testing framework remains robust,
maintainable, and provides confidence in the platform's reliability. All contributors are expected
to adhere to these standards and continuously improve test quality.

For questions or clarifications, please reach out to the development team or create an issue in the
project repository.
