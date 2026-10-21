/**
 * Jest Setup for HSM Security Tests
 *
 * Global setup configuration for HSM security validation tests.
 * Provides common utilities, mocks, and security test helpers.
 */

const crypto = require('crypto');
const jwt = require('jsonwebtoken');

// Global test configuration
global.SECURITY_TEST_CONFIG = {
  HSM_ENDPOINT: process.env.HSM_TEST_ENDPOINT || 'localhost:9000',
  JWT_SECRET: process.env.TEST_JWT_SECRET || 'hsm-security-test-key-12345',
  SESSION_TIMEOUT: 3600000, // 1 hour
  MAX_CONCURRENT_SESSIONS: 100,
  KEY_ROTATION_INTERVAL: 86400000 * 30, // 30 days
  COMPLIANCE_LEVEL: 'FIPS-140-2-L3',
};

// Global security test utilities
global.SecurityTestUtils = {
  /**
   * Generate a test user with security context
   */
  createTestUser: (overrides = {}) => {
    return {
      id: crypto.randomBytes(16).toString('hex'),
      username: `test_user_${Date.now()}`,
      email: `test.${Date.now()}@wrfcoin.com`,
      role: 'user',
      clearanceLevel: 'basic',
      ...overrides,
    };
  },

  /**
   * Generate a test HSM session
   */
  createTestSession: (userId, overrides = {}) => {
    return {
      sessionId: crypto.randomBytes(32).toString('hex'),
      userId: userId,
      createdAt: new Date(),
      expiresAt: new Date(
        Date.now() + global.SECURITY_TEST_CONFIG.SESSION_TIMEOUT
      ),
      isActive: true,
      securityLevel: 'standard',
      ...overrides,
    };
  },

  /**
   * Generate test JWT token
   */
  generateTestJWT: (user, options = {}) => {
    return jwt.sign(
      {
        id: user.id,
        username: user.username,
        email: user.email,
        role: user.role || 'user',
        clearanceLevel: user.clearanceLevel || 'basic',
      },
      global.SECURITY_TEST_CONFIG.JWT_SECRET,
      {
        expiresIn: '1h',
        ...options,
      }
    );
  },

  /**
   * Create malicious payload for testing
   */
  createMaliciousPayload: (basePayload, attackVector = 'session_hijack') => {
    const attacks = {
      session_hijack: {
        ...basePayload,
        sessionId: crypto.randomBytes(32).toString('hex'), // Different session
      },
      user_impersonation: {
        ...basePayload,
        userId: 'malicious_user_' + crypto.randomBytes(8).toString('hex'),
      },
      privilege_escalation: {
        ...basePayload,
        role: 'admin',
        clearanceLevel: 'privileged',
      },
      injection_attack: {
        ...basePayload,
        sessionId: "'; DROP TABLE sessions; --",
      },
    };

    return attacks[attackVector] || basePayload;
  },

  /**
   * Validate security response
   */
  validateSecurityResponse: (response, expectedBlocked = true) => {
    if (expectedBlocked) {
      expect(response.status).toBeOneOf([401, 403]);
      expect(response.body).toHaveProperty('error');
      expect(response.body).toHaveProperty('code');
    } else {
      expect(response.status).toBe(200);
      expect(response.body).toHaveProperty('success', true);
    }
  },

  /**
   * Create performance test payload
   */
  createPerformancePayload: (count = 100) => {
    const payloads = [];
    for (let i = 0; i < count; i++) {
      payloads.push({
        authData: {
          userId: `perf_user_${i}`,
          action: `performance_test_${i}`,
          timestamp: new Date().toISOString(),
          nonce: crypto.randomBytes(16).toString('hex'),
        },
        sessionId: crypto.randomBytes(32).toString('hex'),
        keyId: 'perf_test_key',
      });
    }
    return payloads;
  },

  /**
   * Measure response time
   */
  measureResponseTime: async requestPromise => {
    const startTime = process.hrtime.bigint();
    const response = await requestPromise;
    const endTime = process.hrtime.bigint();
    const durationMs = Number(endTime - startTime) / 1000000;

    return {
      response,
      duration: durationMs,
    };
  },

  /**
   * Generate audit event matcher
   */
  createAuditEventMatcher: (action, actor, result = 'failure') => {
    return {
      action: expect.stringContaining(action),
      actor: actor,
      result: result,
      timestamp: expect.any(Date),
      details: expect.any(String),
    };
  },
};

// Global security assertions
global.SecurityAssertions = {
  /**
   * Assert that an attack was properly blocked
   */
  expectAttackBlocked: (response, attackType = 'generic') => {
    expect(response.status).toBeOneOf([401, 403]);
    expect(response.body).toHaveProperty('error');
    expect(response.body.error).toMatch(
      /unauthorized|forbidden|access denied|session/i
    );

    // Log the blocked attack for reporting
    console.log(
      `✅ ${attackType} attack properly blocked: ${response.body.error}`
    );
  },

  /**
   * Assert that legitimate operation succeeded
   */
  expectLegitimateSuccess: (response, userId) => {
    expect(response.status).toBe(200);
    expect(response.body).toHaveProperty('success', true);

    // Verify user context is preserved
    if (response.body.signature) {
      expect(response.body.signature.userId).toBe(userId);
    } else if (response.body.signedAuth) {
      expect(response.body.signedAuth.userId).toBe(userId);
    } else if (response.body.signedTransaction) {
      expect(response.body.signedTransaction.userId).toBe(userId);
    }

    console.log(`✅ Legitimate operation succeeded for user: ${userId}`);
  },

  /**
   * Assert performance metrics are within acceptable limits
   */
  expectPerformanceWithinLimits: (duration, maxDuration = 100) => {
    expect(duration).toBeLessThan(maxDuration);
    console.log(
      `✅ Performance within limits: ${duration.toFixed(2)}ms (limit: ${maxDuration}ms)`
    );
  },

  /**
   * Assert audit log contains security event
   */
  expectSecurityAuditEvent: (auditTrail, expectedEvent) => {
    const matchingEvents = auditTrail.filter(
      event =>
        event.action.includes(expectedEvent.action) &&
        event.actor === expectedEvent.actor &&
        event.result === expectedEvent.result
    );

    expect(matchingEvents.length).toBeGreaterThan(0);
    console.log(`✅ Security audit event logged: ${expectedEvent.action}`);
  },
};

// Custom Jest matchers for security testing
expect.extend({
  toBeOneOf(received, validValues) {
    const pass = validValues.includes(received);
    return {
      message: () =>
        `expected ${received} to be one of ${validValues.join(', ')}`,
      pass,
    };
  },

  toHaveSecurityError(received) {
    const pass =
      received.status >= 400 &&
      received.body &&
      received.body.error &&
      received.body.code;
    return {
      message: () => `expected response to have security error format`,
      pass,
    };
  },

  toIndicateSecurityViolation(received) {
    const securityKeywords = [
      'unauthorized',
      'forbidden',
      'access denied',
      'session',
      'authentication',
      'authorization',
    ];

    const errorMessage = received.body?.error?.toLowerCase() || '';
    const pass = securityKeywords.some(keyword =>
      errorMessage.includes(keyword)
    );

    return {
      message: () => `expected error message to indicate security violation`,
      pass,
    };
  },
});

// Global test hooks
beforeAll(() => {
  console.log('\n🔐 HSM Security Test Suite Starting...');
  console.log('=====================================');

  // Set process environment for security testing
  process.env.NODE_ENV = 'test';
  process.env.HSM_ENDPOINT = global.SECURITY_TEST_CONFIG.HSM_ENDPOINT;
  process.env.JWT_SECRET = global.SECURITY_TEST_CONFIG.JWT_SECRET;
  process.env.SILENT_LOGS = 'true'; // Suppress Winston logger output during tests

  // Initialize security test metrics
  global.SECURITY_METRICS = {
    totalAttacksBlocked: 0,
    totalLegitimateOperations: 0,
    performanceViolations: 0,
    auditEventsGenerated: 0,
  };
});

afterAll(() => {
  console.log('\n📊 Security Test Suite Metrics:');
  console.log(
    `   Attacks Blocked: ${global.SECURITY_METRICS.totalAttacksBlocked}`
  );
  console.log(
    `   Legitimate Operations: ${global.SECURITY_METRICS.totalLegitimateOperations}`
  );
  console.log(
    `   Performance Violations: ${global.SECURITY_METRICS.performanceViolations}`
  );
  console.log(
    `   Audit Events: ${global.SECURITY_METRICS.auditEventsGenerated}`
  );
  console.log('🔐 HSM Security Test Suite Complete\n');
});

// Error handling for unhandled rejections in security tests
process.on('unhandledRejection', (reason, promise) => {
  console.error('Unhandled Rejection in security test:', reason);
  throw reason;
});

// Security test environment validation
console.log('🔧 Security Test Environment:');
console.log(`   HSM Endpoint: ${global.SECURITY_TEST_CONFIG.HSM_ENDPOINT}`);
console.log(
  `   Session Timeout: ${global.SECURITY_TEST_CONFIG.SESSION_TIMEOUT}ms`
);
console.log(
  `   Compliance Level: ${global.SECURITY_TEST_CONFIG.COMPLIANCE_LEVEL}`
);
console.log(
  `   Max Sessions: ${global.SECURITY_TEST_CONFIG.MAX_CONCURRENT_SESSIONS}`
);
console.log('✅ Security test environment initialized\n');
