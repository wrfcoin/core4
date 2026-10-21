/**
 * Dedicated Jest Configuration for Contract Testing
 * Simplified configuration focused solely on contract tests
 */

const path = require('path');

module.exports = {
  rootDir: path.join(__dirname, '../..'),
  displayName: 'contract',
  testMatch: ['<rootDir>/tests/contract/**/*.test.js'],
  testEnvironment: 'node',
  setupFilesAfterEnv: ['<rootDir>/tests/utils/test-helpers.js'],
  globalSetup: '<rootDir>/tests/contract/setup/global.setup.js',
  globalTeardown: '<rootDir>/tests/contract/setup/global.teardown.js',
  testTimeout: 60000,
  verbose: true,
  detectOpenHandles: true,
  forceExit: true,
  collectCoverage: false, // Skip coverage for contract tests
  maxWorkers: 1, // Run contract tests sequentially
};
