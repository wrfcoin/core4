/**
 * K6 Test Helper Utilities for WRFCoin Load Testing
 * Common utilities for load testing scenarios
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import {
  randomIntBetween,
  randomItem,
  randomString,
} from 'https://jslib.k6.io/k6-utils/1.2.0/index.js';
import { SERVICE_REGISTRY } from '../k6.config.js';

/**
 * Service Health Check Utility
 */
export class ServiceHealthChecker {
  constructor(services = SERVICE_REGISTRY) {
    this.services = services;
    this.healthCache = new Map();
    this.cacheTimeout = 30000; // 30 second cache
  }

  async checkService(serviceName, timeout = 5000) {
    const service = this.services[serviceName];
    if (!service || !service.port) {
      return { healthy: false, error: 'Service not configured' };
    }

    const cacheKey = `${serviceName}_health`;
    const cached = this.healthCache.get(cacheKey);

    if (cached && Date.now() - cached.timestamp < this.cacheTimeout) {
      return cached.result;
    }

    try {
      const response = http.get(`${service.host}:${service.port}/health`, {
        timeout: `${timeout}ms`,
        tags: { service: serviceName, operation: 'health_check' },
      });

      const result = {
        healthy: response.status === 200,
        status: response.status,
        responseTime: response.timings.duration,
        timestamp: Date.now(),
      };

      this.healthCache.set(cacheKey, { result, timestamp: Date.now() });
      return result;
    } catch (error) {
      const result = {
        healthy: false,
        error: error.message,
        timestamp: Date.now(),
      };

      this.healthCache.set(cacheKey, { result, timestamp: Date.now() });
      return result;
    }
  }

  async checkAllServices() {
    const results = {};

    for (const serviceName of Object.keys(this.services)) {
      results[serviceName] = await this.checkService(serviceName);
    }

    return results;
  }
}

/**
 * Authentication Helper
 */
export class AuthHelper {
  constructor() {
    this.tokens = new Map();
    this.tokenTimeout = 3600000; // 1 hour
  }

  generateTestToken(userId = null) {
    const id = userId || `user-${randomIntBetween(1, 1000)}`;
    return `test-token-${id}-${randomString(16)}`;
  }

  async authenticateUser(username, password = 'testpass') {
    const authPayload = {
      username,
      password,
      client_id: 'k6_load_test',
      grant_type: 'password',
    };

    const response = http.post(
      `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/auth/token`,
      JSON.stringify(authPayload),
      {
        headers: { 'Content-Type': 'application/json' },
        tags: { operation: 'authentication' },
      }
    );

    if (response.status === 200) {
      const body = JSON.parse(response.body);
      const token = {
        access_token: body.access_token,
        refresh_token: body.refresh_token,
        expires_at: Date.now() + body.expires_in * 1000,
      };

      this.tokens.set(username, token);
      return token;
    }

    throw new Error(`Authentication failed: ${response.status}`);
  }

  getValidToken(username) {
    const token = this.tokens.get(username);

    if (!token || Date.now() > token.expires_at) {
      return this.generateTestToken(username); // Fallback to test token
    }

    return token.access_token;
  }
}

/**
 * Load Test Data Manager
 */
export class TestDataManager {
  constructor() {
    this.weatherStations = this.generateWeatherStations(100);
    this.testUsers = this.generateTestUsers(50);
    this.walletAddresses = this.generateWalletAddresses(100);
  }

  generateWeatherStations(count) {
    const stations = [];

    for (let i = 0; i < count; i++) {
      stations.push({
        id: `WS_${String(i + 1).padStart(3, '0')}`,
        name: `Test Weather Station ${i + 1}`,
        location: {
          lat: randomIntBetween(-90, 90) + Math.random(),
          lon: randomIntBetween(-180, 180) + Math.random(),
          city: `City_${i + 1}`,
        },
        status: randomItem(['active', 'maintenance', 'offline']),
        owner: `user-${randomIntBetween(1, 50)}`,
      });
    }

    return stations;
  }

  generateTestUsers(count) {
    const users = [];

    for (let i = 0; i < count; i++) {
      users.push({
        id: `user-${String(i + 1).padStart(3, '0')}`,
        username: `testuser${i + 1}`,
        email: `testuser${i + 1}@wrfcoin.test`,
        role: randomItem([
          'weather_station_owner',
          'compute_provider',
          'carbon_trader',
          'nft_creator',
        ]),
        wallet: `0x${randomString(40)}`,
        token: `test-token-${String(i + 1).padStart(3, '0')}`,
      });
    }

    return users;
  }

  generateWalletAddresses(count) {
    const addresses = [];

    for (let i = 0; i < count; i++) {
      addresses.push(`0x${randomString(40)}`);
    }

    return addresses;
  }

  getRandomStation() {
    return randomItem(this.weatherStations);
  }

  getRandomUser() {
    return randomItem(this.testUsers);
  }

  getRandomWallet() {
    return randomItem(this.walletAddresses);
  }
}

/**
 * Performance Metrics Collector
 */
export class MetricsCollector {
  constructor() {
    this.metrics = [];
    this.thresholds = new Map();
  }

  addMetric(name, value, tags = {}) {
    this.metrics.push({
      name,
      value,
      tags,
      timestamp: Date.now(),
    });
  }

  setThreshold(metricName, threshold) {
    this.thresholds.set(metricName, threshold);
  }

  evaluateThreshold(metricName) {
    const threshold = this.thresholds.get(metricName);
    if (!threshold) return null;

    const relevantMetrics = this.metrics.filter(m => m.name === metricName);
    if (relevantMetrics.length === 0) return null;

    const values = relevantMetrics.map(m => m.value);
    const avg = values.reduce((a, b) => a + b, 0) / values.length;
    const p95 = percentile(values, 95);
    const p99 = percentile(values, 99);

    return {
      passed: this.evaluateThresholdCondition(threshold, { avg, p95, p99 }),
      metrics: { avg, p95, p99, count: values.length },
    };
  }

  evaluateThresholdCondition(threshold, metrics) {
    // Simple threshold evaluation (can be extended)
    if (threshold.includes('p95<')) {
      const limit = parseFloat(threshold.split('p95<')[1]);
      return metrics.p95 < limit;
    }

    if (threshold.includes('avg<')) {
      const limit = parseFloat(threshold.split('avg<')[1]);
      return metrics.avg < limit;
    }

    return true; // Default pass
  }
}

/**
 * Utility Functions
 */

export function percentile(values, p) {
  const sorted = values.slice().sort((a, b) => a - b);
  const index = (p / 100) * (sorted.length - 1);

  if (Math.floor(index) === index) {
    return sorted[index];
  }

  const lower = sorted[Math.floor(index)];
  const upper = sorted[Math.ceil(index)];
  return lower + (upper - lower) * (index - Math.floor(index));
}

export function formatBytes(bytes) {
  if (bytes === 0) return '0 Bytes';

  const k = 1024;
  const sizes = ['Bytes', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));

  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

export function formatDuration(ms) {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  if (ms < 3600000) return `${(ms / 60000).toFixed(1)}m`;
  return `${(ms / 3600000).toFixed(1)}h`;
}

export function generateCorrelationId() {
  return `k6-${Date.now()}-${randomString(8)}`;
}

/**
 * Request Builder Helper
 */
export class RequestBuilder {
  constructor(baseUrl) {
    this.baseUrl = baseUrl;
    this.headers = {
      'Content-Type': 'application/json',
      'User-Agent': 'K6-WRFCoin-LoadTest/1.0',
    };
    this.timeout = '30s';
  }

  withAuth(token) {
    this.headers['Authorization'] = `Bearer ${token}`;
    return this;
  }

  withHeader(key, value) {
    this.headers[key] = value;
    return this;
  }

  withTimeout(timeout) {
    this.timeout = timeout;
    return this;
  }

  withCorrelationId() {
    this.headers['X-Correlation-ID'] = generateCorrelationId();
    return this;
  }

  get(endpoint, tags = {}) {
    return http.get(`${this.baseUrl}${endpoint}`, {
      headers: this.headers,
      timeout: this.timeout,
      tags,
    });
  }

  post(endpoint, payload, tags = {}) {
    return http.post(`${this.baseUrl}${endpoint}`, JSON.stringify(payload), {
      headers: this.headers,
      timeout: this.timeout,
      tags,
    });
  }

  put(endpoint, payload, tags = {}) {
    return http.put(`${this.baseUrl}${endpoint}`, JSON.stringify(payload), {
      headers: this.headers,
      timeout: this.timeout,
      tags,
    });
  }

  delete(endpoint, tags = {}) {
    return http.del(`${this.baseUrl}${endpoint}`, null, {
      headers: this.headers,
      timeout: this.timeout,
      tags,
    });
  }
}

/**
 * Load Test Patterns
 */
export class LoadPatterns {
  static rampUp(targetVUs, duration) {
    return {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: `${duration / 2}s`, target: targetVUs },
        { duration: `${duration / 2}s`, target: targetVUs },
      ],
    };
  }

  static constantLoad(vus, duration) {
    return {
      executor: 'constant-vus',
      vus: vus,
      duration: `${duration}s`,
    };
  }

  static arrivalRate(rate, duration) {
    return {
      executor: 'constant-arrival-rate',
      rate: rate,
      timeUnit: '1s',
      duration: `${duration}s`,
      preAllocatedVUs: Math.ceil(rate * 1.5),
      maxVUs: Math.ceil(rate * 2),
    };
  }

  static spike(normalVUs, spikeVUs, spikeDuration) {
    return {
      executor: 'ramping-vus',
      startVUs: normalVUs,
      stages: [
        { duration: '30s', target: normalVUs },
        { duration: '10s', target: spikeVUs },
        { duration: `${spikeDuration}s`, target: spikeVUs },
        { duration: '10s', target: normalVUs },
        { duration: '30s', target: normalVUs },
      ],
    };
  }
}

/**
 * Response Validator
 */
export class ResponseValidator {
  static validateApiResponse(response, expectedStatus = 200) {
    return check(response, {
      'Status is correct': r => r.status === expectedStatus,
      'Response time OK': r => r.timings.duration < 5000,
      'Response has body': r => r.body && r.body.length > 0,
      'Content-Type is JSON': r =>
        r.headers['Content-Type'] &&
        r.headers['Content-Type'].includes('application/json'),
    });
  }

  static validateSensorResponse(response) {
    return check(response, {
      'Sensor data accepted': r => r.status === 200 || r.status === 201,
      'Response time < 100ms': r => r.timings.duration < 100,
      'Reading ID present': r => {
        try {
          const body = JSON.parse(r.body);
          return body.readingId !== undefined;
        } catch (e) {
          return false;
        }
      },
    });
  }

  static validateWalletResponse(response) {
    return check(response, {
      'Wallet operation successful': r => r.status === 200,
      'Transaction data present': r => {
        try {
          const body = JSON.parse(r.body);
          return (
            body.transactionHash !== undefined || body.balance !== undefined
          );
        } catch (e) {
          return false;
        }
      },
    });
  }
}

// Export all utilities
export default {
  ServiceHealthChecker,
  AuthHelper,
  TestDataManager,
  MetricsCollector,
  RequestBuilder,
  LoadPatterns,
  ResponseValidator,
  percentile,
  formatBytes,
  formatDuration,
  generateCorrelationId,
};
