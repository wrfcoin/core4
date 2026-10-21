/**
 * Jest Configuration for Task 64.6 - Mining Control Operations Tests
 *
 * Specialized configuration for testing mining dashboard functionality
 */

module.exports = {
  displayName: 'Task 64.6 - Mining Control Operations',

  // Test environment
  testEnvironment: 'jsdom',

  // Setup files
  setupFilesAfterEnv: [
    '<rootDir>/tests/setup/jest.setup.js',
    '<rootDir>/tests/setup/task64.setup.js',
  ],

  // Test file patterns
  testMatch: [
    '<rootDir>/tests/integration/mining-control-operations.test.js',
    '<rootDir>/tests/frontend/mining-dashboard-components.test.jsx',
    '<rootDir>/tests/performance/mining-operations-performance.test.js',
  ],

  // Module name mapping for absolute imports
  moduleNameMapping: {
    '^@/(.*)$': '<rootDir>/frontend/src/$1',
    '^@backend/(.*)$': '<rootDir>/backend/$1',
    '^@tests/(.*)$': '<rootDir>/tests/$1',
  },

  // Transform configuration
  transform: {
    '^.+\\.(js|jsx)$': 'babel-jest',
    '^.+\\.(ts|tsx)$': 'ts-jest',
  },

  // Module file extensions
  moduleFileExtensions: ['js', 'jsx', 'ts', 'tsx', 'json'],

  // Coverage configuration
  collectCoverage: true,
  collectCoverageFrom: [
    'blockchain-core/api/static/js/one_click_mining.js',
    'blockchain-core/api/static/js/enhanced_mining_controls.js',
    'blockchain-core/api/static/js/real_time_mining_stats.js',
    'frontend/src/components/MinerDashboard/**/*.{js,jsx}',
    'backend/server/controllers/miningController.ts',
    'backend/routes/mining.ts',
    '!**/*.test.{js,jsx,ts,tsx}',
    '!**/node_modules/**',
    '!**/coverage/**',
  ],

  coverageReporters: ['text', 'lcov', 'html', 'json'],

  coverageDirectory: '<rootDir>/tests/coverage/task64',

  // Coverage thresholds specific to Task 64
  coverageThreshold: {
    global: {
      branches: 80,
      functions: 85,
      lines: 85,
      statements: 85,
    },
    './blockchain-core/api/static/js/one_click_mining.js': {
      branches: 75,
      functions: 80,
      lines: 80,
      statements: 80,
    },
    './frontend/src/components/MinerDashboard/': {
      branches: 85,
      functions: 90,
      lines: 90,
      statements: 90,
    },
  },

  // Test timeout
  testTimeout: 30000,

  // Global variables
  globals: {
    'ts-jest': {
      useESM: true,
    },
  },

  // Mock configuration
  clearMocks: true,
  restoreMocks: true,

  // Verbose output for detailed test results
  verbose: true,

  // Reporters
  reporters: [
    'default',
    [
      'jest-html-reporters',
      {
        publicPath: './tests/coverage/task64/html-report',
        filename: 'task64-test-report.html',
        pageTitle: 'Task 64.6 - Mining Control Operations Test Report',
      },
    ],
    [
      'jest-junit',
      {
        outputDirectory: './tests/coverage/task64',
        outputName: 'task64-junit.xml',
      },
    ],
  ],

  // Performance monitoring
  maxWorkers: '50%',

  // Error handling
  errorOnDeprecated: true,

  // Module mocking
  modulePathIgnorePatterns: [
    '<rootDir>/node_modules/',
    '<rootDir>/build/',
    '<rootDir>/dist/',
  ],
};
