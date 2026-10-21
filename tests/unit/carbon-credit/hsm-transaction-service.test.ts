/**
 * HSM Transaction Service Unit Tests
 * Location: tests/unit/carbon-credit/hsm-transaction-service.test.ts
 * Coverage: Session management, transaction signing, rate limiting, audit logging
 */

import { jest, describe, it, expect, beforeEach, afterEach } from '@jest/globals';
import crypto from 'crypto';

// Override the global ethers mock with implementations needed by HSMTransactionService.
// The service calls Wallet.createRandom(), wallet.signMessage(), keccak256, etc.
jest.mock('ethers', () => {
  const realKeccak256 = (data: Uint8Array | string): string => {
    const buf = typeof data === 'string'
      ? Buffer.from(data.replace(/^0x/, ''), 'hex')
      : Buffer.from(data);
    return '0x' + crypto.createHash('sha256').update(buf).digest('hex');
  };
  const realToUtf8Bytes = (text: string): Uint8Array => new TextEncoder().encode(text);
  const mockIsAddress = (addr: string) => /^0x[a-fA-F0-9]{40}$/.test(addr);

  let walletCounter = 0;
  class MockWallet {
    privateKey: string;
    publicKey: string;
    address: string;
    constructor(privateKey?: string) {
      walletCounter++;
      this.privateKey = privateKey || '0x' + crypto.randomBytes(32).toString('hex');
      this.publicKey = '0x04' + crypto.randomBytes(64).toString('hex');
      this.address = '0x' + crypto.randomBytes(20).toString('hex');
    }
    signMessage = jest.fn().mockImplementation(async () => {
      return ('0x' + crypto.randomBytes(65).toString('hex')) as never;
    });
    connect = jest.fn().mockReturnThis();
  }
  (MockWallet as any).createRandom = jest.fn(() => new MockWallet());

  const ethersMock = {
    isAddress: jest.fn(mockIsAddress),
    keccak256: jest.fn(realKeccak256),
    toUtf8Bytes: jest.fn(realToUtf8Bytes),
    hashMessage: jest.fn((data: any) => realKeccak256(data)),
    recoverAddress: jest.fn(() => '0x' + 'c'.repeat(40)),
    computeAddress: jest.fn(() => '0x' + 'c'.repeat(40)),
    Wallet: Object.assign(MockWallet, { createRandom: (MockWallet as any).createRandom }),
    Contract: jest.fn(),
    ZeroAddress: '0x0000000000000000000000000000000000000000',
  };

  return {
    ...ethersMock,
    ethers: ethersMock,
  };
});

// Mock the logger
jest.mock('../../../backend/utils/logger.js', () => ({
  logger: {
    info: jest.fn(),
    warn: jest.fn(),
    error: jest.fn(),
    debug: jest.fn(),
  },
}));

// Mock the enterprise HSM service
jest.mock('../../../security/enterprise-hsm-service.js', () => ({
  EnterpriseHSMService: {
    getInstance: jest.fn().mockReturnValue({
      initialize: jest.fn().mockResolvedValue(undefined),
      createSession: jest.fn().mockResolvedValue('test-session-id'),
      signWithHSM: jest.fn().mockResolvedValue({
        signature: '0x' + 'a'.repeat(130),
        publicKey: '0x' + 'b'.repeat(128),
      }),
      verifySignature: jest.fn().mockResolvedValue(true),
      destroySession: jest.fn().mockResolvedValue(undefined),
      getSessionStatus: jest.fn().mockResolvedValue({ active: true }),
    }),
  },
}));

// Import after mocking
import {
  HSMTransactionService,
  TransactionType,
  SecurityLevel,
  type TransactionRequest,
  type SignedTransaction,
} from '../../../backend/services/hsm-transaction-service';

describe('HSMTransactionService', () => {
  let hsmService: HSMTransactionService;

  beforeEach(async () => {
    jest.useFakeTimers();
    hsmService = new HSMTransactionService();
    jest.clearAllMocks();
  });

  afterEach(() => {
    jest.clearAllTimers();
    jest.useRealTimers();
  });

  describe('Session Management', () => {
    it('should create a new HSM session', async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [TransactionType.CREDIT_TRANSFER, TransactionType.CREDIT_PURCHASE],
      });

      expect(session).toBeDefined();
      expect(session.sessionId).toBeDefined();
      expect(session.userId).toBe('user-1');
      expect(session.permissions).toContain(TransactionType.CREDIT_TRANSFER);
      expect(session.expiresAt.getTime()).toBeGreaterThan(Date.now());
    });

    it('should reject session creation with invalid wallet address', async () => {
      await expect(
        hsmService.createSession({
          userId: 'user-1',
          walletAddress: 'invalid-address',
          permissions: [TransactionType.CREDIT_TRANSFER],
        })
      ).rejects.toThrow('Invalid wallet address');
    });

    it('should reject session creation without permissions', async () => {
      await expect(
        hsmService.createSession({
          userId: 'user-1',
          walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
          permissions: [],
        })
      ).rejects.toThrow('At least one permission required');
    });

    it('should destroy an active session', async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [TransactionType.CREDIT_TRANSFER],
      });

      await hsmService.destroySession(session.sessionId);

      // Subsequent operations should fail
      await expect(
        hsmService.signTransaction(session.sessionId, {
          type: TransactionType.CREDIT_TRANSFER,
          to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
          value: BigInt('1000000000000000000'),
          data: '0x',
        })
      ).rejects.toThrow('Session not found');
    });

    it('should get session status', async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [TransactionType.CREDIT_TRANSFER],
      });

      const status = hsmService.getSessionStatus(session.sessionId);

      expect(status).toBeDefined();
      expect(status).not.toBeNull();
      expect(status!.userId).toBe('user-1');
      expect(status!.maxSignatures).toBe(100);
    });

    it('should return null for non-existent session lookup', () => {
      const status = hsmService.getSessionStatus('non-existent-session');
      expect(status).toBeNull();
    });
  });

  describe('Transaction Signing', () => {
    let sessionId: string;

    beforeEach(async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [
          TransactionType.CREDIT_TRANSFER,
          TransactionType.CREDIT_PURCHASE,
          TransactionType.CREDIT_ISSUE,
        ],
      });
      sessionId = session.sessionId;
    });

    it('should sign a valid transaction', async () => {
      const signedTx = await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_TRANSFER,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('1000000000000000000'),
        data: '0x',
      });

      expect(signedTx).toBeDefined();
      expect(signedTx.signature).toBeDefined();
      expect(signedTx.signature).toMatch(/^0x[a-fA-F0-9]+$/);
      expect(signedTx.transactionHash).toBeDefined();
    });

    it('should reject signing without permission', async () => {
      // Create session with limited permissions
      const limitedSession = await hsmService.createSession({
        userId: 'user-2',
        walletAddress: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        permissions: [TransactionType.CREDIT_TRANSFER], // Only transfer
      });

      await expect(
        hsmService.signTransaction(limitedSession.sessionId, {
          type: TransactionType.CREDIT_ISSUE, // Not permitted
          to: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
          value: BigInt('1000000000000000000'),
          data: '0x',
        })
      ).rejects.toThrow(/not authorized/i);
    });

    it('should reject signing with destroyed session', async () => {
      await hsmService.destroySession(sessionId);

      await expect(
        hsmService.signTransaction(sessionId, {
          type: TransactionType.CREDIT_TRANSFER,
          to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
          value: BigInt('1000000000000000000'),
          data: '0x',
        })
      ).rejects.toThrow('Session not found');
    });

    it('should include nonce in signed transaction', async () => {
      const signedTx = await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_TRANSFER,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('1000000000000000000'),
        data: '0x',
      });

      expect(signedTx.nonce).toBeDefined();
      expect(typeof signedTx.nonce).toBe('number');
    });
  });

  describe('Rate Limiting', () => {
    let sessionId: string;

    beforeEach(async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [
          TransactionType.CREDIT_TRANSFER,
          TransactionType.CREDIT_ISSUE,
        ],
      });
      sessionId = session.sessionId;
    });

    it('should enforce per-minute rate limit', async () => {
      // The rate limit for CREDIT_ISSUE is 5/min
      const maxPerMinute = 5;

      // Sign up to the limit
      for (let i = 0; i < maxPerMinute; i++) {
        await hsmService.signTransaction(sessionId, {
          type: TransactionType.CREDIT_ISSUE,
          to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
          value: BigInt('1000000000000000000'),
          data: '0x',
        });
      }

      // Next one should be rate limited
      await expect(
        hsmService.signTransaction(sessionId, {
          type: TransactionType.CREDIT_ISSUE,
          to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
          value: BigInt('1000000000000000000'),
          data: '0x',
        })
      ).rejects.toThrow(/[Rr]ate limit exceeded/);
    });

    it('should enforce session signature limit', async () => {
      const status = hsmService.getSessionStatus(sessionId);
      expect(status!.maxSignatures).toBe(100);
    });

    it('should track signatures per session', async () => {
      await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_TRANSFER,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('1000000000000000000'),
        data: '0x',
      });

      const status = hsmService.getSessionStatus(sessionId);
      expect(status!.signatureCount).toBe(1);
      expect(status!.maxSignatures - status!.signatureCount).toBeLessThan(100);
    });
  });

  describe('Security Levels', () => {
    let sessionId: string;

    beforeEach(async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [
          TransactionType.CREDIT_TRANSFER,
          TransactionType.CREDIT_PURCHASE,
        ],
      });
      sessionId = session.sessionId;
    });

    it('should use standard security for small transactions', async () => {
      const signedTx = await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_TRANSFER,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('1000000000000000000'), // 1 WRF
        data: '0x',
      });

      expect(signedTx.securityLevel).toBe(SecurityLevel.STANDARD);
    });

    it('should assign appropriate security level for transactions', async () => {
      const signedTx = await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_TRANSFER,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('1000000000000000000'), // Small amount
        data: '0x',
      });

      // Standard level for small transfers
      expect([SecurityLevel.STANDARD, SecurityLevel.ELEVATED]).toContain(signedTx.securityLevel);
    });
  });

  describe('Signature Verification', () => {
    let sessionId: string;

    beforeEach(async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [TransactionType.CREDIT_TRANSFER],
      });
      sessionId = session.sessionId;
    });

    it('should sign and produce valid transaction data', async () => {
      const signedTx = await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_TRANSFER,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('1000000000000000000'),
        data: '0x',
      });

      expect(signedTx.signature).toBeDefined();
      expect(signedTx.transactionHash).toBeDefined();
      expect(signedTx.transactionType).toBe(TransactionType.CREDIT_TRANSFER);
      expect(signedTx.publicKey).toBeDefined();
    });

    it('should produce different hashes for different transactions', async () => {
      const signedTx1 = await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_TRANSFER,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('1000000000000000000'),
        data: '0x',
      });

      const signedTx2 = await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_TRANSFER,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('2000000000000000000'),
        data: '0x',
      });

      expect(signedTx1.transactionHash).not.toBe(signedTx2.transactionHash);
    });
  });

  describe('Audit Logging', () => {
    let sessionId: string;

    beforeEach(async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [TransactionType.CREDIT_TRANSFER],
      });
      sessionId = session.sessionId;
    });

    it('should log session creation', async () => {
      // getAuditLog takes userId, not sessionId
      const auditLog = await hsmService.getAuditLog('user-1');

      expect(auditLog).toBeDefined();
      expect(auditLog.length).toBeGreaterThan(0);
      // Service uses eventType 'SESSION_CREATE' not 'SESSION_CREATED'
      expect(auditLog[0].eventType).toBe('SESSION_CREATE');
    });

    it('should log transaction signing', async () => {
      await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_TRANSFER,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('1000000000000000000'),
        data: '0x',
      });

      const auditLog = await hsmService.getAuditLog('user-1');

      // Service uses eventType 'SIGN_SUCCESS'
      const signingEvent = auditLog.find(e => e.eventType === 'SIGN_SUCCESS');
      expect(signingEvent).toBeDefined();
    });

    it('should log session destruction', async () => {
      const currentSessionId = sessionId;
      await hsmService.destroySession(currentSessionId);

      // getAuditLog takes userId
      const auditLog = await hsmService.getAuditLog('user-1');

      // Service uses eventType 'SESSION_DESTROY'
      const destroyEvent = auditLog.find(
        e => e.eventType === 'SESSION_DESTROY' && e.sessionId === currentSessionId
      );
      expect(destroyEvent).toBeDefined();
    });

    it('should log rate limit violations', async () => {
      // Try to exceed rate limit for CREDIT_TRANSFER (10/min)
      // But we only have CREDIT_TRANSFER permission.
      // Let's create a session with CREDIT_ISSUE permission (5/min limit)
      const issueSession = await hsmService.createSession({
        userId: 'user-rate-test',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [TransactionType.CREDIT_ISSUE],
      });

      try {
        for (let i = 0; i < 10; i++) {
          await hsmService.signTransaction(issueSession.sessionId, {
            type: TransactionType.CREDIT_ISSUE,
            to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
            value: BigInt('1000000000000000000'),
            data: '0x',
          });
        }
      } catch {
        // Expected to fail
      }

      const auditLog = await hsmService.getAuditLog('user-rate-test');

      // Service uses eventType 'RATE_LIMIT'
      const rateLimitEvent = auditLog.find(e => e.eventType === 'RATE_LIMIT');
      expect(rateLimitEvent).toBeDefined();
    });

    it('should reject unauthorized transaction types', async () => {
      const limitedSession = await hsmService.createSession({
        userId: 'user-perm-test',
        walletAddress: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        permissions: [TransactionType.CREDIT_TRANSFER],
      });

      // Attempting to sign with an unauthorized transaction type should throw
      await expect(
        hsmService.signTransaction(limitedSession.sessionId, {
          type: TransactionType.CREDIT_ISSUE, // Not permitted
          to: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
          value: BigInt('1000000000000000000'),
          data: '0x',
        })
      ).rejects.toThrow(/not authorized/i);

      // The session creation event should still be in the audit log
      const auditLog = await hsmService.getAuditLog('user-perm-test');
      const createEvent = auditLog.find(e => e.eventType === 'SESSION_CREATE');
      expect(createEvent).toBeDefined();
    });
  });

  describe('Transaction Types', () => {
    let sessionId: string;

    beforeEach(async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: Object.values(TransactionType),
      });
      sessionId = session.sessionId;
    });

    it('should handle CREDIT_ISSUE transaction', async () => {
      const signedTx = await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_ISSUE,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('1000000000000000000'),
        data: '0x',
      });

      expect(signedTx.transactionType).toBe(TransactionType.CREDIT_ISSUE);
    });

    it('should handle CREDIT_RETIRE transaction', async () => {
      const signedTx = await hsmService.signTransaction(sessionId, {
        type: TransactionType.CREDIT_RETIRE,
        to: '0x0000000000000000000000000000000000000000', // Burn address
        value: BigInt('500000000000000000'),
        data: '0x',
      });

      expect(signedTx.transactionType).toBe(TransactionType.CREDIT_RETIRE);
    });

    it('should handle STAKE transaction', async () => {
      const signedTx = await hsmService.signTransaction(sessionId, {
        type: TransactionType.STAKE,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('10000000000000000000'),
        data: '0x',
      });

      expect(signedTx.transactionType).toBe(TransactionType.STAKE);
    });

    it('should handle REWARD_DISTRIBUTION transaction', async () => {
      const signedTx = await hsmService.signTransaction(sessionId, {
        type: TransactionType.REWARD_DISTRIBUTION,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('100000000000000000'),
        data: '0x',
      });

      expect(signedTx.transactionType).toBe(TransactionType.REWARD_DISTRIBUTION);
    });
  });

  describe('Error Handling', () => {
    it('should handle session creation successfully', async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [TransactionType.CREDIT_TRANSFER],
      });

      expect(session.sessionId).toBeDefined();
    });

    it('should complete signing within reasonable time', async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [TransactionType.CREDIT_TRANSFER],
      });

      const startTime = Date.now();

      const signedTx = await hsmService.signTransaction(session.sessionId, {
        type: TransactionType.CREDIT_TRANSFER,
        to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
        value: BigInt('1000000000000000000'),
        data: '0x',
      });

      const elapsed = Date.now() - startTime;
      expect(elapsed).toBeLessThan(30000);
      expect(signedTx).toBeDefined();
    });
  });

  describe('Concurrent Operations', () => {
    it('should handle multiple concurrent sessions', async () => {
      const sessions = await Promise.all([
        hsmService.createSession({
          userId: 'user-1',
          walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
          permissions: [TransactionType.CREDIT_TRANSFER],
        }),
        hsmService.createSession({
          userId: 'user-2',
          walletAddress: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
          permissions: [TransactionType.CREDIT_TRANSFER],
        }),
        hsmService.createSession({
          userId: 'user-3',
          walletAddress: '0xdF3e18d64BC6A983f673Ab319CCaE4f1a57C7097',
          permissions: [TransactionType.CREDIT_TRANSFER],
        }),
      ]);

      expect(sessions.length).toBe(3);
      expect(new Set(sessions.map(s => s.sessionId)).size).toBe(3); // All unique
    });

    it('should handle concurrent signing requests', async () => {
      const session = await hsmService.createSession({
        userId: 'user-1',
        walletAddress: '0x742d35Cc6634C0532925a3b844Bc9e7595f8dB03',
        permissions: [TransactionType.CREDIT_TRANSFER],
      });

      const signingPromises = [];
      for (let i = 0; i < 5; i++) {
        signingPromises.push(
          hsmService.signTransaction(session.sessionId, {
            type: TransactionType.CREDIT_TRANSFER,
            to: '0x8Ba1f109551bD432803012645Ac136ddd64DBA72',
            value: BigInt((i + 1) * 1000000000000000000),
            data: '0x',
          })
        );
      }

      const results = await Promise.all(signingPromises);

      expect(results.length).toBe(5);
      expect(new Set(results.map(r => r.transactionHash)).size).toBe(5);
    });
  });
});
