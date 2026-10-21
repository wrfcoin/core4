/**
 * Contract Testing Utilities
 * Common test helpers and utilities for contract testing
 */

// Global test utilities for contract testing

// Set longer timeout for contract tests
jest.setTimeout(60000);

// Mock console methods to reduce noise during testing
const originalConsoleLog = console.log;
const originalConsoleWarn = console.warn;
const originalConsoleError = console.error;

beforeAll(() => {
  // Quiet console output during tests unless VERBOSE_TESTS is set
  if (!process.env.VERBOSE_TESTS) {
    console.log = jest.fn();
    console.warn = jest.fn();
    console.error = jest.fn();
  }
});

afterAll(() => {
  // Restore console methods
  console.log = originalConsoleLog;
  console.warn = originalConsoleWarn;
  console.error = originalConsoleError;
});

// Global error handler for unhandled promise rejections
process.on('unhandledRejection', (reason, promise) => {
  originalConsoleError('Unhandled Rejection at:', promise, 'reason:', reason);
});

// Helper function to wait for services to be ready
const waitForService = async (url, timeout = 30000) => {
  const start = Date.now();

  while (Date.now() - start < timeout) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return true;
      }
    } catch (error) {
      // Service not ready yet, continue waiting
    }

    await new Promise(resolve => setTimeout(resolve, 1000));
  }

  throw new Error(`Service at ${url} did not become ready within ${timeout}ms`);
};

// Helper function to generate test data
const generateTestData = {
  walletAddress: () =>
    '0x' + Math.random().toString(16).substr(2, 40).padStart(40, '0'),
  transactionHash: () =>
    '0x' + Math.random().toString(16).substr(2, 64).padStart(64, '0'),
  uuid: () => {
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(
      /[xy]/g,
      function (c) {
        const r = (Math.random() * 16) | 0;
        const v = c == 'x' ? r : (r & 0x3) | 0x8;
        return v.toString(16);
      }
    );
  },
  weatherData: () => ({
    temperature: 15 + Math.random() * 20, // 15-35°C
    humidity: 30 + Math.random() * 50, // 30-80%
    pressure: 1000 + Math.random() * 50, // 1000-1050 hPa
    windSpeed: Math.random() * 10, // 0-10 m/s
    windDirection: Math.random() * 360, // 0-360°
  }),
};

// Setup/teardown for test environment
const setupTestEnvironment = async () => {
  return {
    connected: true,
    database: 'wrfcoin_test',
  };
};

const teardownTestEnvironment = async () => {
  // Cleanup test resources
};

// Test data generator with weather-specific methods
const TestDataGenerator = {
  generateWeatherData: (overrides = {}) => {
    const defaults = {
      stationId: 'test-station-001',
      timestamp: Date.now(),
      temperature: 20 + (Math.random() - 0.5) * 10,
      humidity: 50 + (Math.random() - 0.5) * 20,
      pressure: 1013 + (Math.random() - 0.5) * 20,
      windSpeed: Math.max(0, 5 + (Math.random() - 0.5) * 10),
      windDirection: Math.floor(Math.random() * 360),
      rainfall: Math.max(0, (Math.random() - 0.8) * 5),
      visibility: 10 + Math.random() * 10,
      uvIndex: Math.floor(Math.random() * 11),
    };
    return { ...defaults, ...overrides };
  },
};

// Test assertions for validation
const TestAssertions = {
  assertValidWeatherData: (data) => {
    if (typeof data.temperature !== 'number' || data.temperature < -89.2 || data.temperature > 70.7) {
      throw new Error(`Invalid temperature: ${data.temperature}`);
    }
    if (typeof data.humidity !== 'number' || data.humidity < 0 || data.humidity > 100) {
      throw new Error(`Invalid humidity: ${data.humidity}`);
    }
    if (typeof data.pressure !== 'number' || data.pressure < 870 || data.pressure > 1084) {
      throw new Error(`Invalid pressure: ${data.pressure}`);
    }
  },
};

// Export utilities
global.waitForService = waitForService;
global.generateTestData = generateTestData;

module.exports = {
  waitForService,
  generateTestData,
  setupTestEnvironment,
  teardownTestEnvironment,
  TestDataGenerator,
  TestAssertions,
};
