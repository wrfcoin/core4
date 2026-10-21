/**
 * Jest Configuration for Chaos Tests
 *
 * Extends the main jest config with settings specific to chaos/resilience testing:
 * - Extended test timeout (2 minutes)
 * - Sequential execution (--runInBand)
 * - Custom setup for chaos utilities
 */

module.exports = {
  // Extend from root jest config
  preset: 'ts-jest',
  testEnvironment: 'node',

  // Chaos tests require longer timeouts
  testTimeout: 120000, // 2 minutes

  // Run tests sequentially to avoid resource conflicts
  maxWorkers: 1,

  // Only run chaos tests
  testMatch: ['<rootDir>/**/*.test.ts'],

  // Root directory for tests
  rootDir: __dirname,

  // Module name mapping for imports
  moduleNameMapper: {
    '^@/(.*)$': '<rootDir>/../../$1',
    '^@backend/(.*)$': '<rootDir>/../../backend/$1',
    '^@shared/(.*)$': '<rootDir>/../../backend/shared/$1',
    '^@resilience/(.*)$': '<rootDir>/../../backend/microservices/shared/resilience/$1',
  },

  // Transform TypeScript files
  transform: {
    '^.+\\.tsx?$': ['ts-jest', {
      tsconfig: '<rootDir>/../../tsconfig.json',
      diagnostics: {
        ignoreCodes: [151001], // Ignore "isolatedModules" warnings
      },
    }],
  },

  // Setup files
  setupFilesAfterEnv: ['<rootDir>/setup.ts'],

  // Coverage settings (optional for chaos tests)
  collectCoverage: false,

  // Verbose output for debugging
  verbose: true,

  // Don't transform node_modules except specific packages
  transformIgnorePatterns: [
    'node_modules/(?!(.*\\.mjs$))',
  ],

  // Clear mocks between tests
  clearMocks: true,

  // Restore mocks after each test
  restoreMocks: true,

  // Force exit after tests complete (cleanup hanging handles)
  forceExit: true,

  // Detect open handles for debugging
  detectOpenHandles: true,

  // Global settings
  globals: {
    'ts-jest': {
      useESM: true,
    },
  },
};
