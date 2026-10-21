/**
 * Atomic Swap / HTLC Tests
 * Task 114.4: Timelock, preimage, refund paths, cross-chain coordination
 *
 * Comprehensive tests for Hash Time-Locked Contracts (HTLC)
 * used for trustless cross-chain atomic swaps
 * Target: 15 tests
 */

import { describe, it, expect, beforeEach, jest } from '@jest/globals';

enum SwapStatus {
  INITIATED = 'INITIATED',
  FUNDED = 'FUNDED',
  CLAIMED = 'CLAIMED',
  REFUNDED = 'REFUNDED',
  EXPIRED = 'EXPIRED',
}

interface HTLC {
  id: string;
  initiator: string;
  participant: string;
  amount: bigint;
  hashLock: string; // SHA256 hash of the preimage
  timeLock: number; // Unix timestamp
  preimage?: string;
  status: SwapStatus;
  chainId: string;
  createdAt: Date;
  claimedAt?: Date;
  refundedAt?: Date;
}

interface AtomicSwap {
  id: string;
  initiatorHTLC: HTLC;
  participantHTLC: HTLC;
  status: 'PENDING' | 'ACTIVE' | 'COMPLETED' | 'FAILED';
  createdAt: Date;
  completedAt?: Date;
}

// Mock HTLC Service
class MockHTLCService {
  private htlcs: Map<string, HTLC> = new Map();
  private swaps: Map<string, AtomicSwap> = new Map();
  private htlcCounter = 0;
  private swapCounter = 0;

  private readonly DEFAULT_TIMELOCK_HOURS = 48;
  private readonly MIN_TIMELOCK_HOURS = 1;

  // Generate SHA256 hash (simplified for testing)
  generateHashLock(preimage: string): string {
    let hash = 0;
    for (let i = 0; i < preimage.length; i++) {
      const char = preimage.charCodeAt(i);
      hash = (hash << 5) - hash + char;
      hash = hash & hash;
    }
    return `sha256_${Math.abs(hash).toString(16).padStart(64, '0')}`;
  }

  // Generate random preimage
  generatePreimage(): string {
    const bytes = new Array(32)
      .fill(0)
      .map(() => Math.floor(Math.random() * 256));
    return bytes.map(b => b.toString(16).padStart(2, '0')).join('');
  }

  // Create HTLC
  async createHTLC(
    initiator: string,
    participant: string,
    amount: bigint,
    hashLock: string,
    timeLockHours: number,
    chainId: string
  ): Promise<HTLC> {
    if (amount <= 0n) {
      throw new Error('Amount must be positive');
    }

    if (timeLockHours < this.MIN_TIMELOCK_HOURS) {
      throw new Error(
        `Timelock must be at least ${this.MIN_TIMELOCK_HOURS} hour(s)`
      );
    }

    const timeLock = Math.floor(Date.now() / 1000) + timeLockHours * 3600;

    const htlc: HTLC = {
      id: `htlc_${++this.htlcCounter}`,
      initiator,
      participant,
      amount,
      hashLock,
      timeLock,
      status: SwapStatus.INITIATED,
      chainId,
      createdAt: new Date(),
    };

    this.htlcs.set(htlc.id, htlc);
    return htlc;
  }

  // Fund HTLC
  async fundHTLC(htlcId: string): Promise<HTLC> {
    const htlc = this.htlcs.get(htlcId);
    if (!htlc) {
      throw new Error('HTLC not found');
    }

    if (htlc.status !== SwapStatus.INITIATED) {
      throw new Error('HTLC already funded or completed');
    }

    htlc.status = SwapStatus.FUNDED;
    return htlc;
  }

  // Claim with preimage
  async claimHTLC(htlcId: string, preimage: string): Promise<HTLC> {
    const htlc = this.htlcs.get(htlcId);
    if (!htlc) {
      throw new Error('HTLC not found');
    }

    if (htlc.status !== SwapStatus.FUNDED) {
      throw new Error('HTLC not in funded state');
    }

    // Check timelock hasn't expired
    const currentTime = Math.floor(Date.now() / 1000);
    if (currentTime >= htlc.timeLock) {
      throw new Error('HTLC timelock expired');
    }

    // Verify preimage
    const computedHash = this.generateHashLock(preimage);
    if (computedHash !== htlc.hashLock) {
      throw new Error('Invalid preimage');
    }

    htlc.preimage = preimage;
    htlc.status = SwapStatus.CLAIMED;
    htlc.claimedAt = new Date();

    return htlc;
  }

  // Refund after timelock
  async refundHTLC(htlcId: string): Promise<HTLC> {
    const htlc = this.htlcs.get(htlcId);
    if (!htlc) {
      throw new Error('HTLC not found');
    }

    if (htlc.status === SwapStatus.CLAIMED) {
      throw new Error('HTLC already claimed');
    }

    if (htlc.status === SwapStatus.REFUNDED) {
      throw new Error('HTLC already refunded');
    }

    // Check timelock has expired
    const currentTime = Math.floor(Date.now() / 1000);
    if (currentTime < htlc.timeLock) {
      throw new Error('HTLC timelock not yet expired');
    }

    htlc.status = SwapStatus.REFUNDED;
    htlc.refundedAt = new Date();

    return htlc;
  }

  // Initiate atomic swap
  async initiateAtomicSwap(
    initiator: string,
    participant: string,
    initiatorAmount: bigint,
    participantAmount: bigint,
    initiatorChain: string,
    participantChain: string
  ): Promise<AtomicSwap> {
    // Generate preimage and hash
    const preimage = this.generatePreimage();
    const hashLock = this.generateHashLock(preimage);

    // Create initiator's HTLC (longer timelock)
    const initiatorHTLC = await this.createHTLC(
      initiator,
      participant,
      initiatorAmount,
      hashLock,
      this.DEFAULT_TIMELOCK_HOURS,
      initiatorChain
    );

    // Create participant's HTLC (shorter timelock)
    const participantHTLC = await this.createHTLC(
      participant,
      initiator,
      participantAmount,
      hashLock,
      this.DEFAULT_TIMELOCK_HOURS / 2, // Half the timelock
      participantChain
    );

    const swap: AtomicSwap = {
      id: `swap_${++this.swapCounter}`,
      initiatorHTLC,
      participantHTLC,
      status: 'PENDING',
      createdAt: new Date(),
    };

    // Store preimage securely (initiator keeps this secret initially)
    (swap as any)._preimage = preimage;

    this.swaps.set(swap.id, swap);
    return swap;
  }

  // Get HTLC by ID
  getHTLC(htlcId: string): HTLC | undefined {
    return this.htlcs.get(htlcId);
  }

  // Get swap by ID
  getSwap(swapId: string): AtomicSwap | undefined {
    return this.swaps.get(swapId);
  }

  // Get preimage (for testing)
  getSwapPreimage(swapId: string): string | undefined {
    const swap = this.swaps.get(swapId);
    return swap ? (swap as any)._preimage : undefined;
  }

  // Check if HTLC is expired
  isHTLCExpired(htlcId: string): boolean {
    const htlc = this.htlcs.get(htlcId);
    if (!htlc) return false;
    return Math.floor(Date.now() / 1000) >= htlc.timeLock;
  }

  // Get time remaining on HTLC
  getTimeRemaining(htlcId: string): number {
    const htlc = this.htlcs.get(htlcId);
    if (!htlc) return 0;
    const remaining = htlc.timeLock - Math.floor(Date.now() / 1000);
    return remaining > 0 ? remaining : 0;
  }
}

describe('Atomic Swap / HTLC Tests', () => {
  let htlcService: MockHTLCService;

  beforeEach(() => {
    htlcService = new MockHTLCService();
  });

  describe('Hash Lock Generation', () => {
    it('should generate consistent hash for same preimage', () => {
      const preimage = 'secret_preimage_123';
      const hash1 = htlcService.generateHashLock(preimage);
      const hash2 = htlcService.generateHashLock(preimage);

      expect(hash1).toBe(hash2);
    });

    it('should generate different hashes for different preimages', () => {
      const hash1 = htlcService.generateHashLock('preimage1');
      const hash2 = htlcService.generateHashLock('preimage2');

      expect(hash1).not.toBe(hash2);
    });

    it('should generate random preimages', () => {
      const preimage1 = htlcService.generatePreimage();
      const preimage2 = htlcService.generatePreimage();

      expect(preimage1).not.toBe(preimage2);
      expect(preimage1.length).toBe(64); // 32 bytes in hex
    });
  });

  describe('HTLC Creation', () => {
    it('should create HTLC with valid parameters', async () => {
      const hashLock = htlcService.generateHashLock('test_preimage');
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000000000000000000n,
        hashLock,
        24,
        'ethereum'
      );

      expect(htlc.id).toBeDefined();
      expect(htlc.status).toBe(SwapStatus.INITIATED);
      expect(htlc.initiator).toBe('alice');
      expect(htlc.participant).toBe('bob');
    });

    it('should reject HTLC with zero amount', async () => {
      const hashLock = htlcService.generateHashLock('test');

      await expect(
        htlcService.createHTLC('alice', 'bob', 0n, hashLock, 24, 'ethereum')
      ).rejects.toThrow('Amount must be positive');
    });

    it('should reject HTLC with timelock too short', async () => {
      const hashLock = htlcService.generateHashLock('test');

      await expect(
        htlcService.createHTLC('alice', 'bob', 1000n, hashLock, 0.5, 'ethereum')
      ).rejects.toThrow('Timelock must be at least');
    });
  });

  describe('HTLC Funding', () => {
    it('should fund initiated HTLC', async () => {
      const hashLock = htlcService.generateHashLock('test');
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000n,
        hashLock,
        24,
        'ethereum'
      );

      const funded = await htlcService.fundHTLC(htlc.id);

      expect(funded.status).toBe(SwapStatus.FUNDED);
    });

    it('should reject funding already funded HTLC', async () => {
      const hashLock = htlcService.generateHashLock('test');
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000n,
        hashLock,
        24,
        'ethereum'
      );

      await htlcService.fundHTLC(htlc.id);

      await expect(htlcService.fundHTLC(htlc.id)).rejects.toThrow(
        'already funded'
      );
    });
  });

  describe('Preimage Claim', () => {
    it('should claim HTLC with valid preimage', async () => {
      const preimage = 'secret_preimage';
      const hashLock = htlcService.generateHashLock(preimage);
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000n,
        hashLock,
        24,
        'ethereum'
      );

      await htlcService.fundHTLC(htlc.id);
      const claimed = await htlcService.claimHTLC(htlc.id, preimage);

      expect(claimed.status).toBe(SwapStatus.CLAIMED);
      expect(claimed.preimage).toBe(preimage);
      expect(claimed.claimedAt).toBeDefined();
    });

    it('should reject claim with invalid preimage', async () => {
      const preimage = 'correct_preimage';
      const hashLock = htlcService.generateHashLock(preimage);
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000n,
        hashLock,
        24,
        'ethereum'
      );

      await htlcService.fundHTLC(htlc.id);

      await expect(
        htlcService.claimHTLC(htlc.id, 'wrong_preimage')
      ).rejects.toThrow('Invalid preimage');
    });

    it('should reject claim on unfunded HTLC', async () => {
      const preimage = 'secret';
      const hashLock = htlcService.generateHashLock(preimage);
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000n,
        hashLock,
        24,
        'ethereum'
      );

      await expect(htlcService.claimHTLC(htlc.id, preimage)).rejects.toThrow(
        'not in funded state'
      );
    });
  });

  describe('Refund Path', () => {
    it('should refund after timelock expires', async () => {
      const hashLock = htlcService.generateHashLock('test');

      // Create HTLC with very short timelock (1 hour in past for testing)
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000n,
        hashLock,
        1,
        'ethereum'
      );

      await htlcService.fundHTLC(htlc.id);

      // Manually set timelock to past for testing
      const storedHTLC = htlcService.getHTLC(htlc.id)!;
      storedHTLC.timeLock = Math.floor(Date.now() / 1000) - 1;

      const refunded = await htlcService.refundHTLC(htlc.id);

      expect(refunded.status).toBe(SwapStatus.REFUNDED);
      expect(refunded.refundedAt).toBeDefined();
    });

    it('should reject refund before timelock expires', async () => {
      const hashLock = htlcService.generateHashLock('test');
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000n,
        hashLock,
        24,
        'ethereum'
      );

      await htlcService.fundHTLC(htlc.id);

      await expect(htlcService.refundHTLC(htlc.id)).rejects.toThrow(
        'timelock not yet expired'
      );
    });

    it('should reject refund on already claimed HTLC', async () => {
      const preimage = 'secret';
      const hashLock = htlcService.generateHashLock(preimage);
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000n,
        hashLock,
        24,
        'ethereum'
      );

      await htlcService.fundHTLC(htlc.id);
      await htlcService.claimHTLC(htlc.id, preimage);

      // Set timelock to past
      const storedHTLC = htlcService.getHTLC(htlc.id)!;
      storedHTLC.timeLock = Math.floor(Date.now() / 1000) - 1;

      await expect(htlcService.refundHTLC(htlc.id)).rejects.toThrow(
        'already claimed'
      );
    });
  });

  describe('Atomic Swap Coordination', () => {
    it('should initiate atomic swap with two HTLCs', async () => {
      const swap = await htlcService.initiateAtomicSwap(
        'alice',
        'bob',
        1000000000000000000n, // 1 ETH
        500000000n, // 0.5 BTC
        'ethereum',
        'bitcoin'
      );

      expect(swap.id).toBeDefined();
      expect(swap.initiatorHTLC).toBeDefined();
      expect(swap.participantHTLC).toBeDefined();
      expect(swap.status).toBe('PENDING');
    });

    it('should use same hashLock for both HTLCs', async () => {
      const swap = await htlcService.initiateAtomicSwap(
        'alice',
        'bob',
        1000n,
        500n,
        'ethereum',
        'bitcoin'
      );

      expect(swap.initiatorHTLC.hashLock).toBe(swap.participantHTLC.hashLock);
    });

    it('should have longer timelock for initiator', async () => {
      const swap = await htlcService.initiateAtomicSwap(
        'alice',
        'bob',
        1000n,
        500n,
        'ethereum',
        'bitcoin'
      );

      expect(swap.initiatorHTLC.timeLock).toBeGreaterThan(
        swap.participantHTLC.timeLock
      );
    });

    it('should store preimage for swap', async () => {
      const swap = await htlcService.initiateAtomicSwap(
        'alice',
        'bob',
        1000n,
        500n,
        'ethereum',
        'bitcoin'
      );

      const preimage = htlcService.getSwapPreimage(swap.id);
      expect(preimage).toBeDefined();
      expect(preimage?.length).toBe(64);
    });
  });

  describe('Time Utilities', () => {
    it('should report HTLC as not expired when timelock in future', async () => {
      const hashLock = htlcService.generateHashLock('test');
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000n,
        hashLock,
        24,
        'ethereum'
      );

      expect(htlcService.isHTLCExpired(htlc.id)).toBe(false);
    });

    it('should return time remaining on HTLC', async () => {
      const hashLock = htlcService.generateHashLock('test');
      const htlc = await htlcService.createHTLC(
        'alice',
        'bob',
        1000n,
        hashLock,
        1,
        'ethereum'
      );

      const remaining = htlcService.getTimeRemaining(htlc.id);

      // Should be close to 1 hour (3600 seconds)
      expect(remaining).toBeGreaterThan(3500);
      expect(remaining).toBeLessThanOrEqual(3600);
    });
  });
});
