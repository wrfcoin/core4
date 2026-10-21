/**
 * Jest configuration for property-based testing
 */

module.exports = {
  displayName: 'Property-Based Fuzzing',
  testEnvironment: 'node',
  testMatch: [
    '**/property-based/**/*.test.js',
    '**/property-based/**/*.spec.js',
  ],
  collectCoverage: true,
  coverageDirectory: '../../reports/coverage',
  coverageReporters: ['text', 'lcov', 'html', 'json'],
  verbose: true,
  testTimeout: 60000, // 60 seconds for property-based tests
  setupFilesAfterEnv: ['<rootDir>/setup.js'],
};
