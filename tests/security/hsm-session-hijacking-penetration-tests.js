/**
 * HSM Session Hijacking Penetration Tests
 *
 * Advanced penetration testing scenarios specifically targeting the HSM session
 * hijacking vulnerability that was fixed. These tests simulate sophisticated
 * attack vectors and verify they are completely blocked.
 *
 * ATTACK VECTORS TESTED:
 * 1. Session Token Theft & Replay
 * 2. Man-in-the-Middle Session Interception
 * 3. Timing Attack Session Enumeration
 * 4. Privilege Escalation via Session Confusion
 * 5. Cross-Account Session Reuse
 */

const request = require('supertest');
const { expect } = require('chai');
const crypto = require('crypto');
const sinon = require('sinon');
const app = require('../../backend/server');
const EnterpriseHSMService =
  require('../../security/enterprise-hsm-service').default;

describe('HSM Session Hijacking Penetration Tests', () => {
  let hsmService;
  let victimUser, attackerUser, adminUser;
  let victimSession, attackerSession, adminSession;
  let testKeyId;
  let clock;

  before(async () => {
    // Initialize HSM service
    hsmService = new EnterpriseHSMService({
      provider: 'aws-cloudhsm',
      endpoint: 'localhost:9000',
      keyStoreId: 'pentest-keys',
      sessionTimeout: 3600000,
      maxConcurrentSessions: 10,
      keyRotationInterval: 86400000 * 30,
      complianceLevel: 'FIPS-140-2-L3',
      quantumResistant: true,
    });

    await hsmService.initialize();

    // Create test users with different privilege levels
    victimUser = {
      id: 'victim_user_789',
      username: 'victim_alice',
      email: 'alice@wrfcoin.com',
      role: 'standard_user',
      clearanceLevel: 'basic',
    };

    attackerUser = {
      id: 'attacker_user_666',
      username: 'malicious_bob',
      email: 'bob@evil.com',
      role: 'standard_user',
      clearanceLevel: 'basic',
    };

    adminUser = {
      id: 'admin_user_001',
      username: 'admin_charlie',
      email: 'charlie@wrfcoin.com',
      role: 'admin',
      clearanceLevel: 'privileged',
    };

    // Generate test key
    const keyMetadata = await hsmService.generateKeyPair(
      'master-signing',
      'ECDSA-P256',
      3 // CRITICAL security level
    );
    testKeyId = keyMetadata.keyId;

    // Create sessions
    victimSession = {
      sessionId: crypto.randomBytes(32).toString('hex'),
      userId: victimUser.id,
      createdAt: new Date(),
      expiresAt: new Date(Date.now() + 3600000),
      isActive: true,
      securityLevel: 'standard',
      clearanceLevel: 'basic',
    };

    attackerSession = {
      sessionId: crypto.randomBytes(32).toString('hex'),
      userId: attackerUser.id,
      createdAt: new Date(),
      expiresAt: new Date(Date.now() + 3600000),
      isActive: true,
      securityLevel: 'standard',
      clearanceLevel: 'basic',
    };

    adminSession = {
      sessionId: crypto.randomBytes(32).toString('hex'),
      userId: adminUser.id,
      createdAt: new Date(),
      expiresAt: new Date(Date.now() + 3600000),
      isActive: true,
      securityLevel: 'privileged',
      clearanceLevel: 'privileged',
    };

    // Register sessions
    hsmService.activeSessions.set(victimSession.sessionId, victimSession);
    hsmService.activeSessions.set(attackerSession.sessionId, attackerSession);
    hsmService.activeSessions.set(adminSession.sessionId, adminSession);
  });

  after(() => {
    if (clock) clock.restore();
    if (hsmService) {
      hsmService.activeSessions.clear();
      hsmService.activeKeys.clear();
    }
  });

  describe('ATTACK VECTOR 1: Session Token Theft & Replay', () => {
    it('ATTACK: Should block session replay attack with stolen token', async () => {
      // Simulate attacker intercepting victim's session token
      const stolenSessionToken = victimSession.sessionId;

      // Attacker attempts to use stolen session for high-value governance operation
      const maliciousGovernancePayload = {
        proposalData: {
          title: 'MALICIOUS: Transfer All Treasury Funds',
          description:
            'Emergency proposal to transfer all treasury funds to attacker wallet',
          proposalType: 'PARAMETER_CHANGE',
          parametersToChange: {
            treasuryRecipient: '0xMaliciousAttackerWallet123456789',
            transferAmount: '100000000', // 100M tokens
          },
        },
        sessionId: stolenSessionToken,
        keyId: testKeyId,
      };

      const response = await request(app)
        .post('/api/governance/proposals/treasury-hijack-001/sign')
        .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`)
        .set('X-Forwarded-For', '192.168.1.100') // Simulate different IP
        .set('User-Agent', 'AttackerBot/1.0')
        .send(maliciousGovernancePayload)
        .expect(403);

      expect(response.body.error).to.include(
        'Session belongs to different user'
      );
      expect(response.body.code).to.equal('HSM_SESSION_AUTHORIZATION_FAILED');

      // Verify critical security alert was logged
      const criticalAlerts = hsmService.auditTrail.filter(
        event =>
          event.action === 'UNAUTHORIZED_HSM_SESSION_ACCESS' &&
          event.details.includes('treasury') &&
          event.result === 'failure'
      );
      expect(criticalAlerts.length).to.be.at.least(1);
    });

    it('ATTACK: Should block session token reuse across different endpoints', async () => {
      const stolenToken = victimSession.sessionId;

      // Attacker tries multiple endpoints with stolen session
      const endpoints = [
        {
          url: '/api/bridges/transactions/steal-bridge-001/sign',
          payload: {
            transferData: {
              fromChain: 'ethereum',
              toChain: 'attacker-chain',
              amount: '999999999',
              fromAddress: '0xVictimWallet',
              toAddress: '0xAttackerWallet',
            },
            sessionId: stolenToken,
            keyId: testKeyId,
          },
        },
        {
          url: '/api/weather/data/fake-weather-001/sign',
          payload: {
            weatherData: {
              temperature: 999, // Fake extreme data to manipulate weather derivatives
              humidity: 0,
              pressure: 0,
              windSpeed: 1000,
            },
            sessionId: stolenToken,
            keyId: testKeyId,
            validationLevel: 'premium',
          },
        },
        {
          url: '/api/auth/sign',
          payload: {
            authData: {
              userId: victimUser.id, // Impersonating victim
              action: 'admin_privilege_escalation',
              timestamp: new Date().toISOString(),
              nonce: 'evil-nonce-123',
            },
            sessionId: stolenToken,
            keyId: testKeyId,
          },
        },
      ];

      for (const endpoint of endpoints) {
        const response = await request(app)
          .post(endpoint.url)
          .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`)
          .send(endpoint.payload);

        expect(response.status).to.be.oneOf([401, 403]);
        expect(response.body).to.have.property('error');
      }

      // Verify multiple security violations were logged
      const violations = hsmService.auditTrail.filter(
        event => event.actor === attackerUser.id && event.result === 'failure'
      );
      expect(violations.length).to.be.at.least(3);
    });
  });

  describe('ATTACK VECTOR 2: Man-in-the-Middle Session Interception', () => {
    it('ATTACK: Should block MITM session injection attack', async () => {
      // Simulate MITM attacker injecting their own session ID into victim's request
      const victimPayload = {
        proposalData: {
          title: 'Legitimate Proposal from Victim',
          description: 'This should be a normal proposal',
        },
        sessionId: attackerSession.sessionId, // MITM injection: attacker's session in victim's request
        keyId: testKeyId,
      };

      // Request appears to come from victim but has attacker's session
      const response = await request(app)
        .post('/api/governance/proposals/mitm-injection-001/sign')
        .set('Authorization', `Bearer ${generateTestJWT(victimUser)}`) // Victim's auth
        .set('X-Real-IP', '10.0.0.50') // Victim's IP
        .send(victimPayload)
        .expect(403);

      expect(response.body.error).to.include(
        'Session belongs to different user'
      );

      // Should log as a serious security incident
      const securityIncidents = hsmService.auditTrail.filter(
        event =>
          event.action === 'UNAUTHORIZED_HSM_SESSION_ACCESS' &&
          event.actor === victimUser.id && // Authenticated as victim
          event.complianceContext.includes(attackerUser.id) // But trying to use attacker's session
      );
      expect(securityIncidents.length).to.be.at.least(1);
    });

    it('ATTACK: Should detect and block session swap attacks', async () => {
      // Attacker rapidly switches between multiple stolen sessions
      const stolenSessions = [victimSession.sessionId, adminSession.sessionId];
      const responses = [];

      for (let i = 0; i < stolenSessions.length; i++) {
        const payload = {
          authData: {
            userId: attackerUser.id,
            action: `session_swap_attack_${i}`,
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: stolenSessions[i],
          keyId: testKeyId,
        };

        const response = await request(app)
          .post('/api/auth/sign')
          .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`)
          .send(payload);

        responses.push(response);
      }

      // All requests should be blocked
      responses.forEach(response => {
        expect(response.status).to.be.oneOf([401, 403]);
      });
    });
  });

  describe('ATTACK VECTOR 3: Timing Attack Session Enumeration', () => {
    it('ATTACK: Should not reveal session existence through timing differences', async () => {
      const timingResults = [];

      // Test with valid session
      const validPayload = {
        proposalData: { title: 'Timing test valid' },
        sessionId: victimSession.sessionId,
        keyId: testKeyId,
      };

      const validStart = process.hrtime.bigint();
      await request(app)
        .post('/api/governance/proposals/timing-valid/sign')
        .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`) // Wrong user
        .send(validPayload);
      const validEnd = process.hrtime.bigint();
      const validTiming = Number(validEnd - validStart) / 1000000;

      // Test with invalid session
      const invalidPayload = {
        proposalData: { title: 'Timing test invalid' },
        sessionId:
          'non-existent-session-' + crypto.randomBytes(16).toString('hex'),
        keyId: testKeyId,
      };

      const invalidStart = process.hrtime.bigint();
      await request(app)
        .post('/api/governance/proposals/timing-invalid/sign')
        .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`)
        .send(invalidPayload);
      const invalidEnd = process.hrtime.bigint();
      const invalidTiming = Number(invalidEnd - invalidStart) / 1000000;

      // Timing difference should be minimal to prevent enumeration
      const timingDifference = Math.abs(validTiming - invalidTiming);
      expect(timingDifference).to.be.below(10); // Less than 10ms difference
    });

    it('ATTACK: Should rate limit session enumeration attempts', async () => {
      // Simulate rapid session ID guessing attack
      const sessionGuesses = [];
      for (let i = 0; i < 20; i++) {
        sessionGuesses.push(crypto.randomBytes(32).toString('hex'));
      }

      const requests = sessionGuesses.map(sessionId => {
        return request(app)
          .post('/api/auth/sign')
          .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`)
          .send({
            authData: {
              userId: attackerUser.id,
              action: 'enumeration_attack',
              timestamp: new Date().toISOString(),
              nonce: sessionId,
            },
            sessionId: sessionId,
            keyId: testKeyId,
          });
      });

      const responses = await Promise.all(requests);

      // Should see rate limiting kick in for rapid requests
      const rateLimitedResponses = responses.filter(res => res.status === 429);
      expect(rateLimitedResponses.length).to.be.at.least(1);
    });
  });

  describe('ATTACK VECTOR 4: Privilege Escalation via Session Confusion', () => {
    it('ATTACK: Should block privilege escalation using admin session', async () => {
      // Attacker tries to use admin session for privileged operations
      const privilegeEscalationPayload = {
        proposalData: {
          title: 'EMERGENCY: System Shutdown Authorization',
          description:
            'Critical system emergency requiring immediate admin privileges',
          proposalType: 'UPGRADE',
          parametersToChange: {
            systemStatus: 'EMERGENCY_SHUTDOWN',
            newAdminWallet: '0xAttackerControlledWallet',
            privilegeLevel: 'SUPER_ADMIN',
          },
        },
        sessionId: adminSession.sessionId, // Stolen admin session
        keyId: testKeyId,
      };

      const response = await request(app)
        .post('/api/governance/proposals/privilege-escalation-001/sign')
        .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`) // Still attacker's auth
        .set('X-Admin-Override', 'true') // Fake admin header
        .send(privilegeEscalationPayload)
        .expect(403);

      expect(response.body.error).to.include(
        'Session belongs to different user'
      );

      // Should trigger high-severity security alert
      const privilegeEscalationAlerts = hsmService.auditTrail.filter(
        event =>
          event.action === 'UNAUTHORIZED_HSM_SESSION_ACCESS' &&
          event.details.includes('EMERGENCY') &&
          event.complianceContext.includes(adminUser.id)
      );
      expect(privilegeEscalationAlerts.length).to.be.at.least(1);
    });

    it('ATTACK: Should block cross-clearance level attacks', async () => {
      // Low clearance user tries to use high clearance session
      const crossClearancePayload = {
        transferData: {
          fromChain: 'ethereum',
          toChain: 'classified-military-network',
          amount: '1000000',
          securityClassification: 'TOP_SECRET',
          clearanceRequired: 'L5_COSMIC',
        },
        sessionId: adminSession.sessionId, // High clearance session
        keyId: testKeyId,
      };

      const response = await request(app)
        .post('/api/bridges/transactions/clearance-escalation/sign')
        .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`) // Low clearance auth
        .send(crossClearancePayload)
        .expect(403);

      expect(response.body.error).to.include(
        'Session belongs to different user'
      );
    });
  });

  describe('ATTACK VECTOR 5: Cross-Account Session Reuse', () => {
    it('ATTACK: Should block session sharing between colluding users', async () => {
      // Simulate two colluding users trying to share sessions
      const colludingUser1 = {
        id: 'colluder_001',
        username: 'insider_threat_1',
      };

      const colludingUser2 = {
        id: 'colluder_002',
        username: 'insider_threat_2',
      };

      const sharedSession = {
        sessionId: crypto.randomBytes(32).toString('hex'),
        userId: colludingUser1.id,
        createdAt: new Date(),
        expiresAt: new Date(Date.now() + 3600000),
        isActive: true,
      };

      hsmService.activeSessions.set(sharedSession.sessionId, sharedSession);

      // User 2 tries to use User 1's session (insider threat scenario)
      const collusionPayload = {
        weatherData: {
          temperature: 50, // Fake extreme weather to trigger insurance payouts
          humidity: 100,
          pressure: 800,
          windSpeed: 200, // Hurricane-level to trigger catastrophe bonds
          stationId: 'COMPROMISED_STATION_001',
        },
        sessionId: sharedSession.sessionId,
        keyId: testKeyId,
        validationLevel: 'premium',
      };

      const response = await request(app)
        .post('/api/weather/data/collusion-attack/sign')
        .set('Authorization', `Bearer ${generateTestJWT(colludingUser2)}`)
        .send(collusionPayload)
        .expect(403);

      expect(response.body.error).to.include(
        'Session belongs to different user'
      );

      // Should detect potential insider threat
      const insiderThreats = hsmService.auditTrail.filter(
        event =>
          event.action === 'UNAUTHORIZED_HSM_SESSION_ACCESS' &&
          event.actor === colludingUser2.id &&
          event.complianceContext.includes(colludingUser1.id)
      );
      expect(insiderThreats.length).to.be.at.least(1);
    });

    it('ATTACK: Should block automated bot session hijacking', async () => {
      // Simulate automated bot attempting systematic session hijacking
      const botUser = {
        id: 'automated_bot_666',
        username: 'evil_bot',
      };

      const botAttempts = [];

      // Bot tries to hijack multiple sessions rapidly
      const allSessions = [victimSession.sessionId, adminSession.sessionId];

      for (const sessionId of allSessions) {
        for (let i = 0; i < 5; i++) {
          const botPayload = {
            authData: {
              userId: botUser.id,
              action: `bot_attack_${i}`,
              timestamp: new Date().toISOString(),
              nonce: `bot-${sessionId}-${i}`,
            },
            sessionId: sessionId,
            keyId: testKeyId,
          };

          botAttempts.push(
            request(app)
              .post('/api/auth/sign')
              .set('Authorization', `Bearer ${generateTestJWT(botUser)}`)
              .set('User-Agent', 'EvilBot/2.0 (automated-attack)')
              .send(botPayload)
          );
        }
      }

      const responses = await Promise.all(botAttempts);

      // All bot attempts should be blocked
      responses.forEach(response => {
        expect(response.status).to.be.oneOf([401, 403, 429]);
      });

      // Should trigger automated threat detection
      const botThreats = hsmService.auditTrail.filter(
        event => event.actor === botUser.id && event.result === 'failure'
      );
      expect(botThreats.length).to.be.at.least(10);
    });
  });

  describe('DEFENSIVE CAPABILITIES VALIDATION', () => {
    it('DEFENSE: Should maintain session isolation under attack', async () => {
      // Verify legitimate operations continue working during attack
      const legitimatePayload = {
        proposalData: {
          title: 'Legitimate Business Proposal',
          description: 'Normal governance proposal during security testing',
        },
        sessionId: victimSession.sessionId,
        keyId: testKeyId,
      };

      // Legitimate user operation should work normally
      const legitimateResponse = await request(app)
        .post('/api/governance/proposals/legitimate-during-attack/sign')
        .set('Authorization', `Bearer ${generateTestJWT(victimUser)}`)
        .send(legitimatePayload)
        .expect(200);

      expect(legitimateResponse.body).to.have.property('success', true);
      expect(legitimateResponse.body.signature).to.have.property(
        'userId',
        victimUser.id
      );
    });

    it('DEFENSE: Should maintain audit integrity under attack', async () => {
      const initialAuditCount = hsmService.auditTrail.length;

      // Simulate multiple attack attempts
      await request(app)
        .post('/api/governance/proposals/audit-integrity-test/sign')
        .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`)
        .send({
          proposalData: { title: 'Attack attempt 1' },
          sessionId: victimSession.sessionId,
          keyId: testKeyId,
        });

      await request(app)
        .post('/api/bridges/transactions/audit-integrity-test/sign')
        .set('Authorization', `Bearer ${generateTestJWT(attackerUser)}`)
        .send({
          transferData: { fromChain: 'eth', toChain: 'polygon', amount: '1' },
          sessionId: adminSession.sessionId,
          keyId: testKeyId,
        });

      const finalAuditCount = hsmService.auditTrail.length;
      expect(finalAuditCount).to.be.above(initialAuditCount);

      // Audit logs should be immutable and complete
      const attackerViolations = hsmService.auditTrail.filter(
        event => event.actor === attackerUser.id && event.result === 'failure'
      );
      expect(attackerViolations.length).to.be.at.least(2);
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
      role: user.role || 'user',
      clearanceLevel: user.clearanceLevel || 'basic',
    },
    process.env.JWT_SECRET || 'test-secret-key',
    { expiresIn: '1h' }
  );
}
