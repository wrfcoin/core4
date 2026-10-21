/**
 * Unified Jest Configuration for WRFCoin Platform Tests
 * Consolidated from multiple test configurations into a single, organized structure
 */

const path = require('path');

module.exports = {
  // Use tests directory as base
  rootDir: __dirname,

  // Test environment
  testEnvironment: 'node',

  // Enable experimental support for ES modules
  extensionsToTreatAsEsm: ['.ts'],

  // Global settings
  globals: {
    'ts-jest': {
      useESM: true,
      tsconfig: '<rootDir>/../tsconfig.json',
    },
  },

  // Test directory and patterns - unified structure
  testMatch: [
    // Unit tests
    '<rootDir>/tests/unit/**/*.test.{js,jsx,ts,tsx}',
    '<rootDir>/tests/unit/**/*.spec.{js,jsx,ts,tsx}',

    // Integration tests
    '<rootDir>/tests/integration/**/*.test.{js,jsx,ts,tsx}',
    '<rootDir>/tests/integration/**/*.spec.{js,jsx,ts,tsx}',

    // Security tests
    '<rootDir>/tests/security/**/*.test.{js,jsx,ts,tsx}',

    // Performance tests (Jest portion)
    '<rootDir>/tests/performance/**/*.test.{js,jsx,ts,tsx}',

    // Fuzz testing (property-based tests)
    '<rootDir>/tests/fuzz/fuzzing/property-based/**/*.test.{js,jsx,ts,tsx}',

    // Backend component tests
    '<rootDir>/backend/**/__tests__/**/*.test.{js,ts}',

    // Frontend component tests
    '<rootDir>/frontend/src/**/__tests__/**/*.test.{js,jsx,ts,tsx}',
  ],

  // Projects for different test types
  projects: [
    {
      displayName: 'unit',
      testMatch: ['<rootDir>/unit/**/*.test.{js,ts}'],
      testPathIgnorePatterns: [
        '/node_modules/',
        '<rootDir>/unit/smart-contracts/',
        '<rootDir>/unit/templates/',
        // Orphaned by the backend extraction (core4 #373): these suites import
        // backend product code that now lives in wrfcoin/backend, so they can
        // no longer resolve here. Ignored — NOT passing — until ported to that
        // repo. Tracked in wrfcoin/backend#915; context in core4 #2233.
        '<rootDir>/unit/lineage/lineage-service.test.ts',
        '<rootDir>/unit/lineage/lineage-api.test.ts',
        '<rootDir>/unit/network/timeout-retry-behavior.test.ts',
        '<rootDir>/unit/network/blockchain-transactions.test.ts',
        '<rootDir>/unit/network/mesh-messaging.test.ts',
        '<rootDir>/unit/network/offline-degradation.test.ts',
        '<rootDir>/unit/carbon-credit/marketplace-service.test.ts',
        '<rootDir>/unit/carbon-credit/iot-integration-service.test.ts',
        '<rootDir>/unit/carbon-credit/hsm-transaction-service.test.ts',
      ],
      testEnvironment: 'node',
      setupFilesAfterEnv: ['<rootDir>/jest.setup.js'],
      transform: {
        '^.+\\.tsx?$': [
          'ts-jest',
          { tsconfig: '<rootDir>/../tsconfig.json', isolatedModules: true },
        ],
        '^.+\\.jsx?$': 'babel-jest',
      },
    },
    {
      displayName: 'integration',
      testMatch: ['<rootDir>/integration/**/*.test.{js,ts}'],
      testEnvironment: 'node',
      globalSetup: '<rootDir>/integration/backend/globalSetup.js',
      globalTeardown: '<rootDir>/integration/backend/globalTeardown.js',
      testTimeout: 60000,
      transform: {
        '^.+\\.tsx?$': ['ts-jest', { tsconfig: '<rootDir>/../tsconfig.json' }],
        '^.+\\.jsx?$': 'babel-jest',
      },
    },
    {
      displayName: 'contract',
      testMatch: ['<rootDir>/contract/**/*.test.{js,ts}'],
      testEnvironment: 'node',
      setupFilesAfterEnv: ['<rootDir>/jest.setup.js'],
      globalSetup: '<rootDir>/contract/setup/global.setup.js',
      globalTeardown: '<rootDir>/contract/setup/global.teardown.js',
      testTimeout: 60000,
      transform: {
        '^.+\\.tsx?$': ['ts-jest', { tsconfig: '<rootDir>/../tsconfig.json' }],
        '^.+\\.jsx?$': 'babel-jest',
      },
    },
    {
      displayName: 'security',
      testMatch: ['<rootDir>/security/**/*.test.{js,ts}'],
      testEnvironment: 'node',
      setupFilesAfterEnv: ['<rootDir>/jest.setup.js'],
      testTimeout: 120000,
      transform: {
        '^.+\\.tsx?$': ['ts-jest', { tsconfig: '<rootDir>/../tsconfig.json' }],
        '^.+\\.jsx?$': 'babel-jest',
      },
    },
    {
      displayName: 'frontend',
      testMatch: ['<rootDir>/../frontend/**/*.test.{js,jsx,ts,tsx}'],
      testEnvironment: 'jsdom',
      setupFilesAfterEnv: ['<rootDir>/jest.setup.js'],
      transform: {
        '^.+\\.tsx?$': ['ts-jest', { tsconfig: '<rootDir>/../tsconfig.json' }],
        '^.+\\.jsx?$': 'babel-jest',
      },
    },
    {
      displayName: 'smart-contracts',
      testMatch: ['<rootDir>/unit/smart-contracts/**/*.test.{js,ts}'],
      testEnvironment: 'node',
      testTimeout: 60000,
      transform: {
        '^.+\\.tsx?$': ['ts-jest', { tsconfig: '<rootDir>/../tsconfig.json' }],
        '^.+\\.jsx?$': 'babel-jest',
      },
    },
  ],

  // Setup files
  setupFilesAfterEnv: ['<rootDir>/jest.setup.js'],

  // Module name mapping for path aliases
  moduleNameMapper: {
    // ESM .js to .ts mapping for backend modules (handles ../../../backend/path.js -> backend/path.ts)
    '^\\.\\.\\/\\.\\.\\/\\.\\.\\/backend/(.+)\\.js$':
      '<rootDir>/../backend/$1.ts',
    '^\\.\\.\\/\\.\\.\\/backend/(.+)\\.js$': '<rootDir>/../backend/$1.ts',

    // Frontend aliases
    '^@/(.*)$': '<rootDir>/frontend/src/$1',
    '^@components/(.*)$': '<rootDir>/frontend/src/components/$1',
    '^@hooks/(.*)$': '<rootDir>/frontend/src/hooks/$1',
    '^@utils/(.*)$': '<rootDir>/frontend/src/utils/$1',

    // Backend aliases
    '^@backend/(.*)$': '<rootDir>/backend/$1',
    '^@services/(.*)$': '<rootDir>/backend/services/$1',
    '^@models/(.*)$': '<rootDir>/backend/models/$1',
    '^@shared/(.*)$': '<rootDir>/backend/shared/$1',
    '^@/types/(.*)$': '<rootDir>/backend/shared/types/$1',

    // Test utilities
    '^@tests/(.*)$': '<rootDir>/tests/$1',
    '^@testUtils/(.*)$': '<rootDir>/tests/utils/$1',

    // Style and asset mocks
    '\\.(css|less|scss|sass)$': '<rootDir>/tests/mocks/styleMock.js',
    '\\.(jpg|jpeg|png|gif|svg)$': '<rootDir>/tests/mocks/fileMock.js',
  },

  // Transform configuration
  preset: 'ts-jest',
  transform: {
    '^.+\\.tsx?$': 'ts-jest',
    '^.+\\.jsx?$': 'babel-jest',
  },

  transformIgnorePatterns: [
    'node_modules/(?!(uuid|axios|@opentelemetry|@jest|hardhat|ethers)/)',
  ],

  // Coverage configuration
  collectCoverage: process.env.COVERAGE === 'true',
  coverageDirectory: '<rootDir>/tests/results/coverage',
  coverageReporters: ['text', 'lcov', 'html', 'json-summary'],

  collectCoverageFrom: [
    // Frontend coverage
    'frontend/src/**/*.{js,jsx,ts,tsx}',
    '!frontend/src/**/*.d.ts',
    '!frontend/src/**/index.{js,ts}',
    '!frontend/src/**/*.stories.{js,jsx,ts,tsx}',

    // Backend coverage
    'backend/**/*.{js,ts}',
    '!backend/**/*.d.ts',
    '!backend/**/migrations/**',
    '!backend/**/seeds/**',

    // Smart contracts (tested separately with Hardhat)
    'contracts/**/*.sol',

    // Scripts
    'scripts/**/*.{js,ts}',

    // SDK coverage
    'sdks/**/*.{js,ts}',
    '!sdks/**/*.d.ts',
    '!sdks/**/dist/**',
    '!sdks/**/build/**',

    // Blockchain core (Rust - tested separately)
    '!blockchain-core/**',

    // Exclusions
    '!**/node_modules/**',
    '!**/vendor/**',
    '!**/test/**',
    '!**/tests/**',
    '!**/__tests__/**',
    '!**/coverage/**',
    '!**/*.config.js',
    '!**/*.min.js',
  ],

  coverageThreshold: {
    global: {
      branches: 75,
      functions: 75,
      lines: 75,
      statements: 75,
    },
    './backend/': {
      branches: 80,
      functions: 80,
      lines: 80,
      statements: 80,
    },
    './frontend/src/': {
      branches: 70,
      functions: 70,
      lines: 70,
      statements: 70,
    },
  },

  // Module file extensions
  moduleFileExtensions: ['js', 'jsx', 'ts', 'tsx', 'json', 'node'],

  // Ignore patterns
  testPathIgnorePatterns: [
    '/node_modules/',
    '/blockchain-core/target/',
    '/dist/',
    '/build/',
    '/coverage/',
    '/.next/',
    '/tests/archive/',
    '/tests/results/',
  ],

  // Watch plugins for development (removed to avoid dependency issues)
  // watchPlugins: [
  //   'jest-watch-typeahead/filename',
  //   'jest-watch-typeahead/testname',
  // ],

  // Reporters for unified test reporting (simplified to avoid missing dependencies)
  reporters: ['default'],

  // Global variables for testing (merged with ts-jest config above)

  // Timeout configurations
  testTimeout: 30000,

  // Parallel execution
  maxWorkers: process.env.CI ? 2 : '50%',

  // Verbose output in CI
  verbose: process.env.CI === 'true',

  // Clear mocks between tests
  clearMocks: true,

  // Reset modules between tests
  resetMocks: true,
  restoreMocks: true,
};
