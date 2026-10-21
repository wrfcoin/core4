/**
 * Enhanced Jest Configuration for Integration Test Automation
 * Agent 6 - Integration Test Automation Specialist Implementation
 *
 * Builds on Agent 1's Pact contracts and Agent 4's mock services
 * to provide comprehensive automated integration testing
 */

const path = require('path');

module.exports = {
  // Use project root as base
  rootDir: path.join(__dirname, '..'),

  // Test environment optimized for integration tests
  testEnvironment: 'node',

  // Display name for test identification
  displayName: 'integration-automation',

  // Integration test patterns - comprehensive automation
  testMatch: [
    '<rootDir>/tests/integration-automation/**/*.test.js',
    '<rootDir>/tests/integration-automation/**/*.spec.js',
    '<rootDir>/tests/integration/**/*.automation.test.js',
    '<rootDir>/tests/integration/**/*.e2e.test.js',
  ],

  // Setup files for integration test environment
  setupFilesAfterEnv: [
    '<rootDir>/tests/integration-automation/setup/jest.setup.js',
    '<rootDir>/tests/utils/test-helpers.js',
    '<rootDir>/tests/integration-automation/setup/mock-services.setup.js',
  ],

  // Global setup and teardown for Docker services
  globalSetup: '<rootDir>/tests/integration-automation/setup/global.setup.js',
  globalTeardown:
    '<rootDir>/tests/integration-automation/setup/global.teardown.js',

  // Timeout for integration tests
  testTimeout: 90000, // 90 seconds for comprehensive integration tests

  // Module name mapping for easy imports
  moduleNameMapper: {
    // Test automation utilities
    '^@automation/(.*)$': '<rootDir>/tests/integration-automation/$1',
    '^@mocks/(.*)$': '<rootDir>/tests/mocks-enhanced/$1',
    '^@contracts/(.*)$': '<rootDir>/tests/contract/$1',

    // Backend aliases
    '^@backend/(.*)$': '<rootDir>/backend/$1',
    '^@services/(.*)$': '<rootDir>/backend/services/$1',
    '^@models/(.*)$': '<rootDir>/backend/models/$1',
    '^@shared/(.*)$': '<rootDir>/backend/shared/$1',

    // Test utilities
    '^@tests/(.*)$': '<rootDir>/tests/$1',
    '^@testUtils/(.*)$': '<rootDir>/tests/utils/$1',
  },

  // Transform configuration
  preset: 'ts-jest',
  transform: {
    '^.+\\.tsx?$': 'ts-jest',
    '^.+\\.jsx?$': 'babel-jest',
  },

  transformIgnorePatterns: [
    'node_modules/(?!(uuid|axios|@opentelemetry|supertest|docker-compose|@pact-foundation)/)',
  ],

  // Coverage configuration for integration tests
  collectCoverage: process.env.INTEGRATION_COVERAGE === 'true',
  coverageDirectory: '<rootDir>/tests/results/coverage/integration-automation',
  coverageReporters: ['text', 'lcov', 'html', 'json-summary', 'cobertura'],

  collectCoverageFrom: [
    // Backend integration endpoints
    'backend/services/**/*.{js,ts}',
    'backend/api/**/*.{js,ts}',
    'backend/middleware/**/*.{js,ts}',

    // Integration layer coverage
    'backend/integrations/**/*.{js,ts}',
    'backend/controllers/**/*.{js,ts}',

    // Exclude test files and generated code
    '!**/*.test.{js,ts}',
    '!**/*.spec.{js,ts}',
    '!**/__tests__/**',
    '!**/node_modules/**',
    '!**/coverage/**',
    '!**/*.d.ts',
  ],

  coverageThreshold: {
    global: {
      branches: 70,
      functions: 70,
      lines: 70,
      statements: 70,
    },
    './backend/services/': {
      branches: 75,
      functions: 75,
      lines: 75,
      statements: 75,
    },
    './backend/api/': {
      branches: 80,
      functions: 80,
      lines: 80,
      statements: 80,
    },
  },

  // Enhanced reporting for integration tests
  reporters: [
    'default',
    [
      'jest-html-reporter',
      {
        pageTitle: 'WRFCoin Integration Test Automation Report',
        outputPath:
          '<rootDir>/tests/results/integration-automation-report.html',
        includeFailureMsg: true,
        includeConsoleLog: true,
        includeSuiteFailure: true,
        sort: 'status',
      },
    ],
    [
      'jest-junit',
      {
        outputDirectory: '<rootDir>/tests/results',
        outputName: 'integration-automation-junit.xml',
        suiteName: 'WRFCoin Integration Automation Tests',
        classNameTemplate: '{classname}',
        titleTemplate: '{title}',
        ancestorSeparator: ' › ',
        usePathForSuiteName: true,
      },
    ],
    [
      '@jest/test-result-processor',
      '<rootDir>/tests/integration-automation/reporters/custom-reporter.js',
    ],
  ],

  // Global variables for integration testing
  globals: {
    'ts-jest': {
      tsconfig: '<rootDir>/tsconfig.json',
    },
    // Integration test environment variables
    INTEGRATION_TEST_MODE: true,
    MOCK_SERVICES_ENABLED: true,
    PACT_VERIFICATION_ENABLED: true,
  },

  // Parallel execution optimized for integration tests
  maxWorkers: process.env.CI ? 2 : 4, // Limited workers for Docker stability

  // Verbose output for integration test debugging
  verbose: true,

  // Clear mocks between tests for isolation
  clearMocks: true,
  resetMocks: true,
  restoreMocks: true,

  // Detect open handles for cleanup validation
  detectOpenHandles: true,
  forceExit: false, // Allow graceful shutdown

  // Custom test sequencer for integration tests
  testSequencer:
    '<rootDir>/tests/integration-automation/sequencer/integration.sequencer.js',

  // Watch plugins for development
  watchPlugins: [
    'jest-watch-typeahead/filename',
    'jest-watch-typeahead/testname',
    ['jest-watch-toggle-config', { setting: 'verbose' }],
    ['jest-watch-toggle-config', { setting: 'collectCoverage' }],
  ],

  // Module file extensions
  moduleFileExtensions: ['js', 'jsx', 'ts', 'tsx', 'json', 'node'],

  // Ignore patterns for integration tests
  testPathIgnorePatterns: [
    '/node_modules/',
    '/dist/',
    '/build/',
    '/coverage/',
    '/tests/archive/',
    '/tests/results/',
    '/tests/unit/', // Focus only on integration
    '/tests/e2e/', // Separate E2E configuration
  ],

  // Environment variables for integration tests
  testEnvironmentOptions: {
    // Node environment options
    NODE_ENV: 'test',
    // Database URLs for test environment
    DATABASE_URL: 'postgresql://postgres:testpass@localhost:5432/wrfcoin_test',
    DATABASE_REPLICA_URL:
      'postgresql://postgres:testpass@localhost:5433/wrfcoin_test',
    // Mock service URLs
    BLOCKCHAIN_RPC_URL: 'http://localhost:8545',
    BLOCKCHAIN_RPC_WS_URL: 'ws://localhost:8546',
    WEATHER_API_URL: 'http://localhost:3010',
    CARBON_ORACLE_URL: 'http://localhost:3020',
    REDIS_URL: 'redis://localhost:6379',
    MOCKSERVER_URL: 'http://localhost:1080',
    // Integration test configuration
    INTEGRATION_TEST_PARALLEL_WORKERS: '4',
    INTEGRATION_TEST_TIMEOUT: '90000',
    PACT_BROKER_URL: 'http://localhost:9292',
    DOCKER_COMPOSE_FILE: 'tests/docker-compose.test.yml',
    DOCKER_COMPOSE_PROFILE: 'testing',
  },

  // Retry failed tests for flaky integration scenarios
  retry: process.env.CI ? 2 : 1,

  // Custom matchers for integration testing
  snapshotFormat: {
    escapeString: true,
    printBasicPrototype: true,
  },
};
