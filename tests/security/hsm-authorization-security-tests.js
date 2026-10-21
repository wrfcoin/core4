/**
 * HSM Authorization Security Validation Tests
 *
 * Comprehensive test suite to verify the HSM authorization bypass vulnerability
 * has been completely fixed. Tests security against session hijacking and
 * unauthorized access while ensuring legitimate operations continue to work.
 *
 * SECURITY CONTEXT:
 * - Tests the fix for HSM session hijacking vulnerability
 * - Validates that unauthorized users cannot use sessions belonging to others
 * - Ensures proper audit logging for security violations
 * - Verifies performance impact is minimal
 */

const request = require('supertest');
const { expect } = require('chai');
const crypto = require('crypto');
const app = require('../../backend/server'); // Adjust path as needed
const EnterpriseHSMService =
  require('../../security/enterprise-hsm-service').default;

describe('HSM Authorization Security Validation', () => {
  let hsmService;
  let validUser1, validUser2;
  let validSession1, validSession2;
  let validKeyId;
  let stolenSessionId;

  before(async () => {
    // Initialize HSM service for testing
    hsmService = new EnterpriseHSMService({
      provider: 'aws-cloudhsm',
      endpoint: 'localhost:9000',
      keyStoreId: 'test-keys',
      sessionTimeout: 3600000,
      maxConcurrentSessions: 10,
      keyRotationInterval: 86400000 * 30,
      complianceLevel: 'FIPS-140-2-L3',
      quantumResistant: true,
    });

    await hsmService.initialize();

    // Set up test users and sessions
    validUser1 = {
      id: 'user_alice_123',
      username: 'alice',
      email: 'alice@wrfcoin.com',
    };

    validUser2 = {
      id: 'user_bob_456',
      username: 'bob',
      email: 'bob@wrfcoin.com',
    };

    // Generate test key
    const keyMetadata = await hsmService.generateKeyPair(
      'master-signing',
      'ECDSA-P256',
      2 // HIGH security level
    );
    validKeyId = keyMetadata.keyId;

    // Create legitimate sessions
    validSession1 = {
      sessionId: crypto.randomBytes(32).toString('hex'),
      userId: validUser1.id,
      createdAt: new Date(),
      expiresAt: new Date(Date.now() + 3600000),
      isActive: true,
    };

    validSession2 = {
      sessionId: crypto.randomBytes(32).toString('hex'),
      userId: validUser2.id,
      createdAt: new Date(),
      expiresAt: new Date(Date.now() + 3600000),
      isActive: true,
    };

    // Simulate stolen session scenario
    stolenSessionId = validSession1.sessionId;

    // Register sessions with HSM service
    hsmService.activeSessions.set(validSession1.sessionId, validSession1);
    hsmService.activeSessions.set(validSession2.sessionId, validSession2);
  });

  after(async () => {
    // Cleanup test data
    if (hsmService) {
      hsmService.activeSessions.clear();
      hsmService.activeKeys.clear();
    }
  });

  describe('1. PENETRATION TESTS - Session Hijacking Prevention', () => {
    it('SECURITY: Should block unauthorized user from using another users HSM session', async () => {
      // ATTACK SCENARIO: Bob tries to use Alice's session
      const maliciousPayload = {
        proposalData: {
          title: 'Malicious Proposal',
          description: 'This should be blocked',
        },
        sessionId: stolenSessionId, // Alice's session
        keyId: validKeyId,
      };

      const response = await request(app)
        .post('/api/governance/proposals/test-proposal-123/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser2)}`) // Bob's auth
        .send(maliciousPayload)
        .expect(403);

      expect(response.body).to.have.property('error');
      expect(response.body.error).to.include(
        'Session belongs to different user'
      );
      expect(response.body).to.have.property(
        'code',
        'HSM_SESSION_AUTHORIZATION_FAILED'
      );

      // Verify audit log contains security violation
      const auditEvents = hsmService.auditTrail.filter(
        event => event.action === 'UNAUTHORIZED_HSM_SESSION_ACCESS'
      );
      expect(auditEvents).to.have.length.at.least(1);
      expect(auditEvents[0].actor).to.equal(validUser2.id);
      expect(auditEvents[0].result).to.equal('failure');
    });

    it('SECURITY: Should block session access with stolen/invalid session ID', async () => {
      // ATTACK SCENARIO: Attacker uses forged session ID
      const invalidSessionId = crypto.randomBytes(32).toString('hex');

      const maliciousPayload = {
        transferData: {
          fromChain: 'ethereum',
          toChain: 'polygon',
          amount: '1000000', // Large amount
        },
        sessionId: invalidSessionId,
        keyId: validKeyId,
      };

      const response = await request(app)
        .post('/api/bridges/transactions/malicious-tx-789/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
        .send(maliciousPayload)
        .expect(401);

      expect(response.body).to.have.property('error');
      expect(response.body.error).to.include('Invalid or expired HSM session');
      expect(response.body).to.have.property('code', 'HSM_SESSION_INVALID');

      // Verify security violation was logged
      const auditEvents = hsmService.auditTrail.filter(
        event => event.action === 'UNAUTHORIZED_HSM_ACCESS_ATTEMPT'
      );
      expect(auditEvents).to.have.length.at.least(1);
    });

    it('SECURITY: Should block unauthenticated HSM access attempts', async () => {
      const unauthorizedPayload = {
        authData: {
          userId: validUser1.id,
          action: 'transfer',
          timestamp: new Date().toISOString(),
          nonce: 'malicious-nonce',
        },
        sessionId: validSession1.sessionId,
        keyId: validKeyId,
      };

      // No Authorization header - completely unauthenticated
      const response = await request(app)
        .post('/api/auth/sign')
        .send(unauthorizedPayload)
        .expect(401);

      expect(response.body).to.have.property('error');
      expect(response.body.error).to.include('Authentication required');
      expect(response.body).to.have.property('code', 'UNAUTHORIZED_HSM_ACCESS');
    });

    it('SECURITY: Should block cross-user data signing attempts', async () => {
      // ATTACK SCENARIO: Alice tries to sign data claiming to be Bob
      const crossUserPayload = {
        authData: {
          userId: validUser2.id, // Claiming to be Bob
          action: 'critical_operation',
          timestamp: new Date().toISOString(),
          nonce: crypto.randomBytes(16).toString('hex'),
        },
        sessionId: validSession1.sessionId, // Using Alice's session
        keyId: validKeyId,
      };

      const response = await request(app)
        .post('/api/auth/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`) // Alice's auth
        .send(crossUserPayload)
        .expect(403);

      expect(response.body).to.have.property('error');
      expect(response.body.error).to.include(
        'Cannot sign data for different user'
      );
      expect(response.body).to.have.property('code', 'USER_MISMATCH');
    });

    it('SECURITY: Should block expired session usage', async () => {
      // Create expired session
      const expiredSession = {
        sessionId: crypto.randomBytes(32).toString('hex'),
        userId: validUser1.id,
        createdAt: new Date(Date.now() - 7200000), // 2 hours ago
        expiresAt: new Date(Date.now() - 3600000), // Expired 1 hour ago
        isActive: false,
      };

      hsmService.activeSessions.set(expiredSession.sessionId, expiredSession);

      const payload = {
        weatherData: {
          temperature: 25.5,
          humidity: 60,
          pressure: 1013.25,
          windSpeed: 10,
        },
        sessionId: expiredSession.sessionId,
        keyId: validKeyId,
        validationLevel: 'enhanced',
      };

      const response = await request(app)
        .post('/api/weather/data/test-data-456/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
        .send(payload)
        .expect(401);

      expect(response.body).to.have.property('error');
      expect(response.body.error).to.include('Invalid or expired HSM session');
    });
  });

  describe('2. LEGITIMATE OPERATION TESTS - Authorized Access', () => {
    it('VALID: Should allow legitimate user to sign with their own session', async () => {
      const legitimatePayload = {
        proposalData: {
          title: 'Legitimate Governance Proposal',
          description: 'This proposal should be signed successfully',
          proposalType: 'TEXT',
        },
        sessionId: validSession1.sessionId,
        keyId: validKeyId,
      };

      const response = await request(app)
        .post('/api/governance/proposals/valid-proposal-123/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
        .send(legitimatePayload)
        .expect(200);

      expect(response.body).to.have.property('success', true);
      expect(response.body).to.have.property('signature');
      expect(response.body.signature).to.have.property('signatureData');
      expect(response.body.signature).to.have.property('algorithm');
      expect(response.body.signature).to.have.property('userId', validUser1.id);
      expect(response.body.signature).to.have.property('keyId', validKeyId);

      // Verify successful audit log
      const auditEvents = hsmService.auditTrail.filter(
        event =>
          event.action === 'SIGNING_OPERATION' && event.result === 'success'
      );
      expect(auditEvents).to.have.length.at.least(1);
    });

    it('VALID: Should allow legitimate cross-chain bridge operations', async () => {
      const validTransferPayload = {
        transferData: {
          fromChain: 'ethereum',
          toChain: 'polygon',
          amount: '100.50',
          fromAddress: '0x1234567890123456789012345678901234567890',
          toAddress: '0x0987654321098765432109876543210987654321',
          token: 'USDC',
        },
        sessionId: validSession2.sessionId,
        keyId: validKeyId,
      };

      const response = await request(app)
        .post('/api/bridges/transactions/bridge-tx-456/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser2)}`)
        .send(validTransferPayload)
        .expect(200);

      expect(response.body).to.have.property('success', true);
      expect(response.body).to.have.property('signedTransaction');
      expect(response.body.signedTransaction).to.have.property(
        'transactionId',
        'bridge-tx-456'
      );
      expect(response.body.signedTransaction).to.have.property(
        'userId',
        validUser2.id
      );
      expect(response.body.signedTransaction).to.have.property(
        'status',
        'signed'
      );
    });

    it('VALID: Should allow legitimate weather data signing', async () => {
      const validWeatherPayload = {
        weatherData: {
          temperature: 22.3,
          humidity: 65,
          pressure: 1015.2,
          windSpeed: 8.5,
          stationId: 'WRF_STATION_001',
          timestamp: new Date().toISOString(),
        },
        sessionId: validSession1.sessionId,
        keyId: validKeyId,
        validationLevel: 'premium',
      };

      const response = await request(app)
        .post('/api/weather/data/weather-123/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
        .send(validWeatherPayload)
        .expect(200);

      expect(response.body).to.have.property('success', true);
      expect(response.body).to.have.property('signedData');
      expect(response.body.signedData).to.have.property(
        'userId',
        validUser1.id
      );
      expect(response.body.signedData).to.have.property(
        'validationLevel',
        'premium'
      );
    });

    it('VALID: Should allow legitimate authentication data signing', async () => {
      const validAuthPayload = {
        authData: {
          userId: validUser1.id,
          action: 'account_verification',
          timestamp: new Date().toISOString(),
          nonce: crypto.randomBytes(16).toString('hex'),
        },
        sessionId: validSession1.sessionId,
        keyId: validKeyId,
      };

      const response = await request(app)
        .post('/api/auth/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
        .send(validAuthPayload)
        .expect(200);

      expect(response.body).to.have.property('success', true);
      expect(response.body).to.have.property('signedAuth');
      expect(response.body.signedAuth).to.have.property(
        'userId',
        validUser1.id
      );
    });
  });

  describe('3. EDGE CASE TESTS - Boundary Conditions', () => {
    it('EDGE: Should handle malformed session IDs gracefully', async () => {
      const malformedSessionIds = [
        null,
        undefined,
        '',
        'invalid-session-format',
        '12345', // Too short
        'x'.repeat(1000), // Too long
        { sessionId: 'object-instead-of-string' },
        [],
      ];

      for (const malformedSessionId of malformedSessionIds) {
        const payload = {
          proposalData: { title: 'Test', description: 'Test' },
          sessionId: malformedSessionId,
          keyId: validKeyId,
        };

        const response = await request(app)
          .post('/api/governance/proposals/edge-test/sign')
          .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
          .send(payload);

        expect(response.status).to.be.oneOf([400, 401, 500]);
        expect(response.body).to.have.property('error');
      }
    });

    it('EDGE: Should handle missing required parameters', async () => {
      const incompletePayloads = [
        { sessionId: validSession1.sessionId, keyId: validKeyId }, // Missing data
        { proposalData: { title: 'Test' }, keyId: validKeyId }, // Missing sessionId
        { proposalData: { title: 'Test' }, sessionId: validSession1.sessionId }, // Missing keyId
        {}, // Missing everything
      ];

      for (const payload of incompletePayloads) {
        const response = await request(app)
          .post('/api/governance/proposals/incomplete-test/sign')
          .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
          .send(payload)
          .expect(400);

        expect(response.body).to.have.property('error');
        expect(response.body.error).to.include('Missing required parameters');
      }
    });

    it('EDGE: Should handle non-existent key IDs', async () => {
      const nonExistentKeyId =
        'non-existent-key-' + crypto.randomBytes(16).toString('hex');

      const payload = {
        proposalData: {
          title: 'Test with invalid key',
          description: 'This should fail',
        },
        sessionId: validSession1.sessionId,
        keyId: nonExistentKeyId,
      };

      const response = await request(app)
        .post('/api/governance/proposals/invalid-key-test/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
        .send(payload)
        .expect(500);

      expect(response.body).to.have.property('error');
      // Should log security violation for invalid key access
      const auditEvents = hsmService.auditTrail.filter(
        event => event.action === 'INVALID_KEY_ACCESS_ATTEMPT'
      );
      expect(auditEvents).to.have.length.at.least(1);
    });

    it('EDGE: Should handle concurrent session access attempts', async () => {
      // Simulate multiple concurrent requests using the same session
      const concurrentRequests = [];

      for (let i = 0; i < 5; i++) {
        const payload = {
          authData: {
            userId: validUser1.id,
            action: `concurrent_test_${i}`,
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: validSession1.sessionId,
          keyId: validKeyId,
        };

        const request_promise = request(app)
          .post('/api/auth/sign')
          .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
          .send(payload);

        concurrentRequests.push(request_promise);
      }

      const responses = await Promise.all(concurrentRequests);

      // All should succeed since they're from the same legitimate user
      responses.forEach(response => {
        expect(response.status).to.equal(200);
        expect(response.body).to.have.property('success', true);
      });
    });
  });

  describe('4. PERFORMANCE IMPACT TESTS', () => {
    it('PERFORMANCE: Authorization validation should complete under 100ms', async () => {
      const payload = {
        proposalData: {
          title: 'Performance Test Proposal',
          description: 'Testing authorization performance',
        },
        sessionId: validSession1.sessionId,
        keyId: validKeyId,
      };

      const startTime = process.hrtime.bigint();

      const response = await request(app)
        .post('/api/governance/proposals/perf-test/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
        .send(payload)
        .expect(200);

      const endTime = process.hrtime.bigint();
      const durationMs = Number(endTime - startTime) / 1000000;

      expect(durationMs).to.be.below(100); // Should complete under 100ms
      expect(response.body).to.have.property('success', true);
    });

    it('PERFORMANCE: Security rejection should be fast (under 50ms)', async () => {
      // Test that security violations are rejected quickly
      const maliciousPayload = {
        proposalData: { title: 'Fast reject test' },
        sessionId: stolenSessionId, // Wrong user's session
        keyId: validKeyId,
      };

      const startTime = process.hrtime.bigint();

      await request(app)
        .post('/api/governance/proposals/fast-reject-test/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser2)}`)
        .send(maliciousPayload)
        .expect(403);

      const endTime = process.hrtime.bigint();
      const durationMs = Number(endTime - startTime) / 1000000;

      expect(durationMs).to.be.below(50); // Security rejections should be very fast
    });

    it('PERFORMANCE: Bulk legitimate operations should maintain performance', async () => {
      const bulkRequests = [];
      const startTime = process.hrtime.bigint();

      // Create 10 legitimate signing requests
      for (let i = 0; i < 10; i++) {
        const payload = {
          authData: {
            userId: validUser1.id,
            action: `bulk_operation_${i}`,
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: validSession1.sessionId,
          keyId: validKeyId,
        };

        bulkRequests.push(
          request(app)
            .post('/api/auth/sign')
            .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
            .send(payload)
        );
      }

      const responses = await Promise.all(bulkRequests);
      const endTime = process.hrtime.bigint();
      const totalDurationMs = Number(endTime - startTime) / 1000000;
      const avgDurationMs = totalDurationMs / responses.length;

      // All should succeed
      responses.forEach(response => {
        expect(response.status).to.equal(200);
      });

      // Average response time should be reasonable
      expect(avgDurationMs).to.be.below(150);
    });
  });

  describe('5. AUDIT LOG VALIDATION', () => {
    it('AUDIT: Should log all security violations with proper context', async () => {
      const initialAuditCount = hsmService.auditTrail.length;

      // Trigger a security violation
      await request(app)
        .post('/api/governance/proposals/audit-test/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser2)}`)
        .send({
          proposalData: { title: 'Audit test' },
          sessionId: validSession1.sessionId, // Wrong user's session
          keyId: validKeyId,
        })
        .expect(403);

      const finalAuditCount = hsmService.auditTrail.length;
      expect(finalAuditCount).to.be.above(initialAuditCount);

      // Find the security violation event
      const securityViolation = hsmService.auditTrail.find(
        event => event.action === 'UNAUTHORIZED_HSM_SESSION_ACCESS'
      );

      expect(securityViolation).to.exist;
      expect(securityViolation.actor).to.equal(validUser2.id);
      expect(securityViolation.result).to.equal('failure');
      expect(securityViolation.details).to.include(
        'Session belongs to different user'
      );
      expect(securityViolation.complianceContext).to.include('Session Owner');
    });

    it('AUDIT: Should log successful operations with user validation flag', async () => {
      const initialAuditCount = hsmService.auditTrail.length;

      await request(app)
        .post('/api/auth/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser1)}`)
        .send({
          authData: {
            userId: validUser1.id,
            action: 'audit_validation_test',
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: validSession1.sessionId,
          keyId: validKeyId,
        })
        .expect(200);

      const finalAuditCount = hsmService.auditTrail.length;
      expect(finalAuditCount).to.be.above(initialAuditCount);

      // Find the successful signing operation
      const successEvent = hsmService.auditTrail.find(
        event =>
          event.action === 'SIGNING_OPERATION' &&
          event.details.includes('User Validated: true')
      );

      expect(successEvent).to.exist;
      expect(successEvent.actor).to.equal(validUser1.id);
      expect(successEvent.result).to.equal('success');
    });
  });

  describe('6. API ENDPOINT COMPREHENSIVE TESTS', () => {
    it('API: All HSM endpoints should validate user authorization', async () => {
      const endpoints = [
        {
          path: '/api/governance/proposals/test/sign',
          payload: {
            proposalData: { title: 'Test' },
            sessionId: stolenSessionId,
            keyId: validKeyId,
          },
        },
        {
          path: '/api/bridges/transactions/test/sign',
          payload: {
            transferData: { fromChain: 'eth', toChain: 'polygon', amount: '1' },
            sessionId: stolenSessionId,
            keyId: validKeyId,
          },
        },
        {
          path: '/api/auth/sign',
          payload: {
            authData: {
              userId: validUser1.id,
              action: 'test',
              timestamp: new Date().toISOString(),
              nonce: '123',
            },
            sessionId: stolenSessionId,
            keyId: validKeyId,
          },
        },
        {
          path: '/api/weather/data/test/sign',
          payload: {
            weatherData: {
              temperature: 20,
              humidity: 50,
              pressure: 1013,
              windSpeed: 5,
            },
            sessionId: stolenSessionId,
            keyId: validKeyId,
          },
        },
      ];

      for (const endpoint of endpoints) {
        const response = await request(app)
          .post(endpoint.path)
          .set('Authorization', `Bearer ${generateTestJWT(validUser2)}`) // Wrong user
          .send(endpoint.payload);

        expect(response.status).to.be.oneOf([401, 403]);
        expect(response.body).to.have.property('error');
      }
    });
  });
});

/**
 * Helper function to generate test JWT tokens
 * @param {Object} user - User object
 * @returns {string} JWT token
 */
function generateTestJWT(user) {
  return require('jsonwebtoken').sign(
    {
      id: user.id,
      username: user.username,
      email: user.email,
    },
    process.env.JWT_SECRET || 'test-secret-key',
    { expiresIn: '1h' }
  );
}
