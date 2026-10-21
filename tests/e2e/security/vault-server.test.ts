/**
 * Vault Server Tests
 * Tests for enterprise secret management with TypeScript types
 */

import request from 'supertest';
import { VaultServer } from '../../backend/server/vault-server';
import {
  VaultConfig,
  SecretRotationPolicy,
} from '../../backend/shared/types/security-types';

describe('Vault Server', () => {
  let vaultServer: VaultServer;
  let app: any;

  const mockVaultConfig: VaultConfig = {
    url: 'http://localhost:8200',
    token: 'test-token',
    timeout: 5000,
    retryAttempts: 3,
  };

  beforeEach(async () => {
    vaultServer = new VaultServer(mockVaultConfig);
    app = vaultServer.getApp();
    await vaultServer.initialize();
  });

  afterEach(async () => {
    await vaultServer.shutdown();
    jest.clearAllMocks();
  });

  describe('Server Initialization', () => {
    it('should start server on correct port', async () => {
      const response = await request(app).get('/health').expect(200);

      expect(response.body.status).toBe('healthy');
      expect(response.body.vault).toBeDefined();
    });

    it('should initialize with proper CORS and security headers', async () => {
      const response = await request(app).get('/health');

      expect(response.headers['x-frame-options']).toBe('DENY');
      expect(response.headers['x-content-type-options']).toBe('nosniff');
    });
  });

  describe('Secret Management', () => {
    const testSecret = {
      path: 'secret/test',
      data: { password: 'test-password', api_key: 'test-api-key' },
    };

    it('should store secret with proper encryption', async () => {
      const response = await request(app)
        .post('/api/vault/secrets')
        .send(testSecret)
        .expect(201);

      expect(response.body.success).toBe(true);
      expect(response.body.path).toBe(testSecret.path);
      expect(response.body.version).toBeDefined();
    });

    it('should retrieve secret with decryption', async () => {
      // First store the secret
      await request(app).post('/api/vault/secrets').send(testSecret);

      // Then retrieve it
      const response = await request(app)
        .get(`/api/vault/secrets?path=${encodeURIComponent(testSecret.path)}`)
        .expect(200);

      expect(response.body.success).toBe(true);
      expect(response.body.data).toEqual(testSecret.data);
    });

    it('should version secrets correctly', async () => {
      // Store initial version
      await request(app).post('/api/vault/secrets').send(testSecret);

      // Update secret
      const updatedSecret = {
        ...testSecret,
        data: { password: 'updated-password' },
      };

      const response = await request(app)
        .post('/api/vault/secrets')
        .send(updatedSecret)
        .expect(201);

      expect(response.body.version).toBe(2);
    });

    it('should handle secret not found', async () => {
      const response = await request(app)
        .get('/api/vault/secrets?path=secret/nonexistent')
        .expect(404);

      expect(response.body.success).toBe(false);
      expect(response.body.error).toContain('not found');
    });
  });

  describe('Secret Rotation', () => {
    const rotationPolicy: SecretRotationPolicy = {
      enabled: true,
      rotationInterval: '24h',
      lastRotated: new Date(),
      nextRotation: new Date(Date.now() + 24 * 60 * 60 * 1000),
    };

    it('should configure rotation policy', async () => {
      const response = await request(app)
        .post('/api/vault/rotation-policy')
        .send({
          path: 'secret/test',
          policy: rotationPolicy,
        })
        .expect(201);

      expect(response.body.success).toBe(true);
      expect(response.body.policy.enabled).toBe(true);
    });

    it('should rotate secrets automatically', async () => {
      const testPath = 'secret/auto-rotate';

      // Store initial secret
      await request(app)
        .post('/api/vault/secrets')
        .send({
          path: testPath,
          data: { api_key: 'initial-key' },
        });

      // Set up rotation policy
      await request(app).post('/api/vault/rotation-policy').send({
        path: testPath,
        policy: rotationPolicy,
      });

      // Trigger manual rotation
      const response = await request(app)
        .post('/api/vault/rotate')
        .send({ path: testPath })
        .expect(200);

      expect(response.body.success).toBe(true);
      expect(response.body.newVersion).toBeDefined();
    });

    it('should maintain rotation audit trail', async () => {
      const testPath = 'secret/audit-test';

      await request(app)
        .post('/api/vault/secrets')
        .send({
          path: testPath,
          data: { key: 'value' },
        });

      await request(app).post('/api/vault/rotate').send({ path: testPath });

      const response = await request(app)
        .get('/api/vault/audit-logs')
        .expect(200);

      const rotationLog = response.body.logs.find(
        (log: any) =>
          log.event === 'SECRET_ROTATION' && log.requestPath?.includes(testPath)
      );

      expect(rotationLog).toBeDefined();
    });
  });

  describe('Database Credentials', () => {
    it('should generate dynamic database credentials', async () => {
      const response = await request(app)
        .post('/api/vault/database/credentials')
        .send({
          role: 'readonly',
          ttl: '1h',
        })
        .expect(201);

      expect(response.body.success).toBe(true);
      expect(response.body.credentials.username).toBeDefined();
      expect(response.body.credentials.password).toBeDefined();
      expect(response.body.lease_duration).toBe(3600);
    });

    it('should revoke database credentials', async () => {
      // Generate credentials first
      const credResponse = await request(app)
        .post('/api/vault/database/credentials')
        .send({
          role: 'readonly',
          ttl: '1h',
        });

      const leaseId = credResponse.body.lease_id;

      // Revoke them
      const response = await request(app)
        .delete(`/api/vault/database/credentials/${leaseId}`)
        .expect(200);

      expect(response.body.success).toBe(true);
    });
  });

  describe('Health Monitoring', () => {
    it('should report vault health status', async () => {
      const response = await request(app).get('/api/vault/health').expect(200);

      expect(response.body.vault_status).toBeDefined();
      expect(response.body.sealed).toBe(false);
      expect(response.body.initialized).toBe(true);
    });

    it('should provide system metrics', async () => {
      const response = await request(app).get('/api/vault/metrics').expect(200);

      expect(response.body.secrets_count).toBeGreaterThanOrEqual(0);
      expect(response.body.active_leases).toBeGreaterThanOrEqual(0);
      expect(response.body.uptime).toBeDefined();
    });
  });

  describe('Error Handling', () => {
    it('should handle invalid secret paths', async () => {
      const response = await request(app)
        .post('/api/vault/secrets')
        .send({
          path: '', // Invalid empty path
          data: { key: 'value' },
        })
        .expect(400);

      expect(response.body.success).toBe(false);
      expect(response.body.error).toContain('path');
    });

    it('should handle vault connection failures', async () => {
      const faultyVault = new VaultServer({
        ...mockVaultConfig,
        url: 'http://localhost:9999', // Non-existent vault
      });

      const faultyApp = faultyVault.getApp();

      const response = await request(faultyApp)
        .get('/api/vault/health')
        .expect(503);

      expect(response.body.error).toContain('Vault unavailable');
    });

    it('should rate limit requests', async () => {
      const requests = Array.from({ length: 20 }, (_, i) =>
        request(app).get('/health')
      );

      const responses = await Promise.all(requests);
      const rateLimited = responses.some(r => r.status === 429);

      expect(rateLimited).toBe(true);
    });
  });

  describe('Type Safety', () => {
    it('should enforce VaultConfig types', () => {
      expect(() => {
        new VaultServer({
          url: 'invalid-url', // Should be valid URL
          token: '',
          timeout: -1, // Should be positive
          retryAttempts: 0,
        } as VaultConfig);
      }).toThrow();
    });

    it('should validate rotation policy types', async () => {
      const invalidPolicy = {
        enabled: 'yes', // Should be boolean
        rotationInterval: 123, // Should be string
        lastRotated: 'invalid-date', // Should be Date
        nextRotation: null,
      };

      const response = await request(app)
        .post('/api/vault/rotation-policy')
        .send({
          path: 'secret/test',
          policy: invalidPolicy,
        })
        .expect(400);

      expect(response.body.error).toContain('validation');
    });
  });
});
