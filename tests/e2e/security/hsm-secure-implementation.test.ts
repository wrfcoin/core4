/**
 * HSM Secure Implementation Tests
 * Tests for hardware security module functionality with TypeScript types
 */

import { HSMSecureImplementation } from '../../security/hsm-secure-implementation';
import {
  HSMConfiguration,
  HSMKeyPair,
  SecurityAuditLog,
} from '../../backend/shared/types/security-types';

describe('HSM Secure Implementation', () => {
  let hsmImplementation: HSMSecureImplementation;
  let mockConfig: HSMConfiguration;

  beforeEach(() => {
    mockConfig = {
      provider: 'SoftHSM',
      credentials: {
        keyId: 'test-key-id',
        pin: 'test-pin',
      },
      settings: {
        sessionTimeout: 30000,
        maxRetries: 3,
        auditLogging: true,
        keyRotationInterval: 86400000,
      },
    };

    hsmImplementation = new HSMSecureImplementation(mockConfig);
  });

  afterEach(() => {
    jest.clearAllMocks();
  });

  describe('Key Generation', () => {
    it('should generate ECDSA key pair with proper types', async () => {
      const keyPair = await hsmImplementation.generateKeyPair('ECDSA-P256');

      expect(keyPair).toBeDefined();
      expect(keyPair.algorithm).toBe('ECDSA-P256');
      expect(keyPair.keyHandle).toMatch(/^hsm_key_/);
      expect(keyPair.publicKey).toBeTruthy();
      expect(keyPair.keyType).toBe('asymmetric');
      expect(keyPair.createdAt).toBeInstanceOf(Date);
    });

    it('should generate RSA key pair with correct metadata', async () => {
      const keyPair = await hsmImplementation.generateKeyPair('RSA-2048');

      expect(keyPair.algorithm).toBe('RSA-2048');
      expect(keyPair.keyType).toBe('asymmetric');
      expect(keyPair.publicKey.length).toBeGreaterThan(100);
    });

    it('should throw error for unsupported algorithm', async () => {
      await expect(
        hsmImplementation.generateKeyPair('INVALID-ALGO' as any)
      ).rejects.toThrow('Unsupported algorithm');
    });
  });

  describe('Digital Signatures', () => {
    let testKeyPair: HSMKeyPair;
    const testData = 'test message for signing';

    beforeEach(async () => {
      testKeyPair = await hsmImplementation.generateKeyPair('ECDSA-P256');
    });

    it('should sign data and return valid signature', async () => {
      const signature = await hsmImplementation.signData(
        testKeyPair.keyHandle,
        testData
      );

      expect(signature).toBeDefined();
      expect(typeof signature).toBe('string');
      expect(signature.length).toBeGreaterThan(0);
    });

    it('should verify signature successfully', async () => {
      const signature = await hsmImplementation.signData(
        testKeyPair.keyHandle,
        testData
      );
      const isValid = await hsmImplementation.verifySignature(
        testKeyPair.publicKey,
        testData,
        signature
      );

      expect(isValid).toBe(true);
    });

    it('should reject invalid signature', async () => {
      const isValid = await hsmImplementation.verifySignature(
        testKeyPair.publicKey,
        testData,
        'invalid-signature'
      );

      expect(isValid).toBe(false);
    });
  });

  describe('Key Management', () => {
    it('should list generated keys', async () => {
      await hsmImplementation.generateKeyPair('ECDSA-P256');
      await hsmImplementation.generateKeyPair('RSA-2048');

      const keys = await hsmImplementation.listKeys();

      expect(keys.length).toBeGreaterThanOrEqual(2);
      expect(keys[0]).toHaveProperty('keyHandle');
      expect(keys[0]).toHaveProperty('algorithm');
      expect(keys[0]).toHaveProperty('createdAt');
    });

    it('should rotate keys and maintain audit log', async () => {
      const oldKeyPair = await hsmImplementation.generateKeyPair('ECDSA-P256');
      const newKeyPair = await hsmImplementation.rotateKey(
        oldKeyPair.keyHandle,
        'ECDSA-P256'
      );

      expect(newKeyPair.keyHandle).not.toBe(oldKeyPair.keyHandle);
      expect(newKeyPair.algorithm).toBe('ECDSA-P256');

      const auditLogs = await hsmImplementation.getAuditLogs();
      const rotationLog = auditLogs.find(log => log.event === 'KEY_ROTATION');
      expect(rotationLog).toBeDefined();
    });

    it('should delete key securely', async () => {
      const keyPair = await hsmImplementation.generateKeyPair('ECDSA-P256');
      const deleted = await hsmImplementation.deleteKey(keyPair.keyHandle);

      expect(deleted).toBe(true);

      const keys = await hsmImplementation.listKeys();
      const deletedKey = keys.find(k => k.keyHandle === keyPair.keyHandle);
      expect(deletedKey).toBeUndefined();
    });
  });

  describe('Audit Logging', () => {
    it('should log key generation events', async () => {
      await hsmImplementation.generateKeyPair('ECDSA-P256');

      const auditLogs = await hsmImplementation.getAuditLogs();
      const keyGenLog = auditLogs.find(log => log.event === 'KEY_GENERATION');

      expect(keyGenLog).toBeDefined();
      expect(keyGenLog?.success).toBe(true);
      expect(keyGenLog?.algorithm).toBe('ECDSA-P256');
    });

    it('should log signature operations', async () => {
      const keyPair = await hsmImplementation.generateKeyPair('ECDSA-P256');
      await hsmImplementation.signData(keyPair.keyHandle, 'test data');

      const auditLogs = await hsmImplementation.getAuditLogs();
      const signLog = auditLogs.find(log => log.event === 'DIGITAL_SIGNATURE');

      expect(signLog).toBeDefined();
      expect(signLog?.keyHandle).toBe(keyPair.keyHandle);
    });

    it('should maintain audit log integrity', async () => {
      const initialLogCount = (await hsmImplementation.getAuditLogs()).length;

      await hsmImplementation.generateKeyPair('RSA-2048');
      const keyPair = await hsmImplementation.generateKeyPair('ECDSA-P256');
      await hsmImplementation.signData(keyPair.keyHandle, 'test');

      const finalLogs = await hsmImplementation.getAuditLogs();
      expect(finalLogs.length).toBe(initialLogCount + 3); // 2 key gen + 1 sign
    });
  });

  describe('Error Handling', () => {
    it('should handle invalid key handle gracefully', async () => {
      await expect(
        hsmImplementation.signData('invalid-handle', 'test data')
      ).rejects.toThrow('Key not found');
    });

    it('should handle HSM connection failures', async () => {
      const disconnectedHSM = new HSMSecureImplementation({
        ...mockConfig,
        credentials: { keyId: 'invalid' },
      });

      await expect(
        disconnectedHSM.generateKeyPair('ECDSA-P256')
      ).rejects.toThrow();
    });
  });

  describe('Type Safety', () => {
    it('should enforce branded types for key handles', async () => {
      const keyPair = await hsmImplementation.generateKeyPair('ECDSA-P256');

      // TypeScript should enforce HSMKeyHandle type
      expect(keyPair.keyHandle).toMatch(/^hsm_key_/);
    });

    it('should validate cryptographic algorithms', () => {
      const validAlgorithms = ['ECDSA-P256', 'RSA-2048', 'RSA-4096', 'Ed25519'];

      validAlgorithms.forEach(algo => {
        expect(() => {
          // This should not throw at runtime for valid algorithms
          hsmImplementation.validateAlgorithm(algo as any);
        }).not.toThrow();
      });
    });
  });
});
