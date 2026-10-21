/**
 * HSM Edge Case Security Tests
 *
 * Specialized tests for edge cases, error conditions, and boundary scenarios
 * that could potentially bypass the HSM authorization fix. Tests unusual
 * attack vectors and ensures robust security in all conditions.
 *
 * EDGE CASE CATEGORIES:
 * 1. Malformed Request Attacks
 * 2. Race Condition Exploits
 * 3. Memory Corruption Attempts
 * 4. Protocol Confusion Attacks
 * 5. Cryptographic Edge Cases
 * 6. Session State Manipulation
 */

const request = require('supertest');
const { expect } = require('chai');
const crypto = require('crypto');
const sinon = require('sinon');
const app = require('../../backend/server');
const EnterpriseHSMService =
  require('../../security/enterprise-hsm-service').default;

describe('HSM Edge Case Security Tests', () => {
  let hsmService;
  let validUser;
  let validSession;
  let validKeyId;
  let clock;

  before(async () => {
    hsmService = new EnterpriseHSMService({
      provider: 'aws-cloudhsm',
      endpoint: 'localhost:9000',
      keyStoreId: 'edge-case-test-keys',
      sessionTimeout: 3600000,
      maxConcurrentSessions: 10,
      keyRotationInterval: 86400000 * 30,
      complianceLevel: 'FIPS-140-2-L3',
      quantumResistant: true,
    });

    await hsmService.initialize();

    validUser = {
      id: 'edge_test_user_999',
      username: 'edge_tester',
      email: 'edge@wrfcoin.com',
    };

    validSession = {
      sessionId: crypto.randomBytes(32).toString('hex'),
      userId: validUser.id,
      createdAt: new Date(),
      expiresAt: new Date(Date.now() + 3600000),
      isActive: true,
    };

    const keyMetadata = await hsmService.generateKeyPair(
      'master-signing',
      'ECDSA-P256',
      3 // CRITICAL level
    );
    validKeyId = keyMetadata.keyId;

    hsmService.activeSessions.set(validSession.sessionId, validSession);
  });

  after(() => {
    if (clock) clock.restore();
    if (hsmService) {
      hsmService.activeSessions.clear();
      hsmService.activeKeys.clear();
    }
  });

  describe('1. MALFORMED REQUEST ATTACKS', () => {
    it('EDGE: Should handle null userId injection attempts', async () => {
      const malformedPayloads = [
        {
          proposalData: { title: 'Null user test' },
          sessionId: validSession.sessionId,
          keyId: validKeyId,
          userId: null, // Explicit null injection
        },
        {
          proposalData: { title: 'Undefined user test' },
          sessionId: validSession.sessionId,
          keyId: validKeyId,
          userId: undefined,
        },
        {
          proposalData: { title: 'Empty user test' },
          sessionId: validSession.sessionId,
          keyId: validKeyId,
          userId: '',
        },
      ];

      for (const payload of malformedPayloads) {
        const response = await request(app)
          .post('/api/governance/proposals/null-user-test/sign')
          .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
          .send(payload);

        // Should handle gracefully without crashing
        expect(response.status).to.be.oneOf([400, 401, 403, 500]);
        expect(response.body).to.have.property('error');
      }
    });

    it('EDGE: Should handle session ID manipulation attacks', async () => {
      const manipulatedSessionIds = [
        null,
        undefined,
        '',
        '0',
        '{}',
        '[]',
        'null',
        'undefined',
        { sessionId: 'object_injection' },
        [validSession.sessionId], // Array instead of string
        validSession.sessionId + '\x00', // Null byte injection
        validSession.sessionId.toUpperCase(), // Case manipulation
        validSession.sessionId.replace(/a/g, '@'), // Character substitution
        '0'.repeat(1000), // Extremely long session ID
        crypto.randomBytes(1000).toString('hex'), // Very long random ID
      ];

      for (const sessionId of manipulatedSessionIds) {
        const payload = {
          proposalData: { title: 'Session manipulation test' },
          sessionId: sessionId,
          keyId: validKeyId,
        };

        const response = await request(app)
          .post('/api/governance/proposals/session-manip/sign')
          .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
          .send(payload);

        expect(response.status).to.be.oneOf([400, 401, 403, 500]);

        // Should not crash the service
        expect(response.body).to.have.property('error');
      }
    });

    it('EDGE: Should handle JSON pollution attacks', async () => {
      const pollutionAttacks = [
        {
          proposalData: { title: 'JSON pollution' },
          sessionId: validSession.sessionId,
          keyId: validKeyId,
          __proto__: { userId: 'polluted_user' },
          constructor: { prototype: { userId: 'constructor_pollution' } },
        },
        {
          proposalData: { title: 'Prototype pollution' },
          sessionId: validSession.sessionId,
          keyId: validKeyId,
          userId: validUser.id,
          prototype: { userId: 'prototype_attack' },
        },
      ];

      for (const payload of pollutionAttacks) {
        const response = await request(app)
          .post('/api/governance/proposals/json-pollution/sign')
          .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
          .send(payload);

        // Should handle pollution attempts without security bypass
        if (response.status === 200) {
          expect(response.body.signature.userId).to.equal(validUser.id);
        } else {
          expect(response.status).to.be.oneOf([400, 401, 403]);
        }
      }
    });
  });

  describe('2. RACE CONDITION EXPLOITS', () => {
    it('EDGE: Should handle concurrent session modification attacks', async () => {
      // Create a session for race condition testing
      const raceSession = {
        sessionId: crypto.randomBytes(32).toString('hex'),
        userId: validUser.id,
        createdAt: new Date(),
        expiresAt: new Date(Date.now() + 3600000),
        isActive: true,
      };

      hsmService.activeSessions.set(raceSession.sessionId, raceSession);

      const concurrentRequests = [];

      // User 1: Legitimate requests
      for (let i = 0; i < 5; i++) {
        concurrentRequests.push(
          request(app)
            .post('/api/auth/sign')
            .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
            .send({
              authData: {
                userId: validUser.id,
                action: `race_legitimate_${i}`,
                timestamp: new Date().toISOString(),
                nonce: crypto.randomBytes(16).toString('hex'),
              },
              sessionId: raceSession.sessionId,
              keyId: validKeyId,
            })
        );
      }

      // User 2: Concurrent attack attempts
      const attackerUser = {
        id: 'race_attacker_666',
        username: 'race_attacker',
      };
      for (let i = 0; i < 5; i++) {
        concurrentRequests.push(
          request(app)
            .post('/api/auth/sign')
            .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`)
            .send({
              authData: {
                userId: attackerUser.id,
                action: `race_attack_${i}`,
                timestamp: new Date().toISOString(),
                nonce: crypto.randomBytes(16).toString('hex'),
              },
              sessionId: raceSession.sessionId, // Same session as legitimate user
              keyId: validKeyId,
            })
        );
      }

      const responses = await Promise.all(concurrentRequests);

      const legitimateResponses = responses.slice(0, 5);
      const attackResponses = responses.slice(5);

      // All legitimate requests should succeed
      legitimateResponses.forEach(response => {
        expect(response.status).to.equal(200);
      });

      // All attack requests should be blocked
      attackResponses.forEach(response => {
        expect(response.status).to.be.oneOf([401, 403]);
      });
    });

    it('EDGE: Should handle session expiration race conditions', async () => {
      // Create session that will expire during test
      const expiringSession = {
        sessionId: crypto.randomBytes(32).toString('hex'),
        userId: validUser.id,
        createdAt: new Date(),
        expiresAt: new Date(Date.now() + 1000), // Expires in 1 second
        isActive: true,
      };

      hsmService.activeSessions.set(expiringSession.sessionId, expiringSession);

      // Make request immediately
      const immediateResponse = await request(app)
        .post('/api/auth/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
        .send({
          authData: {
            userId: validUser.id,
            action: 'before_expiry',
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: expiringSession.sessionId,
          keyId: validKeyId,
        });

      expect(immediateResponse.status).to.equal(200);

      // Wait for session to expire
      await new Promise(resolve => setTimeout(resolve, 1500));

      // Make request after expiration
      const expiredResponse = await request(app)
        .post('/api/auth/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
        .send({
          authData: {
            userId: validUser.id,
            action: 'after_expiry',
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: expiringSession.sessionId,
          keyId: validKeyId,
        });

      expect(expiredResponse.status).to.be.oneOf([401, 403]);
      expect(expiredResponse.body.error).to.include('expired');
    });
  });

  describe('3. MEMORY CORRUPTION ATTEMPTS', () => {
    it('EDGE: Should handle extremely large payload attacks', async () => {
      const largeString = 'A'.repeat(10000000); // 10MB string

      const largePayloads = [
        {
          proposalData: {
            title: largeString,
            description: 'Memory exhaustion test',
          },
          sessionId: validSession.sessionId,
          keyId: validKeyId,
        },
        {
          proposalData: {
            title: 'Memory test',
            description: largeString,
          },
          sessionId: validSession.sessionId,
          keyId: validKeyId,
        },
      ];

      for (const payload of largePayloads) {
        const response = await request(app)
          .post('/api/governance/proposals/memory-attack/sign')
          .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
          .send(payload)
          .timeout(10000); // 10 second timeout

        // Should handle large payloads gracefully
        expect(response.status).to.be.oneOf([200, 400, 413, 500]);

        // If successful, should still maintain security
        if (response.status === 200) {
          expect(response.body.signature.userId).to.equal(validUser.id);
        }
      }
    });

    it('EDGE: Should handle deeply nested object attacks', async () => {
      // Create deeply nested object to test parser limits
      let deepObject = {};
      let current = deepObject;
      for (let i = 0; i < 1000; i++) {
        current.nested = {};
        current = current.nested;
      }
      current.sessionId = validSession.sessionId;
      current.keyId = validKeyId;
      current.userId = 'nested_attack_user';

      const nestedPayload = {
        proposalData: { title: 'Nested object test' },
        sessionId: validSession.sessionId,
        keyId: validKeyId,
        deepData: deepObject,
      };

      const response = await request(app)
        .post('/api/governance/proposals/nested-attack/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
        .send(nestedPayload);

      // Should handle deep nesting without crashing
      expect(response.status).to.be.oneOf([200, 400, 413, 500]);
    });
  });

  describe('4. PROTOCOL CONFUSION ATTACKS', () => {
    it('EDGE: Should handle HTTP header injection attacks', async () => {
      const headerInjectionPayloads = [
        {
          headers: {
            Authorization: `Bearer ${generateTestJWT(validUser)}`,
            'X-User-Id': 'injected_user_id',
            'X-Session-Override': validSession.sessionId,
            'X-Admin-Bypass': 'true',
          },
        },
        {
          headers: {
            Authorization: `Bearer ${generateTestJWT(validUser)}\r\nX-Injected: malicious`,
            'User-Agent': 'Normal\r\nX-Malicious: header',
          },
        },
      ];

      for (const config of headerInjectionPayloads) {
        const response = await request(app)
          .post('/api/governance/proposals/header-injection/sign')
          .set(config.headers)
          .send({
            proposalData: { title: 'Header injection test' },
            sessionId: validSession.sessionId,
            keyId: validKeyId,
          });

        // Should not be fooled by header injection
        if (response.status === 200) {
          expect(response.body.signature.userId).to.equal(validUser.id);
        } else {
          expect(response.status).to.be.oneOf([400, 401, 403]);
        }
      }
    });

    it('EDGE: Should handle content-type confusion attacks', async () => {
      const payload = {
        proposalData: { title: 'Content type test' },
        sessionId: validSession.sessionId,
        keyId: validKeyId,
      };

      const contentTypeAttacks = [
        'application/x-www-form-urlencoded',
        'text/plain',
        'application/xml',
        'multipart/form-data',
        'application/json; charset=utf-7', // Charset confusion
        'application/json\r\nContent-Length: 0\r\n\r\nGET / HTTP/1.1', // HTTP smuggling attempt
      ];

      for (const contentType of contentTypeAttacks) {
        const response = await request(app)
          .post('/api/governance/proposals/content-type-attack/sign')
          .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
          .set('Content-Type', contentType)
          .send(payload);

        // Should handle all content types securely
        if (response.status === 200) {
          expect(response.body.signature.userId).to.equal(validUser.id);
        }
      }
    });
  });

  describe('5. CRYPTOGRAPHIC EDGE CASES', () => {
    it('EDGE: Should handle key ID manipulation attacks', async () => {
      const maliciousKeyIds = [
        '../../../etc/passwd', // Path traversal
        '$(rm -rf /)', // Command injection
        validKeyId + '\x00malicious', // Null byte injection
        `${validKeyId}'; DROP TABLE users; --`, // SQL injection style
        Buffer.from(validKeyId).toString('base64'), // Encoding confusion
        validKeyId.split('').reverse().join(''), // Reversed key ID
        crypto.randomBytes(32).toString('hex'), // Random key that doesn't exist
      ];

      for (const keyId of maliciousKeyIds) {
        const payload = {
          proposalData: { title: 'Key ID manipulation test' },
          sessionId: validSession.sessionId,
          keyId: keyId,
        };

        const response = await request(app)
          .post('/api/governance/proposals/key-manipulation/sign')
          .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
          .send(payload);

        // Should handle invalid key IDs securely
        if (keyId === validKeyId) {
          expect(response.status).to.equal(200);
        } else {
          expect(response.status).to.be.oneOf([400, 401, 403, 500]);
        }
      }
    });

    it('EDGE: Should handle timestamp manipulation attacks', async () => {
      const maliciousTimestamps = [
        new Date('1970-01-01').toISOString(), // Unix epoch
        new Date('2100-12-31').toISOString(), // Far future
        'invalid-timestamp',
        '1234567890',
        null,
        undefined,
        { timestamp: 'object_injection' },
      ];

      for (const timestamp of maliciousTimestamps) {
        const payload = {
          authData: {
            userId: validUser.id,
            action: 'timestamp_manipulation',
            timestamp: timestamp,
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: validSession.sessionId,
          keyId: validKeyId,
        };

        const response = await request(app)
          .post('/api/auth/sign')
          .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
          .send(payload);

        // Should handle malformed timestamps without security bypass
        if (response.status === 200) {
          expect(response.body.signedAuth.userId).to.equal(validUser.id);
        } else {
          expect(response.status).to.be.oneOf([400, 401, 403]);
        }
      }
    });
  });

  describe('6. SESSION STATE MANIPULATION', () => {
    it('EDGE: Should handle session state tampering attempts', async () => {
      // Attempt to modify session after creation
      const tamperSession = {
        sessionId: crypto.randomBytes(32).toString('hex'),
        userId: validUser.id,
        createdAt: new Date(),
        expiresAt: new Date(Date.now() + 3600000),
        isActive: true,
      };

      hsmService.activeSessions.set(tamperSession.sessionId, tamperSession);

      // Make legitimate request first
      const legitimateResponse = await request(app)
        .post('/api/auth/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
        .send({
          authData: {
            userId: validUser.id,
            action: 'before_tampering',
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: tamperSession.sessionId,
          keyId: validKeyId,
        });

      expect(legitimateResponse.status).to.equal(200);

      // Attempt to tamper with session (simulate attacker gaining access to session store)
      tamperSession.userId = 'tampered_user_id';
      tamperSession.isActive = false;

      // Make request after tampering
      const tamperedResponse = await request(app)
        .post('/api/auth/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
        .send({
          authData: {
            userId: validUser.id,
            action: 'after_tampering',
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: tamperSession.sessionId,
          keyId: validKeyId,
        });

      // Should detect tampering and block request
      expect(tamperedResponse.status).to.be.oneOf([401, 403]);
    });

    it('EDGE: Should handle session ID collision attacks', async () => {
      // Create two users with the same session ID (simulate collision)
      const collisionSessionId = crypto.randomBytes(32).toString('hex');

      const user1 = { id: 'collision_user_1', username: 'user1' };
      const user2 = { id: 'collision_user_2', username: 'user2' };

      const session1 = {
        sessionId: collisionSessionId,
        userId: user1.id,
        createdAt: new Date(),
        expiresAt: new Date(Date.now() + 3600000),
        isActive: true,
      };

      // Register first session
      hsmService.activeSessions.set(collisionSessionId, session1);

      // User 1 makes legitimate request
      const user1Response = await request(app)
        .post('/api/auth/sign')
        .set('Authorization', `Bearer ${generateTestJWT(user1)}`)
        .send({
          authData: {
            userId: user1.id,
            action: 'collision_test_user1',
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: collisionSessionId,
          keyId: validKeyId,
        });

      expect(user1Response.status).to.equal(200);

      // User 2 tries to use same session ID
      const user2Response = await request(app)
        .post('/api/auth/sign')
        .set('Authorization', `Bearer ${generateTestJWT(user2)}`)
        .send({
          authData: {
            userId: user2.id,
            action: 'collision_test_user2',
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: collisionSessionId,
          keyId: validKeyId,
        });

      // User 2 should be blocked due to session ownership mismatch
      expect(user2Response.status).to.be.oneOf([401, 403]);
      expect(user2Response.body.error).to.include(
        'Session belongs to different user'
      );
    });
  });

  describe('7. ERROR HANDLING ROBUSTNESS', () => {
    it('EDGE: Should handle HSM service failures gracefully', async () => {
      // Temporarily disable HSM service
      const originalSignMethod = hsmService.signWithHSM;
      hsmService.signWithHSM = function () {
        throw new Error('HSM hardware failure');
      };

      const payload = {
        proposalData: { title: 'HSM failure test' },
        sessionId: validSession.sessionId,
        keyId: validKeyId,
      };

      const response = await request(app)
        .post('/api/governance/proposals/hsm-failure/sign')
        .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
        .send(payload);

      // Should handle HSM failures gracefully
      expect(response.status).to.be.oneOf([500, 503]);
      expect(response.body).to.have.property('error');

      // Restore HSM service
      hsmService.signWithHSM = originalSignMethod;
    });

    it('EDGE: Should maintain security during high error rates', async () => {
      // Generate many requests that will cause errors
      const errorRequests = [];

      for (let i = 0; i < 50; i++) {
        errorRequests.push(
          request(app)
            .post('/api/governance/proposals/error-flood/sign')
            .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
            .send({
              proposalData: { title: 'Error test' },
              sessionId: 'invalid-session-' + i,
              keyId: validKeyId,
            })
        );
      }

      // Add a few legitimate requests
      for (let i = 0; i < 5; i++) {
        errorRequests.push(
          request(app)
            .post('/api/governance/proposals/legitimate-during-errors/sign')
            .set('Authorization', `Bearer ${generateTestJWT(validUser)}`)
            .send({
              proposalData: { title: 'Legitimate test' },
              sessionId: validSession.sessionId,
              keyId: validKeyId,
            })
        );
      }

      const responses = await Promise.all(errorRequests);

      const errorResponses = responses.slice(0, 50);
      const legitimateResponses = responses.slice(50);

      // Error requests should be handled appropriately
      errorResponses.forEach(response => {
        expect(response.status).to.be.oneOf([400, 401, 403, 500]);
      });

      // Legitimate requests should still work despite high error rate
      legitimateResponses.forEach(response => {
        expect(response.status).to.equal(200);
        expect(response.body.signature.userId).to.equal(validUser.id);
      });
    });
  });
});

/**
 * Helper function to generate test JWT tokens
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
