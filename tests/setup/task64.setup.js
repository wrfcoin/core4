/**
 * Task 64.6 - Test Setup Configuration
 *
 * Setup and teardown for mining control operations tests
 */

// Mock WebSocket for consistent testing
global.WebSocket = jest.fn().mockImplementation(() => ({
  addEventListener: jest.fn(),
  removeEventListener: jest.fn(),
  send: jest.fn(),
  close: jest.fn(),
  readyState: 1,
  CONNECTING: 0,
  OPEN: 1,
  CLOSING: 2,
  CLOSED: 3,
}));

// Mock Chart.js for frontend tests
jest.mock('chart.js/auto', () => ({
  Chart: jest.fn().mockImplementation(() => ({
    update: jest.fn(),
    destroy: jest.fn(),
    resize: jest.fn(),
    data: {
      datasets: [
        {
          data: [],
        },
      ],
    },
  })),
}));

// Mock performance API for performance tests
global.performance = global.performance || {
  now: jest.fn(() => Date.now()),
  mark: jest.fn(),
  measure: jest.fn(),
};

// Setup console spying for error detection
const originalError = console.error;
beforeAll(() => {
  console.error = jest.fn();
});

afterAll(() => {
  console.error = originalError;
});

// Mock fetch for API testing
global.fetch = jest.fn();

// Setup default fetch responses for mining API
const setupDefaultFetchMocks = () => {
  fetch.mockImplementation(url => {
    // Mining stats endpoint
    if (url.includes('/api/mining/stats')) {
      return Promise.resolve({
        ok: true,
        status: 200,
        json: () =>
          Promise.resolve({
            hashrate: {
              current: 1500000,
              formatted: '1.50 MH/s',
              trend: 'up',
              change: '5.2',
            },
            workers: {
              active: 3,
              total: 4,
              efficiency: '96.5',
            },
            earnings: {
              total: 12.45,
              hourly: 0.52,
              daily: 12.48,
            },
            network: {
              difficulty: 15420,
              blockHeight: 156789,
              hashrate: 225000000,
            },
            hardware: {
              temperature: 58,
              power: 285,
              efficiency: 5.26,
            },
          }),
      });
    }

    // Mining operations endpoint
    if (url.includes('/api/mining/operations')) {
      if (url.includes('POST') || url.includes('post')) {
        return Promise.resolve({
          ok: true,
          status: 201,
          json: () =>
            Promise.resolve({
              success: true,
              data: {
                id: 'test-operation-id',
                status: 'active',
                miner: '0x742d35Cc6634C0532925a3b8D7389d72d77f8e82',
                algorithm: 'climate_proof_of_work',
                createdAt: new Date().toISOString(),
              },
            }),
        });
      } else {
        return Promise.resolve({
          ok: true,
          status: 200,
          json: () =>
            Promise.resolve({
              success: true,
              data: {
                id: 'test-operation-id',
                status: 'active',
                miner: '0x742d35Cc6634C0532925a3b8D7389d72d77f8e82',
              },
            }),
        });
      }
    }

    // Performance endpoints
    if (url.includes('/api/mining/performance')) {
      return Promise.resolve({
        ok: true,
        status: 200,
        json: () =>
          Promise.resolve({
            cpu: { usage: 65, temperature: 45, frequency: 3200 },
            memory: { usage: 78, total: 16384, available: 3604 },
            gpu: {
              usage: 95,
              temperature: 58,
              memory: 85,
              clock: 1800,
              power: 285,
            },
            network: {
              upload: 125,
              download: 850,
              latency: 15,
              packetLoss: 0.1,
            },
            storage: { read: 540, write: 320, usage: 67, space: 89 },
          }),
      });
    }

    // Configuration endpoints
    if (url.includes('/api/mining/configuration')) {
      return Promise.resolve({
        ok: true,
        status: 201,
        json: () =>
          Promise.resolve({
            success: true,
            data: {
              id: 'test-config-id',
              name: 'Test Configuration',
              algorithm: 'climate_proof_of_work',
            },
          }),
      });
    }

    // Default fallback
    return Promise.resolve({
      ok: true,
      status: 200,
      json: () => Promise.resolve({ success: true }),
    });
  });
};

// Reset mocks before each test
beforeEach(() => {
  jest.clearAllMocks();
  setupDefaultFetchMocks();
});

// Custom matchers for mining-specific assertions
expect.extend({
  toBeValidHashrate(received) {
    const hashratePattern = /^[\d,.]+ [A-Z]*H\/s$/;
    const pass = hashratePattern.test(received);

    if (pass) {
      return {
        message: () => `expected ${received} not to be a valid hashrate format`,
        pass: true,
      };
    } else {
      return {
        message: () =>
          `expected ${received} to be a valid hashrate format (e.g., "1.50 MH/s")`,
        pass: false,
      };
    }
  },

  toBeValidMinerAddress(received) {
    const addressPattern = /^0x[a-fA-F0-9]{40}$/;
    const pass = addressPattern.test(received);

    if (pass) {
      return {
        message: () =>
          `expected ${received} not to be a valid Ethereum address`,
        pass: true,
      };
    } else {
      return {
        message: () => `expected ${received} to be a valid Ethereum address`,
        pass: false,
      };
    }
  },

  toBeValidMiningStatus(received) {
    const validStatuses = [
      'active',
      'paused',
      'stopped',
      'starting',
      'stopping',
    ];
    const pass = validStatuses.includes(received);

    if (pass) {
      return {
        message: () => `expected ${received} not to be a valid mining status`,
        pass: true,
      };
    } else {
      return {
        message: () =>
          `expected ${received} to be a valid mining status (${validStatuses.join(', ')})`,
        pass: false,
      };
    }
  },

  toBeWithinPerformanceRange(received, min, max) {
    const value = parseFloat(received);
    const pass = value >= min && value <= max;

    if (pass) {
      return {
        message: () =>
          `expected ${received} not to be within performance range ${min}-${max}`,
        pass: true,
      };
    } else {
      return {
        message: () =>
          `expected ${received} to be within performance range ${min}-${max}`,
        pass: false,
      };
    }
  },
});

// Global test utilities
global.testUtils = {
  // Create mock mining operation
  createMockMiningOperation: (overrides = {}) => ({
    id: 'test-operation-id',
    status: 'active',
    miner: '0x742d35Cc6634C0532925a3b8D7389d72d77f8e82',
    algorithm: 'climate_proof_of_work',
    hashRate: 1500000,
    power: 300,
    createdAt: new Date().toISOString(),
    ...overrides,
  }),

  // Create mock mining stats
  createMockMiningStats: (overrides = {}) => ({
    hashrate: {
      current: 1500000,
      formatted: '1.50 MH/s',
      trend: 'up',
      change: '5.2',
    },
    workers: {
      active: 3,
      total: 4,
      efficiency: '96.5',
    },
    earnings: {
      total: 12.45,
      hourly: 0.52,
      daily: 12.48,
    },
    network: {
      difficulty: 15420,
      blockHeight: 156789,
      hashrate: 225000000,
    },
    hardware: {
      temperature: 58,
      power: 285,
      efficiency: 5.26,
    },
    ...overrides,
  }),

  // Wait for async operations
  waitForAsyncOps: (ms = 100) =>
    new Promise(resolve => setTimeout(resolve, ms)),

  // Mock WebSocket connection
  mockWebSocketConnection: () => {
    const mockWS = new WebSocket();
    const listeners = {};

    mockWS.addEventListener = jest.fn((event, callback) => {
      listeners[event] = callback;
    });

    mockWS.emit = (event, data) => {
      if (listeners[event]) {
        listeners[event](data);
      }
    };

    return mockWS;
  },

  // Generate test data for performance tests
  generatePerformanceTestData: (count = 100) => {
    return Array(count)
      .fill(null)
      .map((_, index) => ({
        timestamp: Date.now() + index * 1000,
        hashrate: 1000000 + Math.random() * 500000,
        temperature: 50 + Math.random() * 20,
        power: 250 + Math.random() * 100,
      }));
  },
};

// Environment setup for different test types
const setupTestEnvironment = () => {
  // Set test-specific environment variables
  process.env.NODE_ENV = 'test';
  process.env.TEST_MODE = 'true';
  process.env.API_BASE_URL = 'http://localhost:4001';
  process.env.WS_BASE_URL = 'ws://localhost:4001';

  // Mock localStorage for browser environment
  if (typeof window !== 'undefined') {
    const localStorageMock = {
      getItem: jest.fn(),
      setItem: jest.fn(),
      removeItem: jest.fn(),
      clear: jest.fn(),
    };
    Object.defineProperty(window, 'localStorage', {
      value: localStorageMock,
    });
  }
};

// Initialize test environment
setupTestEnvironment();

// Cleanup after all tests
afterAll(() => {
  // Clean up any remaining timers
  jest.clearAllTimers();

  // Restore original console methods
  console.error = originalError;

  // Clean up global mocks
  jest.restoreAllMocks();
});

console.log('✅ Task 64.6 test setup completed');
