/**
 * Security Integration Tests
 * End-to-end tests for security infrastructure integration
 */

import { describe, it, expect, beforeAll, afterAll, jest } from '@jest/globals';
import request from 'supertest';
import { HSMSecureImplementation } from '../../security/hsm-secure-implementation';
import { VaultServer } from '../../backend/server/vault-server';
import { AuthenticationManager } from '../../backend/server/middleware/auth';
import { SecurityAlertMiddleware } from '../../backend/server/middleware/securityAlertMiddleware';

describe('Security Infrastructure Integration', () => {
  let hsmImplementation: HSMSecureImplementation;
  let vaultServer: VaultServer;
  let authManager: AuthenticationManager;
  let securityMiddleware: SecurityAlertMiddleware;

  beforeAll(async () => {
    // Initialize security components
    hsmImplementation = new HSMSecureImplementation({
      provider: 'SoftHSM',
      credentials: { keyId: 'test-integration', pin: 'test-pin' },
      settings: {
        sessionTimeout: 30000,
        maxRetries: 3,
        auditLogging: true,
        keyRotationInterval: 86400000,
      },
    });

    vaultServer = new VaultServer({
      url: 'http://localhost:8200',
      token: 'integration-test-token',
      timeout: 5000,
      retryAttempts: 3,
    });

    authManager = new AuthenticationManager({
      jwtSecret: 'integration-test-secret',
      jwtExpiresIn: '1h',
      apiKeyHeader: 'X-API-Key',
      bypassAuthInDev: false,
      sessionTimeout: 3600000,
      maxConcurrentSessions: 5,
    });

    securityMiddleware = new SecurityAlertMiddleware({
      rateLimitWindow: 60000,
      rateLimitMaxRequests: 100,
      bruteForceWindow: 300000,
      bruteForceMaxAttempts: 5,
      suspiciousPatterns: [/\.\.\//g, /<script/gi, /union.*select/gi],
    });

    await vaultServer.initialize();
  });

  afterAll(async () => {
    await vaultServer.shutdown();
  });

  describe('HSM + Vault Integration', () => {
    it('should store HSM-generated keys in Vault', async () => {
      // Generate key with HSM
      const keyPair = await hsmImplementation.generateKeyPair('ECDSA-P256');

      // Store private key metadata in Vault
      const vaultApp = vaultServer.getApp();
      const secretData = {
        keyHandle: keyPair.keyHandle,
        algorithm: keyPair.algorithm,
        keyType: keyPair.keyType,
        createdAt: keyPair.createdAt.toISOString(),
      };

      const response = await request(vaultApp)
        .post('/api/vault/secrets')
        .send({
          path: `secret/hsm-keys/${keyPair.keyHandle}`,
          data: secretData,
        })
        .expect(201);

      expect(response.body.success).toBe(true);

      // Retrieve and verify
      const retrieveResponse = await request(vaultApp)
        .get(
          `/api/vault/secrets?path=${encodeURIComponent(`secret/hsm-keys/${keyPair.keyHandle}`)}`
        )
        .expect(200);

      expect(retrieveResponse.body.data.keyHandle).toBe(keyPair.keyHandle);
      expect(retrieveResponse.body.data.algorithm).toBe('ECDSA-P256');
    });

    it('should rotate HSM keys and update Vault records', async () => {
      const oldKeyPair = await hsmImplementation.generateKeyPair('RSA-2048');
      const newKeyPair = await hsmImplementation.rotateKey(
        oldKeyPair.keyHandle,
        'RSA-2048'
      );

      // Update Vault with rotation information
      const vaultApp = vaultServer.getApp();
      await request(vaultApp)
        .post('/api/vault/secrets')
        .send({
          path: `secret/key-rotation/${oldKeyPair.keyHandle}`,
          data: {
            oldKeyHandle: oldKeyPair.keyHandle,
            newKeyHandle: newKeyPair.keyHandle,
            rotatedAt: new Date().toISOString(),
            reason: 'scheduled_rotation',
          },
        });

      // Verify rotation audit logs
      const hsmLogs = await hsmImplementation.getAuditLogs();
      const rotationLog = hsmLogs.find(log => log.event === 'KEY_ROTATION');
      expect(rotationLog).toBeDefined();
      expect(rotationLog?.keyHandle).toBe(oldKeyPair.keyHandle);
    });
  });

  describe('Authentication + Security Monitoring', () => {
    it('should detect and log brute force attacks through auth system', async () => {
      const clientIp = '192.168.1.100';
      const mockReq = {
        ip: clientIp,
        path: '/api/auth/login',
        method: 'POST',
        headers: { 'user-agent': 'Test Browser' },
        body: {},
        query: {},
      } as any;

      const mockRes = {
        status: jest.fn().mockReturnThis(),
        json: jest.fn().mockReturnThis(),
      } as any;

      const mockNext = jest.fn();

      // Simulate multiple failed login attempts
      for (let i = 0; i < 6; i++) {
        securityMiddleware.recordFailedLogin(clientIp);
        await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);
      }

      // Should generate security alert
      const alerts = securityMiddleware.getActiveAlerts();
      const bruteForceAlert = alerts.find(
        alert => alert.type === 'brute_force_attack'
      );

      expect(bruteForceAlert).toBeDefined();
      expect(bruteForceAlert?.severity).toBe('high');
      expect(bruteForceAlert?.clientIp).toBe(clientIp);
    });

    it('should correlate JWT sessions with security monitoring', async () => {
      const testUser = {
        userId: 'security-test-user',
        roles: ['user'],
        permissions: ['read'],
        sessionId: 'secure-session-123',
      };

      // Generate JWT
      const token = authManager.generateJWT(testUser);
      expect(token).toBeDefined();

      // Verify session tracking
      const sessions = authManager.getUserSessions(testUser.userId);
      expect(sessions).toHaveLength(1);
      expect(sessions[0].sessionId).toBe(testUser.sessionId);

      // Simulate suspicious activity for this session
      const mockReq = {
        ip: '192.168.1.101',
        path: '/api/admin/../../../etc/passwd',
        method: 'GET',
        headers: {
          'user-agent': 'Test Browser',
          authorization: `Bearer ${token}`,
        },
        user: testUser,
      } as any;

      const mockRes = {
        status: jest.fn().mockReturnThis(),
        json: jest.fn().mockReturnThis(),
      } as any;

      await securityMiddleware.checkRequest(mockReq, mockRes, jest.fn());

      // Should block request and log with session context
      expect(mockRes.status).toHaveBeenCalledWith(403);

      const auditLogs = securityMiddleware.getAuditLogs();
      const blockLog = auditLogs.find(
        log =>
          log.event === 'REQUEST_BLOCKED' &&
          log.sessionId === testUser.sessionId
      );
      expect(blockLog).toBeDefined();
    });
  });

  describe('Vault + Authentication Integration', () => {
    it('should store and retrieve JWT secrets from Vault', async () => {
      const vaultApp = vaultServer.getApp();

      // Store JWT configuration in Vault
      const jwtConfig = {
        secret: 'super-secure-jwt-secret',
        algorithm: 'HS256',
        expiresIn: '1h',
        issuer: 'wrfcoin-platform',
      };

      await request(vaultApp)
        .post('/api/vault/secrets')
        .send({
          path: 'secret/jwt-config',
          data: jwtConfig,
        })
        .expect(201);

      // Retrieve JWT config
      const response = await request(vaultApp)
        .get('/api/vault/secrets?path=secret/jwt-config')
        .expect(200);

      expect(response.body.data.secret).toBe(jwtConfig.secret);
      expect(response.body.data.algorithm).toBe(jwtConfig.algorithm);
    });

    it('should implement dynamic API key rotation', async () => {
      const vaultApp = vaultServer.getApp();
      const apiKeyPath = 'secret/api-keys/user123';

      // Store initial API key
      const initialKey = 'api-key-initial-123';
      await request(vaultApp)
        .post('/api/vault/secrets')
        .send({
          path: apiKeyPath,
          data: {
            key: initialKey,
            permissions: ['read', 'write'],
            created: new Date().toISOString(),
            expires: new Date(Date.now() + 86400000).toISOString(),
          },
        });

      // Rotate API key
      const rotatedKey = 'api-key-rotated-456';
      const rotateResponse = await request(vaultApp)
        .post('/api/vault/rotate')
        .send({
          path: apiKeyPath,
          newData: {
            key: rotatedKey,
            permissions: ['read', 'write'],
            created: new Date().toISOString(),
            expires: new Date(Date.now() + 86400000).toISOString(),
          },
        })
        .expect(200);

      expect(rotateResponse.body.success).toBe(true);
      expect(rotateResponse.body.newVersion).toBe(2);

      // Verify old key is invalidated
      const validation = await authManager.validateApiKey(initialKey);
      expect(validation.isValid).toBe(false);
    });
  });

  describe('End-to-End Security Flow', () => {
    it('should handle complete authentication and authorization flow', async () => {
      const clientIp = '192.168.1.200';

      // 1. Generate HSM key for signing
      const signingKey = await hsmImplementation.generateKeyPair('ECDSA-P256');

      // 2. Store key metadata in Vault
      const vaultApp = vaultServer.getApp();
      await request(vaultApp)
        .post('/api/vault/secrets')
        .send({
          path: `secret/signing-keys/${signingKey.keyHandle}`,
          data: {
            keyHandle: signingKey.keyHandle,
            purpose: 'jwt-signing',
            algorithm: signingKey.algorithm,
          },
        });

      // 3. Create authenticated user session
      const testUser = {
        userId: 'end-to-end-user',
        roles: ['user', 'verified'],
        permissions: ['read', 'write'],
        sessionId: 'e2e-session-789',
      };

      const jwtToken = authManager.generateJWT(testUser);

      // 4. Sign JWT with HSM key
      const tokenSignature = await hsmImplementation.signData(
        signingKey.keyHandle,
        jwtToken
      );

      // 5. Simulate request with security monitoring
      const mockReq = {
        ip: clientIp,
        path: '/api/protected/data',
        method: 'GET',
        headers: {
          'user-agent': 'Legitimate Client',
          authorization: `Bearer ${jwtToken}`,
          'x-signature': tokenSignature,
        },
        user: testUser,
      } as any;

      const mockRes = {
        status: jest.fn().mockReturnThis(),
        json: jest.fn().mockReturnThis(),
      } as any;

      const mockNext = jest.fn();

      // 6. Process through security middleware
      await securityMiddleware.checkRequest(mockReq, mockRes, mockNext);

      // 7. Authenticate with JWT middleware
      authManager.authenticateJWT(mockReq, mockRes, mockNext);

      // 8. Verify everything passed
      expect(mockNext).toHaveBeenCalled();
      expect(mockReq.user).toEqual(testUser);

      // 9. Verify audit trail
      const hsmLogs = await hsmImplementation.getAuditLogs();
      const signLog = hsmLogs.find(
        log =>
          log.event === 'DIGITAL_SIGNATURE' &&
          log.keyHandle === signingKey.keyHandle
      );
      expect(signLog).toBeDefined();

      const securityLogs = securityMiddleware.getAuditLogs();
      const processLog = securityLogs.find(
        log =>
          log.event === 'REQUEST_PROCESSED' &&
          log.sessionId === testUser.sessionId
      );
      expect(processLog).toBeDefined();
    });

    it('should detect and respond to coordinated security threats', async () => {
      const attackerIps = ['192.168.1.250', '192.168.1.251', '192.168.1.252'];

      // Simulate coordinated brute force attack
      for (const ip of attackerIps) {
        for (let i = 0; i < 10; i++) {
          securityMiddleware.recordFailedLogin(ip);

          const mockReq = {
            ip,
            path: '/api/auth/login',
            method: 'POST',
            headers: { 'user-agent': 'Malicious Bot' },
            body: { username: 'admin', password: 'password123' },
          } as any;

          const mockRes = {
            status: jest.fn().mockReturnThis(),
            json: jest.fn().mockReturnThis(),
          } as any;

          await securityMiddleware.checkRequest(mockReq, mockRes, jest.fn());
        }
      }

      // Verify multiple alerts generated
      const alerts = securityMiddleware.getActiveAlerts();
      const bruteForceAlerts = alerts.filter(
        alert => alert.type === 'brute_force_attack'
      );

      expect(bruteForceAlerts.length).toBeGreaterThanOrEqual(
        attackerIps.length
      );

      // Verify all attacker IPs are blocked
      for (const ip of attackerIps) {
        const isBlocked = securityMiddleware.isBruteForceBlocked(ip);
        expect(isBlocked).toBe(true);
      }
    });
  });

  describe('Performance Under Load', () => {
    it('should maintain security standards under high request volume', async () => {
      const startTime = Date.now();
      const totalRequests = 1000;
      const promises = [];

      for (let i = 0; i < totalRequests; i++) {
        const mockReq = {
          ip: `192.168.1.${i % 255}`,
          path: '/api/data',
          method: 'GET',
          headers: { 'user-agent': 'Load Test Client' },
        } as any;

        const mockRes = {
          status: jest.fn().mockReturnThis(),
          json: jest.fn().mockReturnThis(),
        } as any;

        promises.push(
          securityMiddleware.checkRequest(mockReq, mockRes, jest.fn())
        );
      }

      await Promise.all(promises);

      const processingTime = Date.now() - startTime;
      const requestsPerSecond = totalRequests / (processingTime / 1000);

      // Should maintain reasonable performance
      expect(requestsPerSecond).toBeGreaterThan(100);
      expect(processingTime).toBeLessThan(10000); // Under 10 seconds
    });
  });
});
