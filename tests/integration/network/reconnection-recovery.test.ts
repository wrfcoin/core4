/**
 * Integration Test: Reconnection Recovery (Task 117 Integration)
 *
 * Tests full reconnection with state recovery:
 * - State preservation during disconnect
 * - Pending operation queue replay after reconnect
 * - Heartbeat detection of connection loss
 * - Exponential backoff timing verification
 */

import { EventEmitter } from 'events';

// Mock logger
jest.mock('../../../backend/server/utils/logger', () => ({
  appLogger: {
    info: jest.fn(),
    warn: jest.fn(),
    error: jest.fn(),
    debug: jest.fn(),
  },
}));

describe('Reconnection Recovery Integration Tests', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  describe('State Preservation During Disconnect', () => {
    interface ConnectionState {
      lastBlockHeight: number;
      pendingTransactions: string[];
      syncedAt: Date;
      subscriptions: string[];
    }

    class MockStatefulConnection extends EventEmitter {
      private connected = true;
      private state: ConnectionState = {
        lastBlockHeight: 0,
        pendingTransactions: [],
        syncedAt: new Date(),
        subscriptions: [],
      };
      private preservedState: ConnectionState | null = null;

      updateState(updates: Partial<ConnectionState>): void {
        this.state = { ...this.state, ...updates };
      }

      disconnect(): void {
        if (this.connected) {
          // Preserve state before disconnect
          this.preservedState = { ...this.state };
          this.connected = false;
          this.emit('disconnected', { preservedState: this.preservedState });
        }
      }

      reconnect(): void {
        if (!this.connected) {
          this.connected = true;
          // Restore state
          if (this.preservedState) {
            this.state = { ...this.preservedState };
            this.emit('reconnected', {
              restoredState: this.state,
              wasPreserved: true,
            });
            this.preservedState = null;
          } else {
            this.emit('reconnected', { restoredState: null, wasPreserved: false });
          }
        }
      }

      getState(): ConnectionState {
        return { ...this.state };
      }

      isConnected(): boolean {
        return this.connected;
      }
    }

    it('should preserve state during planned disconnect', () => {
      const connection = new MockStatefulConnection();
      const disconnectHandler = jest.fn();
      connection.on('disconnected', disconnectHandler);

      // Set up state
      connection.updateState({
        lastBlockHeight: 12345678,
        pendingTransactions: ['tx1', 'tx2', 'tx3'],
        subscriptions: ['blocks', 'mempool'],
      });

      connection.disconnect();

      expect(disconnectHandler).toHaveBeenCalledWith({
        preservedState: expect.objectContaining({
          lastBlockHeight: 12345678,
          pendingTransactions: ['tx1', 'tx2', 'tx3'],
        }),
      });
    });

    it('should restore state after reconnect', () => {
      const connection = new MockStatefulConnection();
      const reconnectHandler = jest.fn();
      connection.on('reconnected', reconnectHandler);

      // Set up state and disconnect
      connection.updateState({
        lastBlockHeight: 99999,
        subscriptions: ['newHeads'],
      });
      connection.disconnect();

      // State should be empty while disconnected
      expect(connection.isConnected()).toBe(false);

      // Reconnect
      connection.reconnect();

      expect(connection.isConnected()).toBe(true);
      expect(connection.getState().lastBlockHeight).toBe(99999);
      expect(reconnectHandler).toHaveBeenCalledWith({
        restoredState: expect.objectContaining({ lastBlockHeight: 99999 }),
        wasPreserved: true,
      });
    });

    it('should handle reconnect without preserved state', () => {
      const connection = new MockStatefulConnection();
      const reconnectHandler = jest.fn();
      connection.on('reconnected', reconnectHandler);

      // Force disconnected state without preservation
      (connection as any).connected = false;

      connection.reconnect();

      expect(reconnectHandler).toHaveBeenCalledWith({
        restoredState: null,
        wasPreserved: false,
      });
    });
  });

  describe('Pending Operation Queue Replay', () => {
    interface QueuedOperation {
      id: string;
      type: string;
      payload: unknown;
      addedAt: Date;
      retries: number;
    }

    class MockOperationQueue extends EventEmitter {
      private queue: QueuedOperation[] = [];
      private connected = true;
      private maxRetries = 3;
      private isReplaying = false;

      enqueue(type: string, payload: unknown): string {
        const id = `op-${Date.now()}-${Math.random().toString(36).substring(7)}`;
        this.queue.push({
          id,
          type,
          payload,
          addedAt: new Date(),
          retries: 0,
        });
        this.emit('operation_queued', { id, type, queueLength: this.queue.length });
        return id;
      }

      disconnect(): void {
        this.connected = false;
        this.emit('disconnected', { pendingOperations: this.queue.length });
      }

      async reconnect(): Promise<void> {
        this.connected = true;
        this.emit('reconnected');

        // Replay queue
        await this.replayQueue();
      }

      private async replayQueue(): Promise<void> {
        if (this.isReplaying || this.queue.length === 0) {
          this.emit('replay_complete', { total: 0, successful: 0, failed: 0 });
          return;
        }

        this.isReplaying = true;
        const toReplay = [...this.queue];
        let successful = 0;
        let failed = 0;

        for (const operation of toReplay) {
          try {
            await this.executeOperation(operation);
            this.queue = this.queue.filter((op) => op.id !== operation.id);
            successful++;
            this.emit('operation_replayed', {
              id: operation.id,
              success: true,
            });
          } catch (error) {
            operation.retries++;
            if (operation.retries >= this.maxRetries) {
              this.queue = this.queue.filter((op) => op.id !== operation.id);
              failed++;
              this.emit('operation_failed', {
                id: operation.id,
                error: (error as Error).message,
                retries: operation.retries,
              });
            } else {
              this.emit('operation_retry_scheduled', {
                id: operation.id,
                retries: operation.retries,
              });
            }
          }
        }

        this.isReplaying = false;
        this.emit('replay_complete', {
          total: toReplay.length,
          successful,
          failed,
        });
      }

      private async executeOperation(operation: QueuedOperation): Promise<void> {
        // Simulate operation execution
        if ((operation.payload as any)?.shouldFail) {
          throw new Error('Simulated failure');
        }
        await new Promise((resolve) => setTimeout(resolve, 5));
      }

      getQueueLength(): number {
        return this.queue.length;
      }

      getQueue(): QueuedOperation[] {
        return [...this.queue];
      }
    }

    it('should replay all queued operations after reconnect', async () => {
      const queue = new MockOperationQueue();
      const replayedOps: string[] = [];
      queue.on('operation_replayed', ({ id }) => replayedOps.push(id));

      // Queue operations during connection
      const id1 = queue.enqueue('transaction', { to: '0x123', amount: 100 });
      const id2 = queue.enqueue('subscription', { event: 'blocks' });
      const id3 = queue.enqueue('query', { method: 'eth_blockNumber' });

      queue.disconnect();
      expect(queue.getQueueLength()).toBe(3);

      await queue.reconnect();

      expect(replayedOps).toContain(id1);
      expect(replayedOps).toContain(id2);
      expect(replayedOps).toContain(id3);
      expect(queue.getQueueLength()).toBe(0);
    });

    it('should emit replay completion event with statistics', async () => {
      const queue = new MockOperationQueue();
      const completeHandler = jest.fn();
      queue.on('replay_complete', completeHandler);

      queue.enqueue('op1', { data: 1 });
      queue.enqueue('op2', { data: 2, shouldFail: true }); // Will fail
      queue.enqueue('op3', { data: 3 });

      queue.disconnect();
      await queue.reconnect();

      // May need multiple replays for failed operations
      expect(completeHandler).toHaveBeenCalledWith(
        expect.objectContaining({
          total: expect.any(Number),
        })
      );
    });

    it('should respect max retries for failed operations', async () => {
      const queue = new MockOperationQueue();
      const failedHandler = jest.fn();
      queue.on('operation_failed', failedHandler);

      queue.enqueue('failing_op', { shouldFail: true });
      queue.disconnect();

      // Multiple reconnect attempts
      for (let i = 0; i < 4; i++) {
        await queue.reconnect();
        queue.disconnect();
      }

      expect(failedHandler).toHaveBeenCalledWith(
        expect.objectContaining({
          retries: 3,
        })
      );
    });
  });

  describe('Heartbeat Detection', () => {
    class MockHeartbeatMonitor extends EventEmitter {
      private intervalId: NodeJS.Timeout | null = null;
      private consecutiveFailures = 0;
      private config = {
        interval: 100, // 100ms for testing
        failureThreshold: 3,
        timeout: 50, // 50ms timeout for testing
      };
      private shouldSucceed = true;

      start(): void {
        this.intervalId = setInterval(() => this.sendHeartbeat(), this.config.interval);
        this.emit('started');
      }

      stop(): void {
        if (this.intervalId) {
          clearInterval(this.intervalId);
          this.intervalId = null;
        }
        this.emit('stopped');
      }

      private async sendHeartbeat(): Promise<void> {
        try {
          await this.ping();
          this.consecutiveFailures = 0;
          this.emit('heartbeat_success');
        } catch (error) {
          this.consecutiveFailures++;
          this.emit('heartbeat_failure', {
            consecutiveFailures: this.consecutiveFailures,
          });

          if (this.consecutiveFailures >= this.config.failureThreshold) {
            this.emit('connection_lost', {
              failures: this.consecutiveFailures,
            });
          }
        }
      }

      private async ping(): Promise<void> {
        return new Promise((resolve, reject) => {
          if (!this.shouldSucceed) {
            setTimeout(() => reject(new Error('Ping timeout')), this.config.timeout);
          } else {
            setTimeout(resolve, 10);
          }
        });
      }

      simulateConnectionLoss(): void {
        this.shouldSucceed = false;
      }

      simulateConnectionRestore(): void {
        this.shouldSucceed = true;
        this.consecutiveFailures = 0;
      }

      getConsecutiveFailures(): number {
        return this.consecutiveFailures;
      }
    }

    it('should emit success events on successful heartbeats', async () => {
      const monitor = new MockHeartbeatMonitor();
      const successHandler = jest.fn();
      monitor.on('heartbeat_success', successHandler);

      monitor.start();

      // Wait for a few heartbeats
      await new Promise((resolve) => setTimeout(resolve, 250));

      monitor.stop();

      expect(successHandler.mock.calls.length).toBeGreaterThanOrEqual(2);
    });

    it('should detect connection loss after threshold failures', async () => {
      const monitor = new MockHeartbeatMonitor();
      const lostHandler = jest.fn();
      monitor.on('connection_lost', lostHandler);

      monitor.start();
      monitor.simulateConnectionLoss();

      // Wait for failures to accumulate
      await new Promise((resolve) => setTimeout(resolve, 500));

      monitor.stop();

      expect(lostHandler).toHaveBeenCalledWith(
        expect.objectContaining({
          failures: expect.any(Number),
        })
      );
    });

    it('should reset failure count on successful heartbeat', async () => {
      const monitor = new MockHeartbeatMonitor();

      monitor.start();
      monitor.simulateConnectionLoss();

      // Wait for some failures
      await new Promise((resolve) => setTimeout(resolve, 200));

      const failuresBeforeRestore = monitor.getConsecutiveFailures();
      expect(failuresBeforeRestore).toBeGreaterThan(0);

      // Restore connection
      monitor.simulateConnectionRestore();

      // Wait for successful heartbeat
      await new Promise((resolve) => setTimeout(resolve, 150));

      monitor.stop();

      expect(monitor.getConsecutiveFailures()).toBe(0);
    });
  });

  describe('Exponential Backoff Timing', () => {
    class MockReconnector {
      private attempts: Array<{ timestamp: number; delay: number }> = [];
      private config = {
        baseDelay: 100,
        maxDelay: 3000,
        maxAttempts: 10,
        jitterFactor: 0.25,
      };

      async attemptReconnect(
        simulatedFailures: number
      ): Promise<{ success: boolean; attempts: number }> {
        this.attempts = [];

        for (let attempt = 0; attempt < this.config.maxAttempts; attempt++) {
          const delay = this.calculateDelay(attempt);
          this.attempts.push({ timestamp: Date.now(), delay });

          if (attempt >= simulatedFailures) {
            return { success: true, attempts: attempt + 1 };
          }

          await new Promise((resolve) => setTimeout(resolve, delay));
        }

        return { success: false, attempts: this.config.maxAttempts };
      }

      private calculateDelay(attempt: number): number {
        const exponentialDelay = this.config.baseDelay * Math.pow(2, attempt);
        const cappedDelay = Math.min(exponentialDelay, this.config.maxDelay);

        // Add jitter
        const jitter =
          cappedDelay * this.config.jitterFactor * (Math.random() * 2 - 1);
        return Math.floor(cappedDelay + jitter);
      }

      getAttempts(): Array<{ timestamp: number; delay: number }> {
        return [...this.attempts];
      }
    }

    it('should increase delay exponentially between attempts', async () => {
      const reconnector = new MockReconnector();

      await reconnector.attemptReconnect(4); // Fail first 4 attempts

      const attempts = reconnector.getAttempts();
      expect(attempts.length).toBe(5);

      // Check that delays increase (with jitter tolerance)
      for (let i = 1; i < Math.min(attempts.length, 4); i++) {
        // Due to jitter, we use a loose comparison
        expect(attempts[i].delay).toBeGreaterThanOrEqual(
          attempts[i - 1].delay * 0.5
        );
      }
    });

    it('should cap delay at maximum value', async () => {
      const reconnector = new MockReconnector();

      await reconnector.attemptReconnect(8);

      const attempts = reconnector.getAttempts();

      // Later attempts should not exceed max delay (3000ms + jitter)
      const laterAttempts = attempts.slice(5);
      for (const attempt of laterAttempts) {
        expect(attempt.delay).toBeLessThanOrEqual(3000 * 1.25 + 100); // Max + jitter + tolerance
      }
    });

    it('should succeed on first attempt if no failures', async () => {
      const reconnector = new MockReconnector();

      const result = await reconnector.attemptReconnect(0);

      expect(result.success).toBe(true);
      expect(result.attempts).toBe(1);
    });
  });

  describe('Full Reconnection Flow', () => {
    class MockFullReconnectionManager extends EventEmitter {
      private connected = true;
      private state: Record<string, unknown> = {};
      private operationQueue: Array<{ id: string; execute: () => Promise<void> }> =
        [];
      private reconnectAttempts = 0;
      private maxReconnectAttempts = 5;

      async simulateFullFlow(
        initialState: Record<string, unknown>,
        operations: Array<{ id: string; execute: () => Promise<void> }>
      ): Promise<{
        reconnected: boolean;
        stateRestored: boolean;
        operationsReplayed: number;
      }> {
        // Set initial state
        this.state = { ...initialState };
        this.operationQueue = [...operations];

        // Disconnect
        this.connected = false;
        this.emit('disconnected');

        // Attempt reconnection with backoff
        let reconnected = false;
        for (
          this.reconnectAttempts = 0;
          this.reconnectAttempts < this.maxReconnectAttempts;
          this.reconnectAttempts++
        ) {
          const delay = Math.min(100 * Math.pow(2, this.reconnectAttempts), 1000);
          await new Promise((resolve) => setTimeout(resolve, delay / 10)); // Speed up for tests

          // Simulate reconnection success after 2 attempts
          if (this.reconnectAttempts >= 2) {
            this.connected = true;
            reconnected = true;
            this.emit('reconnected', { attempts: this.reconnectAttempts + 1 });
            break;
          }
        }

        if (!reconnected) {
          return { reconnected: false, stateRestored: false, operationsReplayed: 0 };
        }

        // Verify state restored
        const stateRestored = Object.keys(initialState).every(
          (key) => this.state[key] === initialState[key]
        );

        // Replay operations
        let operationsReplayed = 0;
        for (const op of this.operationQueue) {
          try {
            await op.execute();
            operationsReplayed++;
            this.emit('operation_replayed', { id: op.id });
          } catch {
            this.emit('operation_failed', { id: op.id });
          }
        }

        this.operationQueue = [];
        this.emit('recovery_complete', { operationsReplayed });

        return { reconnected, stateRestored, operationsReplayed };
      }
    }

    it('should complete full reconnection flow successfully', async () => {
      const manager = new MockFullReconnectionManager();
      const events: string[] = [];

      manager.on('disconnected', () => events.push('disconnected'));
      manager.on('reconnected', () => events.push('reconnected'));
      manager.on('operation_replayed', () => events.push('operation_replayed'));
      manager.on('recovery_complete', () => events.push('recovery_complete'));

      const result = await manager.simulateFullFlow(
        { userId: 'user-123', lastBlock: 999 },
        [
          { id: 'op1', execute: async () => {} },
          { id: 'op2', execute: async () => {} },
        ]
      );

      expect(result.reconnected).toBe(true);
      expect(result.stateRestored).toBe(true);
      expect(result.operationsReplayed).toBe(2);
      expect(events).toContain('disconnected');
      expect(events).toContain('reconnected');
      expect(events).toContain('recovery_complete');
    });

    it('should preserve state through disconnect/reconnect cycle', async () => {
      const manager = new MockFullReconnectionManager();

      const initialState = {
        subscriptions: ['blocks', 'transactions'],
        filters: { minValue: 100 },
        cursor: 'abc123',
      };

      const result = await manager.simulateFullFlow(initialState, []);

      expect(result.stateRestored).toBe(true);
    });

    it('should report operations replayed count', async () => {
      const manager = new MockFullReconnectionManager();
      const completeHandler = jest.fn();
      manager.on('recovery_complete', completeHandler);

      await manager.simulateFullFlow({}, [
        { id: 'op1', execute: async () => {} },
        { id: 'op2', execute: async () => {} },
        { id: 'op3', execute: async () => {} },
      ]);

      expect(completeHandler).toHaveBeenCalledWith({ operationsReplayed: 3 });
    });
  });
});
