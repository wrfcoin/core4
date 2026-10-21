/**
 * Jest Configuration for HSM Security Tests
 *
 * Specialized Jest configuration optimized for HSM security validation tests.
 * Configured for comprehensive security testing with proper timeouts and reporting.
 */

module.exports = {
  // Test environment
  testEnvironment: 'node',

  // Test file patterns
  testMatch: ['**/test/security/hsm-*.js'],

  // Test timeout (security tests may take longer)
  testTimeout: 60000, // 60 seconds

  // Setup files
  setupFilesAfterEnv: ['./jest.setup.hsm.js'],

  // Coverage configuration
  collectCoverage: true,
  coverageDirectory: './coverage/hsm-security',
  collectCoverageFrom: [
    'security/enterprise-hsm-service.ts',
    'backend/routes/governance.ts',
    'backend/routes/bridges.ts',
    'backend/routes/auth.ts',
    'backend/routes/weather.ts',
  ],
  coverageReporters: ['text', 'html', 'json', 'lcov'],
  coverageThreshold: {
    global: {
      branches: 80,
      functions: 80,
      lines: 80,
      statements: 80,
    },
  },

  // Transform configuration
  transform: {
    '^.+\\.ts$': 'ts-jest',
  },
  moduleFileExtensions: ['js', 'ts'],

  // Module resolution
  moduleNameMapping: {
    '^@/(.*)$': '<rootDir>/$1',
    '^security/(.*)$': '<rootDir>/security/$1',
    '^backend/(.*)$': '<rootDir>/backend/$1',
  },

  // Test sequencing (run security tests in specific order)
  testSequencer: './security-test-sequencer.js',

  // Reporters
  reporters: [
    'default',
    [
      'jest-html-reporter',
      {
        pageTitle: 'HSM Security Validation Report',
        outputPath: './coverage/hsm-security/security-test-report.html',
        includeFailureMsg: true,
        includeSuiteFailure: true,
      },
    ],
    [
      'jest-junit',
      {
        outputDirectory: './coverage/hsm-security',
        outputName: 'security-test-results.xml',
        suiteName: 'HSM Security Validation',
      },
    ],
  ],

  // Verbose output for security tests
  verbose: true,

  // Global variables for security tests
  globals: {
    SECURITY_TEST_MODE: true,
    HSM_TEST_ENDPOINT: 'localhost:9000',
    TEST_JWT_SECRET: 'hsm-security-test-key-12345',
  },

  // Clear mocks between tests to prevent interference
  clearMocks: true,
  resetMocks: true,
  restoreMocks: true,

  // Fail fast on security test failures
  bail: false, // Set to true for CI/CD to fail fast

  // Maximum worker processes (security tests can be resource intensive)
  maxWorkers: 4,

  // Cache configuration
  cache: false, // Disable cache for security tests to ensure fresh runs

  // Error handling
  errorOnDeprecated: true,

  // Test run configuration
  forceExit: true,
  detectOpenHandles: true,

  // Security-specific configuration
  testPathIgnorePatterns: ['/node_modules/', '/coverage/', '/dist/'],

  // Environment variables for security tests
  setupFiles: ['./security-test-env.js'],
};
