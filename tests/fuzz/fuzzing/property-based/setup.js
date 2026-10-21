/**
 * Jest setup for property-based testing
 */

// Increase timeout for property-based tests
jest.setTimeout(60000);

// Custom matchers for property-based testing
expect.extend({
  toBeCloseToWithPrecision(received, expected, precision = 2) {
    const diff = Math.abs(received - expected);
    const tolerance = Math.pow(10, -precision);

    const pass = diff < tolerance;

    if (pass) {
      return {
        message: () =>
          `expected ${received} not to be close to ${expected} with precision ${precision}`,
        pass: true,
      };
    } else {
      return {
        message: () =>
          `expected ${received} to be close to ${expected} with precision ${precision}, but difference was ${diff}`,
        pass: false,
      };
    }
  },

  toBeValidWrfAddress(received) {
    const isValid =
      typeof received === 'string' &&
      received.startsWith('Wrf') &&
      received.length === 34;

    if (isValid) {
      return {
        message: () => `expected ${received} not to be a valid Wrf address`,
        pass: true,
      };
    } else {
      return {
        message: () =>
          `expected ${received} to be a valid Wrf address (must start with 'Wrf' and be 34 characters long)`,
        pass: false,
      };
    }
  },
});

// Global error handler for property-based tests
process.on('unhandledRejection', (reason, promise) => {
  console.error('Unhandled Rejection at:', promise, 'reason:', reason);
  // Don't exit the process in test environment
});

// Mock console methods for cleaner test output
const originalConsoleLog = console.log;
const originalConsoleError = console.error;

console.log = (...args) => {
  if (process.env.NODE_ENV === 'test' && !process.env.VERBOSE_TESTS) {
    // Only show logs in verbose mode during tests
    return;
  }
  originalConsoleLog(...args);
};

console.error = (...args) => {
  // Always show errors
  originalConsoleError(...args);
};
