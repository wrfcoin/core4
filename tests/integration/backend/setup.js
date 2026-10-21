/**
 * Integration Test Setup
 * Configures test environment for integration testing with real database connections
 */

const { Pool } = require('pg');
const fs = require('fs');
const path = require('path');

// Set test environment variables
process.env.NODE_ENV = 'test';
process.env.LOG_LEVEL = 'error'; // Reduce log noise during tests
process.env.DATABASE_URL =
  process.env.TEST_DATABASE_URL || 'postgresql://localhost:5432/wrfcoin_test';

// Deterministic test-ID counter (#1206).  Each call to _nextId() returns
// a monotonically increasing integer, so IDs are unique across a single
// test run without relying on Date.now() or Math.random().
let _testIdCounter = 0;
function _nextId(prefix) {
  return `${prefix}_${String(++_testIdCounter).padStart(6, '0')}`;
}

// Global test utilities
global.IntegrationTestUtils = {
  // Database utilities
  async createTestDatabase() {
    const pool = new Pool({
      connectionString: process.env.DATABASE_URL,
      max: 5,
      idleTimeoutMillis: 10000,
      connectionTimeoutMillis: 5000,
    });

    try {
      await pool.query('SELECT 1');
      return pool;
    } catch (error) {
      console.warn(
        'Database not available for integration tests:',
        error.message
      );
      return null;
    }
  },

  async cleanupTestData(pool) {
    if (!pool) return;

    try {
      // Clean up test data in reverse dependency order
      await pool.query("DELETE FROM votes WHERE id LIKE 'test_%'");
      await pool.query(
        "DELETE FROM governance_proposals WHERE id LIKE 'test_%'"
      );
      await pool.query("DELETE FROM transactions WHERE id LIKE 'test_%'");
      await pool.query("DELETE FROM blocks WHERE id LIKE 'test_%'");
      await pool.query(
        "DELETE FROM weather_data WHERE station_id LIKE 'test_%'"
      );
      await pool.query("DELETE FROM nfts WHERE id LIKE 'test_%'");
      await pool.query("DELETE FROM wallets WHERE address LIKE 'test_%'");
    } catch (error) {
      console.warn('Error cleaning test data:', error.message);
    }
  },

  // Mock data generators
  generateTestWallet() {
    const id = _nextId('test_wallet');
    return {
      address: id,
      balance: 5000,
      network: 'testnet',
      created_at: '2026-01-01T00:00:00.000Z',
    };
  },

  generateTestTransaction() {
    const seq = _testIdCounter + 1; // peek so sub-ids are related
    return {
      id: _nextId('test_tx'),
      from_address: `test_from_${String(seq).padStart(6, '0')}`,
      to_address: `test_to_${String(seq).padStart(6, '0')}`,
      amount: 500,
      fee: 5,
      status: 'confirmed',
      timestamp: '2026-01-01T00:00:00.000Z',
    };
  },

  generateTestWeatherData() {
    return {
      station_id: _nextId('test_station'),
      timestamp: '2026-01-01T00:00:00.000Z',
      temperature: 25.0,
      humidity: 50.0,
      pressure: 1013.25,
      wind_speed: 10.0,
      wind_direction: 180,
      location: {
        latitude: 40.7128,
        longitude: -74.006,
      },
    };
  },

  generateTestNFT() {
    const seq = _testIdCounter + 1;
    return {
      id: _nextId('test_nft'),
      title: `Test NFT ${String(seq).padStart(6, '0')}`,
      description: 'Integration test NFT',
      creator: `test_creator_${String(seq).padStart(6, '0')}`,
      owner: `test_owner_${String(seq).padStart(6, '0')}`,
      price: 500,
      category: 'test',
      created_at: '2026-01-01T00:00:00.000Z',
    };
  },

  // API test helpers
  async makeApiRequest(app, method, endpoint, data = null, headers = {}) {
    const request = require('supertest')(app);

    const defaultHeaders = {
      'Content-Type': 'application/json',
      'X-API-Key': 'test-integration-key',
      ...headers,
    };

    let req;
    switch (method.toLowerCase()) {
      case 'get':
        req = request.get(endpoint);
        break;
      case 'post':
        req = request.post(endpoint);
        break;
      case 'put':
        req = request.put(endpoint);
        break;
      case 'delete':
        req = request.delete(endpoint);
        break;
      default:
        throw new Error(`Unsupported HTTP method: ${method}`);
    }

    // Add headers
    Object.entries(defaultHeaders).forEach(([key, value]) => {
      req.set(key, value);
    });

    // Add data for POST/PUT requests
    if (data && ['post', 'put'].includes(method.toLowerCase())) {
      req.send(data);
    }

    return req;
  },

  // Assertion helpers
  expectValidApiResponse(response, expectedStatus = 200) {
    expect(response.status).toBe(expectedStatus);
    expect(response.body).toBeDefined();

    if (expectedStatus >= 200 && expectedStatus < 300) {
      expect(response.body.success).toBe(true);
      expect(response.body.data).toBeDefined();
    } else {
      expect(response.body.success).toBe(false);
      expect(response.body.error).toBeDefined();
    }
  },

  expectValidPaginatedResponse(response, expectedStatus = 200) {
    this.expectValidApiResponse(response, expectedStatus);

    if (expectedStatus >= 200 && expectedStatus < 300) {
      expect(response.body.data.items).toBeInstanceOf(Array);
      expect(response.body.data.pagination).toBeDefined();
      expect(response.body.data.pagination.page).toBeGreaterThan(0);
      expect(response.body.data.pagination.limit).toBeGreaterThan(0);
      expect(response.body.data.pagination.total).toBeGreaterThanOrEqual(0);
    }
  },
};

// Increase timeout for integration tests
jest.setTimeout(30000);

// Global error handler for unhandled promise rejections
process.on('unhandledRejection', (reason, promise) => {
  console.error('Unhandled Rejection at:', promise, 'reason:', reason);
});

// Console log setup
console.log('Integration test setup completed');
console.log('Database URL:', process.env.DATABASE_URL);
console.log('Node environment:', process.env.NODE_ENV);
