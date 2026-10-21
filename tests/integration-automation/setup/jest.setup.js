/**
 * Jest Setup for Integration Test Automation
 * Agent 6 - Integration Test Automation Specialist
 *
 * Configures Jest environment for comprehensive integration testing
 * Integrates with Agent 1's Pact contracts and Agent 4's mock services
 */

const { configureToMatchImageSnapshot } = require('jest-image-snapshot');
const supertest = require('supertest');

// Configure Jest extensions and custom matchers
expect.extend({
  // Custom matcher for API response validation
  toHaveValidApiResponse(received, expectedStatus = 200) {
    const pass =
      received.status === expectedStatus &&
      received.headers['content-type'] &&
      received.headers['content-type'].includes('application/json');

    if (pass) {
      return {
        message: () => `Expected response not to have valid API format`,
        pass: true,
      };
    } else {
      return {
        message: () =>
          `Expected response to have valid API format (status: ${expectedStatus}, content-type: application/json)`,
        pass: false,
      };
    }
  },

  // Custom matcher for service health check
  toBeHealthyService(received) {
    const pass =
      received &&
      received.status === 'healthy' &&
      received.timestamp &&
      new Date(received.timestamp) > new Date(Date.now() - 60000); // Within last minute

    if (pass) {
      return {
        message: () => `Expected service not to be healthy`,
        pass: true,
      };
    } else {
      return {
        message: () => `Expected service to be healthy with recent timestamp`,
        pass: false,
      };
    }
  },

  // Custom matcher for Docker service validation
  toBeRunningDockerService(received, serviceName) {
    const pass =
      received &&
      received.includes(serviceName) &&
      !received.includes('Exited') &&
      !received.includes('Dead');

    if (pass) {
      return {
        message: () =>
          `Expected Docker service ${serviceName} not to be running`,
        pass: true,
      };
    } else {
      return {
        message: () => `Expected Docker service ${serviceName} to be running`,
        pass: false,
      };
    }
  },

  // Custom matcher for Agent 1's Pact contract validation
  toSatisfyPactContract(received, contractExpectation) {
    const pass = this.equals(
      received,
      expect.objectContaining(contractExpectation)
    );

    if (pass) {
      return {
        message: () => `Expected response not to satisfy Pact contract`,
        pass: true,
      };
    } else {
      return {
        message: () => `Expected response to satisfy Pact contract expectation`,
        pass: false,
      };
    }
  },

  // Custom matcher for mock service interaction validation
  toHaveInteractedWithMockService(received, mockServiceCall) {
    const pass =
      received &&
      received.mocked === true &&
      received.service === mockServiceCall.service &&
      received.endpoint === mockServiceCall.endpoint;

    if (pass) {
      return {
        message: () =>
          `Expected not to have interacted with mock service ${mockServiceCall.service}`,
        pass: true,
      };
    } else {
      return {
        message: () =>
          `Expected to have interacted with mock service ${mockServiceCall.service} at endpoint ${mockServiceCall.endpoint}`,
        pass: false,
      };
    }
  },
});

// Configure image snapshot testing for UI integration
const toMatchImageSnapshot = configureToMatchImageSnapshot({
  customDiffConfig: {
    threshold: 0.2,
  },
  failureThreshold: 0.01,
  failureThresholdType: 'percent',
});
expect.extend({ toMatchImageSnapshot });

// Global test configuration
global.testConfig = {
  // Service URLs for integration testing
  services: {
    postgres: 'postgresql://postgres:testpass@localhost:5432/wrfcoin_test',
    redis: 'redis://localhost:6379',
    blockchain: 'http://localhost:8545',
    weather: 'http://localhost:3010',
    carbon: 'http://localhost:3020',
    mockserver: 'http://localhost:1080',
  },

  // Test timeouts
  timeouts: {
    service: 5000, // 5 seconds for service calls
    database: 3000, // 3 seconds for database operations
    integration: 30000, // 30 seconds for integration flows
    e2e: 90000, // 90 seconds for end-to-end flows
  },

  // Agent integrations
  agents: {
    pactContractsEnabled: process.env.PACT_VERIFICATION_ENABLED === 'true',
    mockServicesEnabled: process.env.MOCK_SERVICES_ENABLED !== 'false',
    agent1ContractPath: process.env.PACT_CONTRACT_PATH || 'tests/contract',
    agent4MockPath: process.env.MOCK_SERVICES_PATH || 'tests/mocks-enhanced',
  },

  // Test data configuration
  testData: {
    users: {
      testUser: {
        username: 'integration_test_user',
        email: 'test@wrfcoin.integration',
        walletAddress: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
      },
      stationOperator: {
        username: 'station_operator_test',
        email: 'station@wrfcoin.integration',
        walletAddress: '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
      },
      admin: {
        username: 'admin_test',
        email: 'admin@wrfcoin.integration',
        walletAddress: '0x123456789ABCDEF123456789ABCDEF1234567890',
      },
    },

    stations: {
      testStation1: 'station-integration-001',
      testStation2: 'station-integration-002',
    },
  },
};

// Global test utilities
global.testUtils = {
  // Create supertest instance for API testing
  createApiClient: (baseURL = 'http://localhost:4001') => {
    return supertest(baseURL);
  },

  // Wait for service to be available
  waitForService: async (url, timeout = 30000) => {
    const start = Date.now();
    while (Date.now() - start < timeout) {
      try {
        const response = await fetch(url);
        if (response.ok) return true;
      } catch (error) {
        // Service not ready yet
      }
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
    throw new Error(`Service at ${url} not available within ${timeout}ms`);
  },

  // Clean database tables for test isolation
  cleanDatabase: async pool => {
    const cleanupOrder = [
      'user_sessions',
      'api_keys',
      'carbon_credits',
      'transactions',
      'weather_data',
      'weather_stations',
      // 'users' // Keep test users for integration tests
    ];

    for (const table of cleanupOrder) {
      try {
        await pool.query(`DELETE FROM ${table} WHERE true`);
      } catch (error) {
        console.warn(`Warning cleaning ${table}:`, error.message);
      }
    }
  },

  // Generate test data
  generateTestData: {
    user: (overrides = {}) => ({
      username: `test_user_${Date.now()}`,
      email: `test_${Date.now()}@wrfcoin.test`,
      walletAddress: `0x${Math.random().toString(16).substr(2, 40).padEnd(40, '0')}`,
      status: 'active',
      role: 'user',
      ...overrides,
    }),

    weatherData: (stationId = 'test-station', overrides = {}) => ({
      station_id: stationId,
      measurements: {
        temperature: 20.0 + Math.random() * 20,
        humidity: 40.0 + Math.random() * 40,
        pressure: 1000 + Math.random() * 50,
        windSpeed: Math.random() * 20,
        windDirection: Math.random() * 360,
      },
      quality_score: 0.8 + Math.random() * 0.2,
      timestamp: new Date().toISOString(),
      ...overrides,
    }),

    transaction: (overrides = {}) => ({
      tx_id: `0x${Math.random().toString(16).substr(2, 64).padEnd(64, '0')}`,
      from_address: global.testConfig.testData.users.testUser.walletAddress,
      to_address:
        global.testConfig.testData.users.stationOperator.walletAddress,
      amount: Math.floor(Math.random() * 1000000),
      fee: 21000,
      status: 'confirmed',
      type: 'transfer',
      ...overrides,
    }),
  },
};

// Setup console formatting for better test output
const originalConsole = console.log;
console.log = (...args) => {
  const timestamp = new Date().toISOString().substring(11, 23);
  originalConsole(`[${timestamp}]`, ...args);
};

// Global error handling for unhandled promises
process.on('unhandledRejection', (reason, promise) => {
  console.error('Unhandled Rejection at:', promise, 'reason:', reason);
  // Don't exit process during tests, but log the error
});

process.on('uncaughtException', error => {
  console.error('Uncaught Exception:', error);
  // Don't exit process during tests, but log the error
});

// Suppress Winston logger output during tests
process.env.NODE_ENV = 'test';
process.env.SILENT_LOGS = 'true';

// Jest setup completion log
console.log('✅ Integration Test Automation Jest Setup Complete');
console.log(
  '   🤝 Agent 1 Pact Integration:',
  global.testConfig.agents.pactContractsEnabled ? 'Enabled' : 'Disabled'
);
console.log(
  '   🐳 Agent 4 Mock Services:',
  global.testConfig.agents.mockServicesEnabled ? 'Enabled' : 'Disabled'
);
console.log('   🔧 Test Environment: Integration Automation');

// Export setup configuration for other test files
module.exports = {
  testConfig: global.testConfig,
  testUtils: global.testUtils,
};
