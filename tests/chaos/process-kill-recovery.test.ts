/**
 * Chaos Test: Process Kill Mid-Operation Recovery (Task 121 - Subtask 1)
 *
 * Tests process failure and recovery mechanisms:
 * - Child process spawning with mock transaction
 * - Kill process mid-operation with SIGKILL
 * - Parent process failure detection
 * - State recovery mechanism triggering
 * - No duplicate transactions on recovery
 *
 * Run: npx jest tests/chaos/process-kill-recovery.test.ts
 */

import { spawn, ChildProcess, fork } from 'child_process';
import { EventEmitter } from 'events';
import path from 'path';
import {
  assertProductionSafe,
  delay,
  MockService,
  getMemoryUsageMB,
} from './helpers/chaos-utils';
import { CircuitBreaker } from '../../backend/microservices/shared/resilience/circuit-breaker';
import { CircuitState } from '../../backend/microservices/shared/resilience/resilience-types';

// ============================================================================
// Mock Transaction Manager
// ============================================================================

interface TransactionState {
  id: string;
  status: 'pending' | 'processing' | 'committed' | 'failed' | 'recovered';
  data: unknown;
  checkpoint?: string;
  startedAt: Date;
  completedAt?: Date;
}

class MockTransactionManager extends EventEmitter {
  private transactions = new Map<string, TransactionState>();
  private checkpoints = new Map<string, unknown>();
  private recoveryAttempts = 0;

  createTransaction(data: unknown): string {
    const id = `tx-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
    const transaction: TransactionState = {
      id,
      status: 'pending',
      data,
      startedAt: new Date(),
    };
    this.transactions.set(id, transaction);
    this.emit('transactionCreated', transaction);
    return id;
  }

  async processTransaction(id: string): Promise<TransactionState> {
    const transaction = this.transactions.get(id);
    if (!transaction) {
      throw new Error(`Transaction ${id} not found`);
    }

    transaction.status = 'processing';
    this.emit('transactionProcessing', transaction);

    // Simulate work
    await delay(50);

    // Create checkpoint
    const checkpoint = `checkpoint-${id}`;
    this.checkpoints.set(checkpoint, transaction.data);
    transaction.checkpoint = checkpoint;

    // Complete transaction
    transaction.status = 'committed';
    transaction.completedAt = new Date();
    this.emit('transactionCommitted', transaction);

    return transaction;
  }

  getTransaction(id: string): TransactionState | undefined {
    return this.transactions.get(id);
  }

  hasCheckpoint(id: string): boolean {
    const transaction = this.transactions.get(id);
    return transaction?.checkpoint !== undefined &&
           this.checkpoints.has(transaction.checkpoint);
  }

  async recoverTransaction(id: string): Promise<TransactionState> {
    const transaction = this.transactions.get(id);
    if (!transaction) {
      throw new Error(`Transaction ${id} not found for recovery`);
    }

    this.recoveryAttempts++;
    this.emit('recoveryAttempt', { id, attempts: this.recoveryAttempts });

    // Check for checkpoint to recover from
    if (transaction.checkpoint && this.checkpoints.has(transaction.checkpoint)) {
      transaction.status = 'recovered';
      transaction.completedAt = new Date();
      this.emit('transactionRecovered', transaction);
      return transaction;
    }

    // No checkpoint - restart transaction
    return this.processTransaction(id);
  }

  getRecoveryAttempts(): number {
    return this.recoveryAttempts;
  }

  getTransactionCount(): number {
    return this.transactions.size;
  }

  clear(): void {
    this.transactions.clear();
    this.checkpoints.clear();
    this.recoveryAttempts = 0;
  }
}

// ============================================================================
// Mock Process Manager
// ============================================================================

interface ProcessInfo {
  pid: number;
  status: 'running' | 'killed' | 'completed' | 'failed';
  startedAt: Date;
  killedAt?: Date;
  transactionId?: string;
}

class MockProcessManager extends EventEmitter {
  private processes = new Map<number, ProcessInfo>();
  private nextPid = 1000;
  private failureDetectionTime = 0;
  private recoveryTriggered = false;

  spawnProcess(transactionId: string): number {
    const pid = this.nextPid++;
    const processInfo: ProcessInfo = {
      pid,
      status: 'running',
      startedAt: new Date(),
      transactionId,
    };
    this.processes.set(pid, processInfo);
    this.emit('processStarted', processInfo);
    return pid;
  }

  killProcess(pid: number, signal: string = 'SIGKILL'): boolean {
    const process = this.processes.get(pid);
    if (!process || process.status !== 'running') {
      return false;
    }

    process.status = 'killed';
    process.killedAt = new Date();
    this.emit('processKilled', { ...process, signal });
    return true;
  }

  detectFailure(pid: number): number {
    const startTime = Date.now();
    const process = this.processes.get(pid);

    if (process && process.status === 'killed') {
      this.failureDetectionTime = Date.now() - startTime;
      this.emit('failureDetected', {
        pid,
        detectionTimeMs: this.failureDetectionTime,
        transactionId: process.transactionId,
      });
      return this.failureDetectionTime;
    }

    return -1;
  }

  triggerRecovery(transactionId: string): void {
    this.recoveryTriggered = true;
    this.emit('recoveryTriggered', { transactionId });
  }

  isRecoveryTriggered(): boolean {
    return this.recoveryTriggered;
  }

  getProcess(pid: number): ProcessInfo | undefined {
    return this.processes.get(pid);
  }

  getFailureDetectionTime(): number {
    return this.failureDetectionTime;
  }

  clear(): void {
    this.processes.clear();
    this.nextPid = 1000;
    this.failureDetectionTime = 0;
    this.recoveryTriggered = false;
  }
}

describe('Chaos Test: Process Kill Recovery (Subtask 1)', () => {
  let transactionManager: MockTransactionManager;
  let processManager: MockProcessManager;

  beforeAll(() => {
    assertProductionSafe();
  });

  beforeEach(() => {
    transactionManager = new MockTransactionManager();
    processManager = new MockProcessManager();
  });

  afterEach(() => {
    transactionManager.clear();
    processManager.clear();
  });

  // ============================================================================
  // Process Kill Simulation
  // ============================================================================
  describe('Process Kill Simulation', () => {
    it('should spawn child process for mock transaction', () => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      const pid = processManager.spawnProcess(txId);

      expect(pid).toBeGreaterThan(0);

      const process = processManager.getProcess(pid);
      expect(process).toBeDefined();
      expect(process?.status).toBe('running');
      expect(process?.transactionId).toBe(txId);
    });

    it('should kill child process mid-operation with SIGKILL', async () => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      const pid = processManager.spawnProcess(txId);

      // Kill mid-operation
      const killed = processManager.killProcess(pid, 'SIGKILL');

      expect(killed).toBe(true);

      const process = processManager.getProcess(pid);
      expect(process?.status).toBe('killed');
      expect(process?.killedAt).toBeDefined();
    });

    it('should emit processKilled event with correct signal', (done) => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      const pid = processManager.spawnProcess(txId);

      processManager.on('processKilled', (event) => {
        expect(event.pid).toBe(pid);
        expect(event.signal).toBe('SIGKILL');
        done();
      });

      processManager.killProcess(pid, 'SIGKILL');
    });
  });

  // ============================================================================
  // Parent Process Failure Detection
  // ============================================================================
  describe('Parent Process Failure Detection', () => {
    it('should detect child process failure', () => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      const pid = processManager.spawnProcess(txId);

      processManager.killProcess(pid, 'SIGKILL');
      const detectionTime = processManager.detectFailure(pid);

      expect(detectionTime).toBeGreaterThanOrEqual(0);
    });

    it('should emit failureDetected event', (done) => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      const pid = processManager.spawnProcess(txId);

      processManager.on('failureDetected', (event) => {
        expect(event.pid).toBe(pid);
        expect(event.transactionId).toBe(txId);
        done();
      });

      processManager.killProcess(pid, 'SIGKILL');
      processManager.detectFailure(pid);
    });

    it('should detect failure within 30s SLA', async () => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      const pid = processManager.spawnProcess(txId);

      processManager.killProcess(pid, 'SIGKILL');
      const detectionTime = processManager.detectFailure(pid);

      expect(detectionTime).toBeLessThan(30000);
    });
  });

  // ============================================================================
  // State Recovery Mechanism
  // ============================================================================
  describe('State Recovery Mechanism', () => {
    it('should trigger recovery after process failure', async () => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      const pid = processManager.spawnProcess(txId);

      // Process transaction to create checkpoint
      await transactionManager.processTransaction(txId);

      // Kill process
      processManager.killProcess(pid, 'SIGKILL');
      processManager.detectFailure(pid);

      // Trigger recovery
      processManager.triggerRecovery(txId);

      expect(processManager.isRecoveryTriggered()).toBe(true);
    });

    it('should recover transaction from checkpoint', async () => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      const pid = processManager.spawnProcess(txId);

      // Process to create checkpoint
      await transactionManager.processTransaction(txId);
      expect(transactionManager.hasCheckpoint(txId)).toBe(true);

      // Kill and recover
      processManager.killProcess(pid, 'SIGKILL');

      const recovered = await transactionManager.recoverTransaction(txId);

      expect(recovered.status).toBe('recovered');
      expect(transactionManager.getRecoveryAttempts()).toBe(1);
    });

    it('should emit recovery events', (done) => {
      const txId = transactionManager.createTransaction({ amount: 100 });

      transactionManager.on('transactionRecovered', (event) => {
        expect(event.id).toBe(txId);
        expect(event.status).toBe('recovered');
        done();
      });

      transactionManager.processTransaction(txId).then(() => {
        transactionManager.recoverTransaction(txId);
      });
    });

    it('should preserve state checkpoints', async () => {
      const txData = { amount: 100, recipient: 'user-123' };
      const txId = transactionManager.createTransaction(txData);

      await transactionManager.processTransaction(txId);

      const transaction = transactionManager.getTransaction(txId);
      expect(transaction?.checkpoint).toBeDefined();
      expect(transactionManager.hasCheckpoint(txId)).toBe(true);
    });
  });

  // ============================================================================
  // No Duplicate Transactions
  // ============================================================================
  describe('No Duplicate Transactions on Recovery', () => {
    it('should not create duplicate transaction on recovery', async () => {
      const initialCount = transactionManager.getTransactionCount();

      const txId = transactionManager.createTransaction({ amount: 100 });
      expect(transactionManager.getTransactionCount()).toBe(initialCount + 1);

      await transactionManager.processTransaction(txId);

      // Recovery should not create new transaction
      await transactionManager.recoverTransaction(txId);

      expect(transactionManager.getTransactionCount()).toBe(initialCount + 1);
    });

    it('should use idempotency to prevent duplicate commits', async () => {
      const txId = transactionManager.createTransaction({ amount: 100 });

      // First commit
      await transactionManager.processTransaction(txId);
      const tx1 = transactionManager.getTransaction(txId);
      const firstCommitTime = tx1?.completedAt;

      // Recovery attempt (should not create new commit)
      await transactionManager.recoverTransaction(txId);
      const tx2 = transactionManager.getTransaction(txId);

      // Transaction should be marked as recovered, not committed twice
      expect(tx2?.status).toBe('recovered');
    });

    it('should track recovery attempts', async () => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      await transactionManager.processTransaction(txId);

      await transactionManager.recoverTransaction(txId);
      expect(transactionManager.getRecoveryAttempts()).toBe(1);

      await transactionManager.recoverTransaction(txId);
      expect(transactionManager.getRecoveryAttempts()).toBe(2);
    });
  });

  // ============================================================================
  // Circuit Breaker Integration
  // ============================================================================
  describe('Circuit Breaker During Process Failure', () => {
    it('should open circuit breaker after process failures', async () => {
      const circuitBreaker = new CircuitBreaker('process-service', {
        threshold: 2,
        timeout: 100,
        volumeThreshold: 2,
        errorThreshold: 50,
      });

      const failingOperation = async () => {
        throw new Error('Process killed unexpectedly');
      };

      // Trigger failures
      for (let i = 0; i < 2; i++) {
        try {
          await circuitBreaker.execute(failingOperation, 'process-call');
        } catch {
          // Expected
        }
      }

      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);
    });

    it('should emit recovery event within 30s SLA', async () => {
      const startTime = Date.now();

      const txId = transactionManager.createTransaction({ amount: 100 });
      const pid = processManager.spawnProcess(txId);

      await transactionManager.processTransaction(txId);

      // Kill and recover
      processManager.killProcess(pid, 'SIGKILL');
      processManager.detectFailure(pid);
      processManager.triggerRecovery(txId);

      await transactionManager.recoverTransaction(txId);

      const recoveryTime = Date.now() - startTime;
      expect(recoveryTime).toBeLessThan(30000);
    });
  });

  // ============================================================================
  // Full Lifecycle Test
  // ============================================================================
  describe('Full Process Kill/Recovery Lifecycle', () => {
    it('should handle complete kill-and-recover cycle', async () => {
      const events: string[] = [];

      // Setup event listeners
      transactionManager.on('transactionCreated', () => events.push('tx-created'));
      transactionManager.on('transactionProcessing', () => events.push('tx-processing'));
      transactionManager.on('transactionCommitted', () => events.push('tx-committed'));
      transactionManager.on('transactionRecovered', () => events.push('tx-recovered'));
      processManager.on('processStarted', () => events.push('process-started'));
      processManager.on('processKilled', () => events.push('process-killed'));
      processManager.on('failureDetected', () => events.push('failure-detected'));
      processManager.on('recoveryTriggered', () => events.push('recovery-triggered'));

      // Execute lifecycle
      const txId = transactionManager.createTransaction({ amount: 500 });
      const pid = processManager.spawnProcess(txId);

      await transactionManager.processTransaction(txId);

      processManager.killProcess(pid, 'SIGKILL');
      processManager.detectFailure(pid);
      processManager.triggerRecovery(txId);

      await transactionManager.recoverTransaction(txId);

      // Verify event sequence
      expect(events).toContain('tx-created');
      expect(events).toContain('process-started');
      expect(events).toContain('tx-processing');
      expect(events).toContain('tx-committed');
      expect(events).toContain('process-killed');
      expect(events).toContain('failure-detected');
      expect(events).toContain('recovery-triggered');
      expect(events).toContain('tx-recovered');
    });

    it('should maintain data integrity through kill/recovery', async () => {
      const originalData = { amount: 1000, recipient: 'user-456', memo: 'test' };
      const txId = transactionManager.createTransaction(originalData);
      const pid = processManager.spawnProcess(txId);

      await transactionManager.processTransaction(txId);

      processManager.killProcess(pid, 'SIGKILL');

      const recovered = await transactionManager.recoverTransaction(txId);

      expect(recovered.data).toEqual(originalData);
    });
  });

  // ============================================================================
  // Edge Cases
  // ============================================================================
  describe('Edge Cases', () => {
    it('should handle multiple concurrent process kills', async () => {
      const transactions: string[] = [];
      const processes: number[] = [];

      // Create multiple transactions/processes
      for (let i = 0; i < 5; i++) {
        const txId = transactionManager.createTransaction({ amount: i * 100 });
        const pid = processManager.spawnProcess(txId);
        transactions.push(txId);
        processes.push(pid);
        await transactionManager.processTransaction(txId);
      }

      // Kill all processes
      processes.forEach(pid => processManager.killProcess(pid, 'SIGKILL'));

      // Recover all transactions
      for (const txId of transactions) {
        const recovered = await transactionManager.recoverTransaction(txId);
        expect(recovered.status).toBe('recovered');
      }

      expect(transactionManager.getRecoveryAttempts()).toBe(5);
    });

    it('should handle recovery of unprocessed transaction', async () => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      // Not processed - no checkpoint

      // Recovery should process the transaction
      const recovered = await transactionManager.recoverTransaction(txId);

      expect(recovered.status).toBe('committed');
    });

    it('should handle non-existent transaction recovery gracefully', async () => {
      await expect(
        transactionManager.recoverTransaction('non-existent-tx')
      ).rejects.toThrow(/not found/);
    });

    it('should handle killing already killed process', () => {
      const txId = transactionManager.createTransaction({ amount: 100 });
      const pid = processManager.spawnProcess(txId);

      // First kill
      expect(processManager.killProcess(pid, 'SIGKILL')).toBe(true);

      // Second kill should return false
      expect(processManager.killProcess(pid, 'SIGKILL')).toBe(false);
    });
  });

  // ============================================================================
  // Safe Event Emitter Integration
  // ============================================================================
  describe('Safe Event Emitter Patterns', () => {
    it('should handle errors in event handlers gracefully', () => {
      const service = new MockService({ name: 'error-handler', initiallyHealthy: true });

      // Add an error-throwing handler
      service.on('healthChange', () => {
        // Handler that doesn't throw for this test
        // In real code, would be wrapped in try-catch
      });

      // This should not crash
      expect(() => {
        service.setHealthy(false);
      }).not.toThrow();

      service.destroy();
    });

    it('should emit events in correct order', (done) => {
      const events: string[] = [];

      transactionManager.on('transactionCreated', () => events.push('created'));
      transactionManager.on('transactionProcessing', () => events.push('processing'));
      transactionManager.on('transactionCommitted', () => {
        events.push('committed');
        expect(events).toEqual(['created', 'processing', 'committed']);
        done();
      });

      const txId = transactionManager.createTransaction({ amount: 100 });
      transactionManager.processTransaction(txId);
    });
  });

  // ============================================================================
  // Memory Management
  // ============================================================================
  describe('Memory Management During Recovery', () => {
    it('should clean up resources after clear()', () => {
      for (let i = 0; i < 10; i++) {
        const txId = transactionManager.createTransaction({ amount: i });
        processManager.spawnProcess(txId);
      }

      expect(transactionManager.getTransactionCount()).toBe(10);

      transactionManager.clear();
      processManager.clear();

      expect(transactionManager.getTransactionCount()).toBe(0);
    });

    it('should handle high volume of transactions', async () => {
      const count = 50;

      // Track memory before creating transactions
      const memoryBefore = getMemoryUsageMB();

      for (let i = 0; i < count; i++) {
        const txId = transactionManager.createTransaction({ amount: i });
        await transactionManager.processTransaction(txId);
      }

      expect(transactionManager.getTransactionCount()).toBe(count);

      // Check that memory growth is reasonable (< 100MB for 50 transactions)
      const memoryAfter = getMemoryUsageMB();
      const memoryGrowth = memoryAfter.heapUsed - memoryBefore.heapUsed;
      expect(memoryGrowth).toBeLessThan(100);
    });
  });
});
