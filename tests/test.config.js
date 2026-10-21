/**
 * Test Configuration
 * Environment settings for all test types
 */

const config = {
  // Test Database Configuration
  TEST_DATABASE: {
    host: process.env.TEST_DB_HOST || 'localhost',
    port: process.env.TEST_DB_PORT || 5432,
    database: process.env.TEST_DB_NAME || 'wrfcoin_test',
    user: process.env.TEST_DB_USER || 'postgres',
    password: process.env.TEST_DB_PASSWORD || 'postgres',
    schema: `test_${Date.now()}_${Math.random().toString(36).substring(7)}`, // Unique schema per test run
    ssl: false,
  },

  // Redis Configuration for Tests
  TEST_REDIS: {
    host: process.env.TEST_REDIS_HOST || 'localhost',
    port: process.env.TEST_REDIS_PORT || 6379,
    password: process.env.TEST_REDIS_PASSWORD || '',
    db: 15, // Use high DB number for tests
  },

  // Blockchain Configuration
  TEST_BLOCKCHAIN: {
    network: 'hardhat',
    url: process.env.TEST_BLOCKCHAIN_URL || 'http://localhost:8545',
    chainId: 31337,
    accounts: {
      deployer:
        '0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80',
      user1:
        '0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d',
      user2:
        '0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a',
      validator:
        '0x7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6',
    },
  },

  // Test Server Ports
  TEST_PORTS: {
    api: process.env.TEST_API_PORT || 4001,
    frontend: process.env.TEST_FRONTEND_PORT || 5001,
    graphql: process.env.TEST_GRAPHQL_PORT || 4002,
    websocket: process.env.TEST_WS_PORT || 4003,
  },

  // Test Timeouts
  TIMEOUTS: {
    short: 5000,
    medium: 15000,
    long: 60000,
    blockchain: 120000,
  },

  // Environment
  NODE_ENV: 'test',
  LOG_LEVEL: process.env.TEST_LOG_LEVEL || 'error',

  // Test Data Paths
  FIXTURES_PATH: './tests/fixtures',
  TEST_DATA_PATH: './tests/test-data',
  MOCK_DATA_PATH: './tests/mocks',

  // Performance Test Settings
  PERFORMANCE: {
    maxResponseTime: 1000,
    maxMemoryUsage: 100 * 1024 * 1024, // 100MB
    concurrentUsers: 100,
    testDuration: 60000, // 1 minute
  },

  // Security Test Settings
  SECURITY: {
    enablePenetrationTests: false, // Enable in CI only
    maxLoginAttempts: 3,
    sessionTimeout: 3600000, // 1 hour
  },
};

module.exports = config;
