/**
 * PKCS#11 Provider Unit Tests
 *
 * Tests for the HSM integration layer including:
 * - Mock provider functionality
 * - SecureHSM wrapper
 * - Key generation, signing, and verification
 *
 * Run with: npm run test:security
 * Or: npx jest tests/unit/security/pkcs11-provider.test.js
 */

// Force mock mode for tests
process.env.HSM_MOCK_MODE = 'true';

const SecureHSM = require('../../../security/hsm-secure-implementation.cjs');
const { PKCS11MockProvider, isMockModeEnabled } = require('../../../security/pkcs11-mock-provider.cjs');

describe('PKCS11 Mock Provider', () => {
  let provider;

  beforeEach(() => {
    provider = new PKCS11MockProvider();
  });

  afterEach(async () => {
    if (provider.isInitialized) {
      await provider.finalize();
    }
  });

  describe('Initialization', () => {
    test('should initialize successfully', async () => {
      const deviceInfo = await provider.initialize();

      expect(deviceInfo).toBeDefined();
      expect(deviceInfo.id).toMatch(/^mock-hsm-/);
      expect(deviceInfo.type).toBe('mock-hsm');
      expect(deviceInfo.isHardware).toBe(false);
      expect(deviceInfo.warning).toContain('DEVELOPMENT ONLY');
    });

    test('should return same device info on subsequent initialize calls', async () => {
      const deviceInfo1 = await provider.initialize();
      const deviceInfo2 = await provider.initialize();

      expect(deviceInfo1.id).toBe(deviceInfo2.id);
    });

    test('isMockModeEnabled should return true in test environment', () => {
      expect(isMockModeEnabled()).toBe(true);
    });
  });

  describe('Login', () => {
    beforeEach(async () => {
      await provider.initialize();
    });

    test('should login successfully with valid PIN', async () => {
      const result = await provider.login('123456');

      expect(result.loggedIn).toBe(true);
      expect(result.sessionId).toBeDefined();
      expect(result.warning).toContain('Mock session');
    });

    test('should reject PIN shorter than 4 characters', async () => {
      await expect(provider.login('123')).rejects.toThrow('PIN must be at least 4 characters');
    });

    test('should require initialization before login', async () => {
      const newProvider = new PKCS11MockProvider();
      await expect(newProvider.login('123456')).rejects.toThrow('Provider not initialized');
    });
  });

  describe('Key Generation', () => {
    beforeEach(async () => {
      await provider.initialize();
      await provider.login('123456');
    });

    test('should generate P-256 key pair', async () => {
      const keyInfo = await provider.generateKeyPair('test-key', { curve: 'P-256' });

      expect(keyInfo.keyHandle).toBeDefined();
      expect(keyInfo.keyId).toBe('test-key');
      expect(keyInfo.curve).toBe('P-256');
      expect(keyInfo.publicKey).toBeDefined();
      expect(keyInfo.algorithm).toBe('ECDSA-P-256');
      expect(keyInfo.hsmStored).toBe(false); // Mock keys are not HSM-stored
    });

    test('should generate P-384 key pair', async () => {
      const keyInfo = await provider.generateKeyPair('test-key-384', { curve: 'P-384' });

      expect(keyInfo.curve).toBe('P-384');
      expect(keyInfo.algorithm).toBe('ECDSA-P-384');
    });

    test('should require login before key generation', async () => {
      await provider.logout();
      await expect(provider.generateKeyPair('test-key')).rejects.toThrow('Not logged in');
    });
  });

  describe('Signing', () => {
    let keyHandle;

    beforeEach(async () => {
      await provider.initialize();
      await provider.login('123456');
      const keyInfo = await provider.generateKeyPair('signing-key');
      keyHandle = keyInfo.keyHandle;
    });

    test('should sign data successfully', async () => {
      const data = 'Test data to sign';
      const result = await provider.sign(data, keyHandle);

      expect(result.signature).toBeDefined();
      expect(result.signature.length).toBeGreaterThan(0);
      expect(result.algorithm).toBe('ECDSA-SHA256');
      expect(result.hsmSigned).toBe(false); // Mock provider
    });

    test('should sign buffer data', async () => {
      const data = Buffer.from('Test buffer data');
      const result = await provider.sign(data, keyHandle);

      expect(result.signature).toBeDefined();
      expect(result.dataHash).toBeDefined();
    });

    test('should reject invalid key handle', async () => {
      await expect(provider.sign('data', 'invalid-handle')).rejects.toThrow('Key handle not found');
    });

    test('should require login for signing', async () => {
      await provider.logout();
      await expect(provider.sign('data', keyHandle)).rejects.toThrow('Not logged in');
    });
  });

  describe('Verification', () => {
    let keyHandle;
    const testData = 'Data to verify';

    beforeEach(async () => {
      await provider.initialize();
      await provider.login('123456');
      const keyInfo = await provider.generateKeyPair('verify-key');
      keyHandle = keyInfo.keyHandle;
    });

    test('should verify valid signature', async () => {
      const signResult = await provider.sign(testData, keyHandle);
      const isValid = await provider.verify(testData, signResult.signature, keyHandle);

      expect(isValid).toBe(true);
    });

    test('should reject invalid signature', async () => {
      const invalidSignature = 'invalid-signature-hex';
      const isValid = await provider.verify(testData, invalidSignature, keyHandle);

      expect(isValid).toBe(false);
    });

    test('should reject signature for wrong data', async () => {
      const signResult = await provider.sign(testData, keyHandle);
      const isValid = await provider.verify('Different data', signResult.signature, keyHandle);

      expect(isValid).toBe(false);
    });
  });

  describe('Key Management', () => {
    beforeEach(async () => {
      await provider.initialize();
      await provider.login('123456');
    });

    test('should list generated keys', async () => {
      await provider.generateKeyPair('key-1');
      await provider.generateKeyPair('key-2');

      const keys = await provider.listKeys();

      expect(keys.length).toBe(2);
      expect(keys.some(k => k.label === 'key-1')).toBe(true);
      expect(keys.some(k => k.label === 'key-2')).toBe(true);
    });

    test('should delete key', async () => {
      const keyInfo = await provider.generateKeyPair('delete-me');
      await provider.deleteKey(keyInfo.keyHandle);

      const keys = await provider.listKeys();
      expect(keys.some(k => k.handle === keyInfo.keyHandle)).toBe(false);
    });

    test('should reject delete of non-existent key', async () => {
      await expect(provider.deleteKey('non-existent')).rejects.toThrow('Key handle not found');
    });
  });

  describe('Status and Audit', () => {
    beforeEach(async () => {
      await provider.initialize();
    });

    test('should return status', () => {
      const status = provider.getStatus();

      expect(status.isInitialized).toBe(true);
      expect(status.isLoggedIn).toBe(false);
      expect(status.isMockMode).toBe(true);
      expect(status.keyCount).toBe(0);
    });

    test('should maintain audit log', async () => {
      await provider.login('123456');
      await provider.generateKeyPair('audit-test');

      const auditLog = provider.getAuditLog();

      expect(auditLog.length).toBeGreaterThan(0);
      expect(auditLog.some(e => e.event === 'MOCK_DEVICE_INITIALIZED')).toBe(true);
      expect(auditLog.some(e => e.event === 'MOCK_SESSION_OPENED')).toBe(true);
      expect(auditLog.some(e => e.event === 'MOCK_KEY_GENERATED')).toBe(true);
    });
  });

  describe('Finalization', () => {
    test('should finalize and clean up', async () => {
      await provider.initialize();
      await provider.login('123456');
      await provider.generateKeyPair('test-key');

      await provider.finalize();

      expect(provider.isInitialized).toBe(false);
      expect(provider.keyHandles.size).toBe(0);
    });
  });
});

describe('SecureHSM Wrapper', () => {
  let hsm;

  beforeEach(() => {
    hsm = new SecureHSM();
  });

  afterEach(async () => {
    if (hsm.isInitialized) {
      await hsm.shutdown();
    }
  });

  describe('Initialization', () => {
    test('should initialize with mock provider in test mode', async () => {
      await hsm.initialize();

      expect(hsm.isInitialized).toBe(true);
      expect(hsm.deviceId).toBeDefined();
    });

    test('should detect mock mode status', async () => {
      await hsm.initialize();

      const status = hsm.getStatus();
      expect(status.isMockMode).toBe(true);
      expect(status.isHardware).toBe(false);
    });
  });

  describe('Session Management', () => {
    beforeEach(async () => {
      await hsm.initialize();
    });

    test('should authenticate session', async () => {
      const sessionId = await hsm.authenticateSession({
        userId: 'test-user',
        pin: '123456',
      });

      expect(sessionId).toBeDefined();
      expect(sessionId.length).toBeGreaterThan(0);
    });

    test('should reject invalid credentials', async () => {
      await expect(
        hsm.authenticateSession({
          userId: 'test-user',
          // Missing PIN
        })
      ).rejects.toThrow('Authentication failed');
    });

    test('should reject short PIN', async () => {
      await expect(
        hsm.authenticateSession({
          userId: 'test-user',
          pin: '123', // Too short
        })
      ).rejects.toThrow('PIN must be 4-8 characters');
    });

    test('should terminate session', async () => {
      const sessionId = await hsm.authenticateSession({
        userId: 'test-user',
        pin: '123456',
      });

      hsm.terminateSession(sessionId);

      const status = hsm.getStatus();
      expect(status.activeSessionCount).toBe(0);
    });
  });

  describe('Key Operations', () => {
    let sessionId;

    beforeEach(async () => {
      await hsm.initialize();
      sessionId = await hsm.authenticateSession({
        userId: 'test-user',
        pin: '123456',
      });
    });

    test('should generate key pair', async () => {
      const keyInfo = await hsm.generateKeyPair('test-signing');

      expect(keyInfo.keyHandle).toBeDefined();
      expect(keyInfo.publicKey).toBeDefined();
      expect(keyInfo.keyType).toBe('test-signing');
      expect(keyInfo.algorithm).toBe('ECDSA-P256');
    });

    test('should sign and verify data', async () => {
      const keyInfo = await hsm.generateKeyPair('signing-key');
      const testData = 'Test message for signing';

      const signResult = await hsm.signWithHSM(testData, keyInfo.keyHandle, sessionId);
      expect(signResult.signature).toBeDefined();

      const isValid = await hsm.verifySignature(testData, signResult.signature, keyInfo.keyHandle);
      expect(isValid).toBe(true);
    });

    test('should reject signing with invalid session', async () => {
      const keyInfo = await hsm.generateKeyPair('signing-key');

      await expect(
        hsm.signWithHSM('data', keyInfo.keyHandle, 'invalid-session')
      ).rejects.toThrow('Invalid or expired session');
    });

    test('should reject signing with invalid key handle', async () => {
      await expect(
        hsm.signWithHSM('data', 'invalid-key', sessionId)
      ).rejects.toThrow('Invalid key handle');
    });

    test('should list keys', async () => {
      await hsm.generateKeyPair('key-1');
      await hsm.generateKeyPair('key-2');

      const keys = hsm.listKeys();

      expect(keys.length).toBe(2);
    });

    test('should get public key', async () => {
      const keyInfo = await hsm.generateKeyPair('pubkey-test');
      const publicKey = hsm.getPublicKey(keyInfo.keyHandle);

      expect(publicKey).toBeDefined();
    });
  });

  describe('Audit Logging', () => {
    test('should maintain audit log', async () => {
      await hsm.initialize();
      await hsm.authenticateSession({
        userId: 'test-user',
        pin: '123456',
      });

      const auditLog = hsm.getAuditLog();

      expect(auditLog.length).toBeGreaterThan(0);
      expect(auditLog.some(e => e.event === 'HSM_INITIALIZED')).toBe(true);
      expect(auditLog.some(e => e.event === 'SESSION_AUTHENTICATED')).toBe(true);
    });
  });

  describe('Status', () => {
    test('should return comprehensive status', async () => {
      await hsm.initialize();
      await hsm.authenticateSession({
        userId: 'test-user',
        pin: '123456',
      });
      await hsm.generateKeyPair('status-test');

      const status = hsm.getStatus();

      expect(status.isInitialized).toBe(true);
      expect(status.deviceId).toBeDefined();
      expect(status.keyCount).toBe(1);
      expect(status.activeSessionCount).toBe(1);
      expect(status.provider).toBeDefined();
    });
  });

  describe('Shutdown', () => {
    test('should shut down cleanly', async () => {
      await hsm.initialize();
      await hsm.authenticateSession({
        userId: 'test-user',
        pin: '123456',
      });

      await hsm.shutdown();

      expect(hsm.isInitialized).toBe(false);

      const auditLog = hsm.getAuditLog();
      expect(auditLog.some(e => e.event === 'HSM_SHUTDOWN')).toBe(true);
    });
  });
});

describe('Production Mode Protection', () => {
  const originalEnv = process.env;

  beforeEach(() => {
    // Reset environment
    jest.resetModules();
    process.env = { ...originalEnv };
  });

  afterEach(() => {
    process.env = originalEnv;
  });

  test('should reject mock mode in production', async () => {
    process.env.NODE_ENV = 'production';
    process.env.HSM_MOCK_MODE = 'true';

    // Re-require to get fresh instance
    const SecureHSMFresh = require('../../../security/hsm-secure-implementation.cjs');
    const hsmProduction = new SecureHSMFresh();

    await expect(hsmProduction.initialize()).rejects.toThrow('CRITICAL SECURITY ERROR');
  });

  test('should require PKCS11_LIB in production', async () => {
    process.env.NODE_ENV = 'production';
    delete process.env.HSM_MOCK_MODE;
    delete process.env.PKCS11_LIB;
    delete process.env.HSM_ENDPOINT;

    const SecureHSMFresh = require('../../../security/hsm-secure-implementation.cjs');
    const hsmProduction = new SecureHSMFresh();

    await expect(hsmProduction.initialize()).rejects.toThrow('PKCS11_LIB or HSM_ENDPOINT must be configured');
  });
});
