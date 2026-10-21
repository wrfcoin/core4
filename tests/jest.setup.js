/**
 * Jest Testing Framework Setup
 *
 * Global configuration and setup for Jest-based tests including:
 * - Test environment configuration
 * - Global test utilities
 * - Mock configurations
 * - Test database setup
 * - Winston logger configuration for tests (Task 113.9)
 */

const { TextEncoder, TextDecoder } = require('util');

// Global polyfills for Node.js environment
global.TextEncoder = TextEncoder;
global.TextDecoder = TextDecoder;

// Mock Web APIs that are not available in Node.js
global.fetch = require('node-fetch');

// Setup test environment variables
process.env.NODE_ENV = 'test';
process.env.DATABASE_URL =
  process.env.TEST_DATABASE_URL ||
  'postgresql://postgres:testpass@localhost:5432/wrfcoin_test';
process.env.REDIS_URL = process.env.TEST_REDIS_URL || 'redis://localhost:6379';
process.env.JWT_SECRET = 'test-jwt-secret-key-for-testing-only';
process.env.API_BASE_URL = 'http://localhost:4000';

/**
 * Winston Logger Configuration for Tests (Task 113.9)
 *
 * Configure logging behavior based on environment:
 * - SILENT_LOGS=true: Completely silence Winston output (default for tests)
 * - DEBUG=true or TEST_DEBUG=true: Enable verbose logging for debugging
 * - LOG_LEVEL: Override the log level (default: 'error' in tests)
 */
if (!process.env.DEBUG && !process.env.TEST_DEBUG) {
  process.env.SILENT_LOGS = 'true'; // Suppress Winston logger output during tests
  process.env.LOG_LEVEL = 'error'; // Only show errors if not completely silent
} else {
  // When debugging, show all logs
  process.env.SILENT_LOGS = 'false';
  process.env.LOG_LEVEL = process.env.LOG_LEVEL || 'debug';
}

// Increase test timeout for integration tests
jest.setTimeout(30000);

// Global test utilities
global.testUtils = {
  // Helper to wait for async operations
  sleep: ms => new Promise(resolve => setTimeout(resolve, ms)),

  // Helper to generate random test data
  generateTestData: {
    email: () =>
      `test-${Date.now()}-${Math.random().toString(36).substr(2, 9)}@example.com`,
    walletAddress: () => `0x${Math.random().toString(16).substr(2, 40)}`,
    apiKey: () => `test-api-${Math.random().toString(36).substr(2, 32)}`,
    userId: () =>
      `user-${Date.now()}-${Math.random().toString(36).substr(2, 8)}`,
    projectId: () =>
      `project-${Date.now()}-${Math.random().toString(36).substr(2, 8)}`,
  },

  // Helper to create mock blockchain transaction
  mockTransaction: (overrides = {}) => ({
    hash: `0x${Math.random().toString(16).substr(2, 64)}`,
    from: `0x${Math.random().toString(16).substr(2, 40)}`,
    to: `0x${Math.random().toString(16).substr(2, 40)}`,
    value: '1000000000000000000', // 1 ETH in wei
    gasPrice: '20000000000', // 20 Gwei
    gasLimit: '21000',
    blockNumber: Math.floor(Math.random() * 1000000),
    status: 'success',
    timestamp: Date.now(),
    ...overrides,
  }),

  // Helper to create mock weather data
  mockWeatherReading: (stationId = 'STATION_001', overrides = {}) => ({
    stationId,
    timestamp: Date.now(),
    temperature: 20 + (Math.random() - 0.5) * 10,
    humidity: 50 + (Math.random() - 0.5) * 20,
    pressure: 1013 + (Math.random() - 0.5) * 20,
    windSpeed: Math.max(0, 5 + (Math.random() - 0.5) * 10),
    precipitation: Math.max(0, (Math.random() - 0.8) * 5),
    ...overrides,
  }),

  // Helper to create mock carbon credit project
  mockCarbonProject: (overrides = {}) => ({
    id: global.testUtils.generateTestData.projectId(),
    name: 'Test Carbon Project',
    type: 'Forest Conservation',
    location: 'Test Location',
    area: 1000,
    methodology: 'REDD+',
    verificationStandard: 'VCS',
    estimatedCredits: 50000,
    pricePerCredit: 15.0,
    status: 'verified',
    ...overrides,
  }),
};

// Setup global test fixtures
global.testFixtures = {
  users: {
    admin: {
      id: 'admin-user',
      email: 'admin@wrfcoin.test',
      role: 'admin',
      walletAddress: '0x1234567890123456789012345678901234567890',
    },
    user1: {
      id: 'test-user-1',
      email: 'user1@wrfcoin.test',
      role: 'user',
      walletAddress: '0x2345678901234567890123456789012345678901',
    },
    user2: {
      id: 'test-user-2',
      email: 'user2@wrfcoin.test',
      role: 'user',
      walletAddress: '0x3456789012345678901234567890123456789012',
    },
    weatherProvider: {
      id: 'weather-provider',
      email: 'weather@wrfcoin.test',
      role: 'weather-provider',
      walletAddress: '0x4567890123456789012345678901234567890123',
    },
    carbonProvider: {
      id: 'carbon-provider',
      email: 'carbon@wrfcoin.test',
      role: 'carbon-provider',
      walletAddress: '0x5678901234567890123456789012345678901234',
    },
  },

  contracts: {
    wrfToken: '0xWRFTOKEN123456789012345678901234567890',
    usdcToken: '0xUSDCTOKEN12345678901234567890123456789',
    dao: '0xDAO1234567890123456789012345678901234567890',
    amm: '0xAMM1234567890123456789012345678901234567890',
    nftMarketplace: '0xNFT1234567890123456789012345678901234567890',
  },
};

// Mock external services - ethers.js v6 compatible (uses named exports)
jest.mock('ethers', () => {
  // Mock Wallet class
  class MockWallet {
    constructor() {
      this.address = '0x' + 'c'.repeat(40);
      this.privateKey = '0x' + 'a'.repeat(64);
      this.publicKey = '0x04' + 'b'.repeat(128);
    }
    connect = jest.fn().mockReturnThis();
    signTransaction = jest.fn().mockResolvedValue('0xsig');
    signMessage = jest.fn().mockResolvedValue('0x' + 'd'.repeat(130));
    sendTransaction = jest.fn().mockResolvedValue({
      hash: '0x' + 'e'.repeat(64),
      wait: jest.fn().mockResolvedValue({ status: 1 }),
    });
  }
  MockWallet.createRandom = jest.fn(() => new MockWallet());

  const mockContractFactory = jest.fn(() =>
    new Proxy(
      {},
      {
        get: (target, prop) => {
          if (prop === 'balanceOf') return jest.fn().mockResolvedValue(BigInt('1000000000000000000'));
          if (prop === 'transfer') return jest.fn().mockResolvedValue({ hash: '0x' + 'f'.repeat(64), wait: jest.fn() });
          if (prop === 'mint') return jest.fn().mockResolvedValue({ hash: '0x' + 'f'.repeat(64), wait: jest.fn() });
          return jest.fn().mockResolvedValue('0x');
        },
      }
    )
  );

  const mockGetDefaultProvider = jest.fn(() => ({
    getBalance: jest.fn().mockResolvedValue(BigInt('1000000000000000000')),
    getTransactionCount: jest.fn().mockResolvedValue(1),
    getNetwork: jest.fn().mockResolvedValue({ chainId: 1n, name: 'mainnet' }),
  }));

  const mockJsonRpcProvider = jest.fn(() => ({
    getBalance: jest.fn().mockResolvedValue(BigInt('1000000000000000000')),
    getTransactionCount: jest.fn().mockResolvedValue(1),
    getNetwork: jest.fn().mockResolvedValue({ chainId: 1n, name: 'mainnet' }),
  }));

  const mockIsAddress = jest.fn(addr => /^0x[a-fA-F0-9]{40}$/.test(addr));
  const mockKeccak256 = jest.fn(data => '0x' + 'a'.repeat(64));
  const mockToUtf8Bytes = jest.fn(text => new global.TextEncoder().encode(text));
  const mockVerifyMessage = jest.fn(() => '0x' + 'b'.repeat(40));
  const mockParseEther = jest.fn(v => BigInt(Math.floor(Number(v) * 1e18)));
  const mockFormatEther = jest.fn(v => (Number(v) / 1e18).toString());
  const mockParseUnits = jest.fn((v, decimals = 18) => BigInt(Math.floor(Number(v) * 10 ** decimals)));
  const mockFormatUnits = jest.fn((v, decimals = 18) => (Number(v) / 10 ** decimals).toString());
  const WalletRef = Object.assign(MockWallet, { createRandom: MockWallet.createRandom });

  const ZERO_ADDRESS = '0x0000000000000000000000000000000000000000';
  const MAX_UINT256 = BigInt('0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff');

  // Shared mock object used both at top level and as the `ethers` named export
  const ethersMock = {
    isAddress: mockIsAddress,
    keccak256: mockKeccak256,
    toUtf8Bytes: mockToUtf8Bytes,
    verifyMessage: mockVerifyMessage,
    Wallet: WalletRef,
    Contract: mockContractFactory,
    getDefaultProvider: mockGetDefaultProvider,
    JsonRpcProvider: mockJsonRpcProvider,
    parseEther: mockParseEther,
    formatEther: mockFormatEther,
    parseUnits: mockParseUnits,
    formatUnits: mockFormatUnits,
    ZeroAddress: ZERO_ADDRESS,
    MaxUint256: MAX_UINT256,
    // v5 compat: ethers.utils namespace
    utils: {
      keccak256: mockKeccak256,
      toUtf8Bytes: mockToUtf8Bytes,
      isAddress: mockIsAddress,
      formatEther: mockFormatEther,
      parseEther: mockParseEther,
      formatUnits: mockFormatUnits,
      parseUnits: mockParseUnits,
    },
  };

  return {
    // Named exports (ethers.js v6 style)
    ...ethersMock,

    // Also provide ethers object for imports like `import { ethers } from 'ethers'`
    ethers: ethersMock,
  };
});

// Mock Redis for caching
jest.mock('redis', () => ({
  createClient: jest.fn(() => ({
    connect: jest.fn().mockResolvedValue(true),
    get: jest.fn().mockResolvedValue(null),
    set: jest.fn().mockResolvedValue('OK'),
    del: jest.fn().mockResolvedValue(1),
    quit: jest.fn().mockResolvedValue('OK'),
  })),
}));

// Mock web3 provider
global.mockWeb3Provider = {
  request: jest.fn().mockImplementation(({ method }) => {
    switch (method) {
      case 'eth_accounts':
        return Promise.resolve([global.testFixtures.users.user1.walletAddress]);
      case 'eth_requestAccounts':
        return Promise.resolve([global.testFixtures.users.user1.walletAddress]);
      case 'eth_getBalance':
        return Promise.resolve('0xde0b6b3a7640000'); // 1 ETH
      case 'eth_sendTransaction':
        return Promise.resolve(global.testUtils.mockTransaction().hash);
      default:
        return Promise.resolve(null);
    }
  }),
};

// Setup and teardown hooks
beforeAll(async () => {
  // Global setup before all tests
  console.log('🧪 Starting Jest test environment');
});

afterAll(async () => {
  // Global cleanup after all tests
  console.log('🏁 Jest test environment complete');
});

beforeEach(() => {
  // Reset mocks before each test
  jest.clearAllMocks();
});

afterEach(() => {
  // Cleanup after each test if needed
});

// Custom Jest matchers
expect.extend({
  toBeValidAddress(received) {
    const isValid = /^0x[a-fA-F0-9]{40}$/.test(received);
    return {
      message: () => `expected ${received} to be a valid Ethereum address`,
      pass: isValid,
    };
  },

  toBeValidTransactionHash(received) {
    const isValid = /^0x[a-fA-F0-9]{64}$/.test(received);
    return {
      message: () => `expected ${received} to be a valid transaction hash`,
      pass: isValid,
    };
  },

  toHaveValidWeatherData(received) {
    const hasRequired =
      received.temperature !== undefined &&
      received.humidity !== undefined &&
      received.pressure !== undefined &&
      received.timestamp !== undefined;
    return {
      message: () => `expected weather reading to have required fields`,
      pass: hasRequired,
    };
  },

  /**
   * Custom matchers for Winston logger assertions (Task 113.9)
   * These matchers work with TestLogCapture instances from test-logger.ts
   */
  toHaveLogged(received, level, message) {
    // Check if received is a TestLogCapture-like object
    if (!received || typeof received.hasLog !== 'function') {
      return {
        pass: false,
        message: () => 'Expected a TestLogCapture instance with hasLog method',
      };
    }
    const pass = received.hasLog(level, message);
    const logs = received.getLogs ? received.getLogs() : [];
    return {
      pass,
      message: () =>
        pass
          ? `Expected not to find ${level} log containing "${message}"`
          : `Expected to find ${level} log containing "${message}"\n` +
            `Captured logs:\n${logs
              .map(l => `  [${l.level}] ${l.message}`)
              .join('\n') || '  (no logs captured)'}`,
    };
  },

  toHaveLoggedError(received, message) {
    if (!received || typeof received.hasLog !== 'function') {
      return {
        pass: false,
        message: () => 'Expected a TestLogCapture instance with hasLog method',
      };
    }
    const pass = received.hasLog('error', message);
    const errorLogs = received.getLogsByLevel ? received.getLogsByLevel('error') : [];
    return {
      pass,
      message: () =>
        pass
          ? `Expected not to find error log containing "${message}"`
          : `Expected to find error log containing "${message}"\n` +
            `Error logs:\n${errorLogs
              .map(l => `  ${l.message}`)
              .join('\n') || '  (no error logs captured)'}`,
    };
  },

  toHaveLoggedWarning(received, message) {
    if (!received || typeof received.hasLog !== 'function') {
      return {
        pass: false,
        message: () => 'Expected a TestLogCapture instance with hasLog method',
      };
    }
    const pass = received.hasLog('warn', message);
    return {
      pass,
      message: () =>
        pass
          ? `Expected not to find warning log containing "${message}"`
          : `Expected to find warning log containing "${message}"`,
    };
  },

  toHaveLoggedInfo(received, message) {
    if (!received || typeof received.hasLog !== 'function') {
      return {
        pass: false,
        message: () => 'Expected a TestLogCapture instance with hasLog method',
      };
    }
    const pass = received.hasLog('info', message);
    return {
      pass,
      message: () =>
        pass
          ? `Expected not to find info log containing "${message}"`
          : `Expected to find info log containing "${message}"`,
    };
  },

  toHaveLogCount(received, expectedCount) {
    if (!received || typeof received.getLogs !== 'function') {
      return {
        pass: false,
        message: () => 'Expected a TestLogCapture instance with getLogs method',
      };
    }
    const actualCount = received.getLogs().length;
    const pass = actualCount === expectedCount;
    return {
      pass,
      message: () =>
        pass
          ? `Expected not to have ${expectedCount} logs`
          : `Expected ${expectedCount} logs but found ${actualCount}`,
    };
  },
});

/**
 * Logger Testing Utilities (Task 113.9)
 *
 * Provides utilities for testing code that uses Winston loggers:
 * - createMockLogger(): Create a fully mocked logger for unit tests
 * - TestLoggerUtils: Helper functions for log capture and assertions
 */
global.TestLoggerUtils = {
  /**
   * Create a mock logger with jest.fn() methods for all log levels
   * @returns {Object} Mock logger object with mocked methods and assertion helpers
   */
  createMockLogger: () => {
    const calls = {
      error: [],
      warn: [],
      info: [],
      http: [],
      debug: [],
    };

    const createLogFn = level => {
      return jest.fn((message, meta) => {
        calls[level].push([message, meta]);
      });
    };

    return {
      error: createLogFn('error'),
      warn: createLogFn('warn'),
      info: createLogFn('info'),
      http: createLogFn('http'),
      debug: createLogFn('debug'),
      log: jest.fn((level, message, meta) => {
        if (calls[level]) {
          calls[level].push([message, meta]);
        }
      }),
      child: jest.fn(function () {
        return global.TestLoggerUtils.createMockLogger();
      }),
      _calls: calls,
      _clear: function () {
        Object.keys(calls).forEach(level => {
          calls[level] = [];
        });
        this.error.mockClear();
        this.warn.mockClear();
        this.info.mockClear();
        this.http.mockClear();
        this.debug.mockClear();
        this.log.mockClear();
      },
      _hasLogged: function (level, message) {
        return calls[level].some(
          ([msg]) => msg.toLowerCase().includes(message.toLowerCase())
        );
      },
      // TestLogCapture-compatible interface for custom matchers
      hasLog: function (level, message) {
        return this._hasLogged(level, message);
      },
      getLogs: function () {
        const allLogs = [];
        Object.entries(calls).forEach(([level, levelCalls]) => {
          levelCalls.forEach(([message, meta]) => {
            allLogs.push({ level, message, metadata: meta });
          });
        });
        return allLogs;
      },
      getLogsByLevel: function (level) {
        return (calls[level] || []).map(([message, meta]) => ({
          level,
          message,
          metadata: meta,
        }));
      },
    };
  },

  /**
   * Silence all Winston loggers for the current test
   * Sets SILENT_LOGS=true temporarily
   */
  silenceLogs: () => {
    const originalSilentLogs = process.env.SILENT_LOGS;
    process.env.SILENT_LOGS = 'true';
    return () => {
      process.env.SILENT_LOGS = originalSilentLogs;
    };
  },

  /**
   * Enable verbose logging for debugging a specific test
   * Sets DEBUG=true and SILENT_LOGS=false temporarily
   */
  enableDebugLogs: () => {
    const originalDebug = process.env.DEBUG;
    const originalSilentLogs = process.env.SILENT_LOGS;
    const originalLogLevel = process.env.LOG_LEVEL;

    process.env.DEBUG = 'true';
    process.env.SILENT_LOGS = 'false';
    process.env.LOG_LEVEL = 'debug';

    return () => {
      process.env.DEBUG = originalDebug;
      process.env.SILENT_LOGS = originalSilentLogs;
      process.env.LOG_LEVEL = originalLogLevel;
    };
  },
};

// Export test utilities for use in tests
module.exports = {
  testUtils: global.testUtils,
  testFixtures: global.testFixtures,
  mockWeb3Provider: global.mockWeb3Provider,
  TestLoggerUtils: global.TestLoggerUtils,
};
