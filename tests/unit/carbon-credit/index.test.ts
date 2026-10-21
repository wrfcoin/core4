/**
 * Carbon Credit System Test Suite
 * Location: tests/unit/carbon-credit/index.test.ts
 *
 * This file exports and organizes all carbon credit related tests for Task 81.
 *
 * Test Coverage:
 * - Marketplace Service: Order book, matching, MEV protection, trade execution
 * - IoT Integration Service: Sensor validation, quality tiers, carbon credits
 * - HSM Transaction Service: Session management, signing, rate limiting
 * - Carbon Audit Service: Event logging, chain integrity, compliance reports
 */

// Re-export test helpers if needed
export const TEST_CONSTANTS = {
  // Valid Ethereum addresses for testing
  WALLET_ADDRESS_1: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
  WALLET_ADDRESS_2: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
  WALLET_ADDRESS_3: '0xdF3e18d64BC6A983f673Ab319CCaE4f1a57C7097',

  // Standard quantities for testing (in wei)
  ONE_CREDIT: BigInt('1000000000000000000'),
  TEN_CREDITS: BigInt('10000000000000000000'),
  HUNDRED_CREDITS: BigInt('100000000000000000000'),

  // Standard prices for testing (in wei)
  PRICE_50_WRF: BigInt('50000000000000000000'),
  PRICE_75_WRF: BigInt('75000000000000000000'),
  PRICE_100_WRF: BigInt('100000000000000000000'),

  // Test user IDs
  USER_1: 'user-1',
  USER_2: 'user-2',
  USER_3: 'user-3',

  // Test station IDs
  STATION_1: 'station-001',
  STATION_2: 'station-002',

  // Standard location for testing
  TEST_LOCATION: {
    latitude: 40.7128,
    longitude: -74.0060,
    elevation: 10,
    timezone: 'America/New_York',
  },
};

// Test data generators
export const generateTestOrder = (overrides: Partial<{
  userId: string;
  walletAddress: string;
  orderType: 'BUY' | 'SELL';
  creditType: string;
  quantity: bigint;
  price: bigint;
}> = {}) => ({
  userId: TEST_CONSTANTS.USER_1,
  walletAddress: TEST_CONSTANTS.WALLET_ADDRESS_1,
  orderType: 'BUY' as const,
  creditType: 'VCS',
  quantity: TEST_CONSTANTS.ONE_CREDIT,
  price: TEST_CONSTANTS.PRICE_50_WRF,
  ...overrides,
});

export const generateTestSensorReading = (overrides: Partial<{
  stationId: string;
  sensorType: string;
  value: number;
  unit: string;
  timestamp: Date;
}> = {}) => ({
  stationId: TEST_CONSTANTS.STATION_1,
  sensorType: 'TEMPERATURE',
  value: 22.5,
  unit: '°C',
  timestamp: new Date(),
  ...overrides,
});

export const generateTestStation = (overrides: Partial<{
  ownerId: string;
  name: string;
  location: {
    latitude: number;
    longitude: number;
    elevation: number;
    timezone: string;
  };
  supportedSensors: string[];
}> = {}) => ({
  ownerId: TEST_CONSTANTS.USER_1,
  name: 'Test Weather Station',
  location: TEST_CONSTANTS.TEST_LOCATION,
  supportedSensors: ['TEMPERATURE', 'HUMIDITY', 'PRESSURE'],
  ...overrides,
});

// Validation helpers
export const isValidEthereumAddress = (address: string): boolean => {
  return /^0x[a-fA-F0-9]{40}$/.test(address);
};

export const isValidHash = (hash: string): boolean => {
  return /^0x[a-fA-F0-9]{64}$/.test(hash);
};

// Mock data for integration tests
export const mockTradeData = {
  validTrade: {
    tradeId: 'trade-001',
    buyOrderId: 'order-buy-001',
    sellOrderId: 'order-sell-001',
    buyerId: TEST_CONSTANTS.USER_1,
    sellerId: TEST_CONSTANTS.USER_2,
    creditType: 'VCS',
    quantity: TEST_CONSTANTS.ONE_CREDIT,
    price: TEST_CONSTANTS.PRICE_50_WRF,
    totalValue: TEST_CONSTANTS.PRICE_50_WRF,
    fee: BigInt('150000000000000000'), // 0.3%
  },
};

export const mockSensorData = {
  validTemperature: {
    stationId: TEST_CONSTANTS.STATION_1,
    sensorType: 'TEMPERATURE',
    value: 22.5,
    unit: '°C',
    timestamp: new Date(),
  },
  validHumidity: {
    stationId: TEST_CONSTANTS.STATION_1,
    sensorType: 'HUMIDITY',
    value: 55,
    unit: '%',
    timestamp: new Date(),
  },
  validPressure: {
    stationId: TEST_CONSTANTS.STATION_1,
    sensorType: 'PRESSURE',
    value: 1013.25,
    unit: 'hPa',
    timestamp: new Date(),
  },
  invalidTemperature: {
    stationId: TEST_CONSTANTS.STATION_1,
    sensorType: 'TEMPERATURE',
    value: 200, // Out of range
    unit: '°C',
    timestamp: new Date(),
  },
};

// Test assertions
export const assertValidAuditEvent = (event: {
  id?: string;
  eventType?: string;
  timestamp?: Date;
  hash?: string;
}) => {
  expect(event.id).toBeDefined();
  expect(event.eventType).toBeDefined();
  expect(event.timestamp).toBeInstanceOf(Date);
  expect(event.hash).toBeDefined();
};

export const assertValidSignedTransaction = (signedTx: {
  signature?: string;
  transactionHash?: string;
  nonce?: number;
}) => {
  expect(signedTx.signature).toBeDefined();
  expect(signedTx.signature).toMatch(/^0x[a-fA-F0-9]+$/);
  expect(signedTx.transactionHash).toBeDefined();
};

/**
 * Test Suite Summary:
 *
 * 1. marketplace-service.test.ts
 *    - Order creation and validation
 *    - Order book management (bid/ask sorting)
 *    - Order matching and trade execution
 *    - Order cancellation
 *    - MEV protection (commit-reveal)
 *    - Market data and statistics
 *    - Trade settlement
 *    - User orders and trades
 *    - Event emission
 *
 * 2. iot-integration-service.test.ts
 *    - Station registration and management
 *    - Sensor data validation (range, physics-based)
 *    - Quality tier calculation
 *    - Carbon credit calculation
 *    - Air Quality Index (AQI) calculation
 *    - Batch processing
 *    - Event emission
 *
 * 3. hsm-transaction-service.test.ts
 *    - Session management (create, destroy, status)
 *    - Transaction signing
 *    - Rate limiting
 *    - Security levels
 *    - Signature verification
 *    - Audit logging
 *    - Transaction types
 *    - Error handling
 *    - Concurrent operations
 *
 * 4. carbon-audit-service.test.ts
 *    - Event logging (all types)
 *    - Audit chain integrity
 *    - Event queries
 *    - Helper logging methods
 *    - Summary generation
 *    - Compliance report generation
 *    - Credit lifecycle tracking
 *    - Security event handling
 *    - Export and archive
 */

// Ensure this test file has at least one test to satisfy Jest
describe('Carbon Credit Test Suite Constants', () => {
  it('should export valid test constants', () => {
    expect(TEST_CONSTANTS.WALLET_ADDRESS_1).toMatch(/^0x[a-fA-F0-9]{40}$/);
    expect(TEST_CONSTANTS.WALLET_ADDRESS_2).toMatch(/^0x[a-fA-F0-9]{40}$/);
    expect(TEST_CONSTANTS.ONE_CREDIT).toBe(BigInt('1000000000000000000'));
  });

  it('should export valid test data generators', () => {
    const order = generateTestOrder();
    expect(order.userId).toBe(TEST_CONSTANTS.USER_1);
    expect(order.orderType).toBe('BUY');
  });

  it('should export valid validation helpers', () => {
    expect(isValidEthereumAddress(TEST_CONSTANTS.WALLET_ADDRESS_1)).toBe(true);
    expect(isValidEthereumAddress('invalid')).toBe(false);
    expect(isValidHash('0x' + 'a'.repeat(64))).toBe(true);
  });
});
