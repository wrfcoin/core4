/**
 * Task 117.6: Automatic Reconnection Tests
 *
 * Tests for network reconnection behavior including:
 * - Auto-reconnection after disconnect
 * - Reconnection within SLA (<30s)
 * - State recovery after reconnect
 * - Heartbeat monitoring (5s interval, 3 failures threshold)
 * - Exponential backoff on repeated failures
 * - Partition healing detection
 */

import { EventEmitter } from 'events';

describe('Task 117.6: Automatic Reconnection Tests', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    jest.useFakeTimers();
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  describe('Network Connection Manager', () => {
    // Every manager created in a test is registered here and destroyed in
    // afterEach. Tests that switch to real timers otherwise leak live
    // heartbeat intervals, which keep the jest process alive after the run
    // and hang CI (#2233).
    const activeManagers: MockNetworkConnectionManager[] = [];

    afterEach(() => {
      for (const manager of activeManagers.splice(0)) {
        manager.destroy();
      }
    });

    /**
     * Mock implementation for testing reconnection behavior
     */
    class MockNetworkConnectionManager extends EventEmitter {
      private connected: boolean = true;
      private reconnecting: boolean = false;
      private reconnectAttempts: number = 0;
      private maxReconnectAttempts: number = 10;
      private baseReconnectDelay: number = 1000; // 1 second
      private maxReconnectDelay: number = 30000; // 30 seconds (SLA)
      private heartbeatInterval: number = 5000; // 5 seconds
      private heartbeatFailureThreshold: number = 3;
      private consecutiveHeartbeatFailures: number = 0;
      private heartbeatTimer: NodeJS.Timeout | null = null;
      private reconnectTimer: NodeJS.Timeout | null = null;
      private connectionState: Map<string, unknown> = new Map();
      private pendingOperations: Array<{
        id: string;
        operation: () => Promise<void>;
      }> = [];

      constructor(options?: {
        maxReconnectAttempts?: number;
        baseReconnectDelay?: number;
        maxReconnectDelay?: number;
        heartbeatInterval?: number;
        heartbeatFailureThreshold?: number;
      }) {
        super();
        if (options) {
          this.maxReconnectAttempts =
            options.maxReconnectAttempts ?? this.maxReconnectAttempts;
          this.baseReconnectDelay =
            options.baseReconnectDelay ?? this.baseReconnectDelay;
          this.maxReconnectDelay =
            options.maxReconnectDelay ?? this.maxReconnectDelay;
          this.heartbeatInterval =
            options.heartbeatInterval ?? this.heartbeatInterval;
          this.heartbeatFailureThreshold =
            options.heartbeatFailureThreshold ?? this.heartbeatFailureThreshold;
        }
        activeManagers.push(this);
      }

      isConnected(): boolean {
        return this.connected;
      }

      isReconnecting(): boolean {
        return this.reconnecting;
      }

      getReconnectAttempts(): number {
        return this.reconnectAttempts;
      }

      getConsecutiveHeartbeatFailures(): number {
        return this.consecutiveHeartbeatFailures;
      }

      // Simulate connection loss
      simulateDisconnect(): void {
        if (this.connected) {
          this.connected = false;
          this.emit('disconnect', {
            reason: 'connection_lost',
            timestamp: Date.now(),
          });
          this.startReconnection();
        }
      }

      // Simulate successful connection
      simulateConnect(): void {
        if (!this.connected) {
          this.connected = true;
          this.reconnecting = false;
          this.reconnectAttempts = 0;
          this.consecutiveHeartbeatFailures = 0;
          this.emit('connect', { timestamp: Date.now() });
          this.startHeartbeat();
          this.replayPendingOperations();
        }
      }

      // Start reconnection with exponential backoff
      private startReconnection(): void {
        if (this.reconnecting) return;

        this.reconnecting = true;
        this.stopHeartbeat();
        this.attemptReconnect();
      }

      private attemptReconnect(): void {
        if (this.connected) return;

        this.reconnectAttempts++;

        if (this.reconnectAttempts > this.maxReconnectAttempts) {
          this.reconnecting = false;
          this.emit('reconnect_failed', {
            attempts: this.reconnectAttempts - 1,
            reason: 'max_attempts_exceeded',
          });
          return;
        }

        // Calculate delay with exponential backoff
        const delay = this.calculateReconnectDelay();
        this.emit('reconnect_attempt', {
          attempt: this.reconnectAttempts,
          delay,
          timestamp: Date.now(),
        });

        this.reconnectTimer = setTimeout(() => {
          // Simulate reconnection attempt (can be overridden in tests)
          this.emit('reconnect_try', { attempt: this.reconnectAttempts });
        }, delay);
      }

      calculateReconnectDelay(): number {
        // Exponential backoff: baseDelay * 2^(attempt-1), capped at maxDelay
        const exponentialDelay =
          this.baseReconnectDelay * Math.pow(2, this.reconnectAttempts - 1);
        return Math.min(exponentialDelay, this.maxReconnectDelay);
      }

      // Continue reconnection after failed attempt
      continueReconnection(): void {
        if (this.reconnecting && !this.connected) {
          this.attemptReconnect();
        }
      }

      // Heartbeat monitoring
      startHeartbeat(): void {
        this.stopHeartbeat();
        this.heartbeatTimer = setInterval(() => {
          this.performHeartbeat();
        }, this.heartbeatInterval);
      }

      stopHeartbeat(): void {
        if (this.heartbeatTimer) {
          clearInterval(this.heartbeatTimer);
          this.heartbeatTimer = null;
        }
      }

      private performHeartbeat(): void {
        // Emit heartbeat event - tests can intercept and simulate success/failure
        this.emit('heartbeat', { timestamp: Date.now() });
      }

      // Called when heartbeat succeeds
      heartbeatSuccess(): void {
        this.consecutiveHeartbeatFailures = 0;
        this.emit('heartbeat_success', { timestamp: Date.now() });
      }

      // Called when heartbeat fails
      heartbeatFailure(): void {
        this.consecutiveHeartbeatFailures++;
        this.emit('heartbeat_failure', {
          consecutiveFailures: this.consecutiveHeartbeatFailures,
          threshold: this.heartbeatFailureThreshold,
        });

        if (
          this.consecutiveHeartbeatFailures >= this.heartbeatFailureThreshold
        ) {
          this.simulateDisconnect();
        }
      }

      // State management for recovery
      saveState(key: string, value: unknown): void {
        this.connectionState.set(key, value);
      }

      getState(key: string): unknown {
        return this.connectionState.get(key);
      }

      clearState(): void {
        this.connectionState.clear();
      }

      hasState(): boolean {
        return this.connectionState.size > 0;
      }

      // Pending operations queue
      queueOperation(id: string, operation: () => Promise<void>): void {
        this.pendingOperations.push({ id, operation });
        this.emit('operation_queued', {
          id,
          queueSize: this.pendingOperations.length,
        });
      }

      getPendingOperationsCount(): number {
        return this.pendingOperations.length;
      }

      private async replayPendingOperations(): Promise<void> {
        const operations = [...this.pendingOperations];
        this.pendingOperations = [];

        for (const op of operations) {
          try {
            await op.operation();
            this.emit('operation_replayed', { id: op.id, success: true });
          } catch (error) {
            this.emit('operation_replayed', {
              id: op.id,
              success: false,
              error,
            });
          }
        }

        this.emit('operations_replay_complete', { total: operations.length });
      }

      // Cleanup
      destroy(): void {
        this.stopHeartbeat();
        if (this.reconnectTimer) {
          clearTimeout(this.reconnectTimer);
        }
        this.removeAllListeners();
      }
    }

    describe('Auto-reconnection after disconnect', () => {
      test('should emit disconnect event when connection is lost', () => {
        const manager = new MockNetworkConnectionManager();
        const disconnectHandler = jest.fn();

        manager.on('disconnect', disconnectHandler);
        manager.simulateDisconnect();

        expect(disconnectHandler).toHaveBeenCalledWith(
          expect.objectContaining({
            reason: 'connection_lost',
            timestamp: expect.any(Number),
          })
        );
        expect(manager.isConnected()).toBe(false);
      });

      test('should start reconnection automatically after disconnect', () => {
        const manager = new MockNetworkConnectionManager();
        const reconnectHandler = jest.fn();

        manager.on('reconnect_attempt', reconnectHandler);
        manager.simulateDisconnect();

        expect(manager.isReconnecting()).toBe(true);
        expect(reconnectHandler).toHaveBeenCalledWith(
          expect.objectContaining({
            attempt: 1,
            delay: expect.any(Number),
          })
        );
      });

      test('should emit connect event when reconnection succeeds', () => {
        const manager = new MockNetworkConnectionManager();
        const connectHandler = jest.fn();

        manager.on('connect', connectHandler);
        manager.simulateDisconnect();
        manager.simulateConnect();

        expect(connectHandler).toHaveBeenCalled();
        expect(manager.isConnected()).toBe(true);
        expect(manager.isReconnecting()).toBe(false);
      });

      test('should reset reconnect attempts after successful connection', () => {
        const manager = new MockNetworkConnectionManager();

        manager.simulateDisconnect();
        expect(manager.getReconnectAttempts()).toBe(1);

        manager.continueReconnection();
        expect(manager.getReconnectAttempts()).toBe(2);

        manager.simulateConnect();
        expect(manager.getReconnectAttempts()).toBe(0);
      });
    });

    describe('Reconnection within SLA (<30s)', () => {
      test('should cap reconnection delay at 30 seconds', () => {
        const manager = new MockNetworkConnectionManager({
          baseReconnectDelay: 1000,
          maxReconnectDelay: 30000,
        });

        // Initial delay (before first attempt, reconnectAttempts=0): baseDelay * 2^(-1) = 500
        // Note: Formula is baseDelay * 2^(attempt-1), so at attempt 0 it's baseDelay/2
        expect(manager.calculateReconnectDelay()).toBe(500);

        // Simulate multiple failed attempts
        manager.simulateDisconnect();
        for (let i = 0; i < 10; i++) {
          manager.continueReconnection();
        }

        // Should be capped at 30s even after many attempts
        const delay = manager.calculateReconnectDelay();
        expect(delay).toBeLessThanOrEqual(30000);
      });

      test('should attempt reconnection within SLA timeframe', () => {
        const manager = new MockNetworkConnectionManager({
          baseReconnectDelay: 100,
          maxReconnectDelay: 30000,
        });

        const attempts: number[] = [];
        manager.on('reconnect_attempt', data => {
          attempts.push(data.delay);
        });

        manager.simulateDisconnect();

        // Verify first reconnection attempt happens quickly
        expect(attempts[0]).toBeLessThanOrEqual(30000);
      });

      test('should fail reconnection if max attempts exceeded', () => {
        const manager = new MockNetworkConnectionManager({
          maxReconnectAttempts: 3,
        });

        const failHandler = jest.fn();
        manager.on('reconnect_failed', failHandler);

        manager.simulateDisconnect();
        manager.continueReconnection();
        manager.continueReconnection();
        manager.continueReconnection();

        expect(failHandler).toHaveBeenCalledWith(
          expect.objectContaining({
            attempts: 3,
            reason: 'max_attempts_exceeded',
          })
        );
      });
    });

    describe('State recovery after reconnect', () => {
      test('should preserve state during disconnect', () => {
        const manager = new MockNetworkConnectionManager();

        manager.saveState('lastBlockHeight', 12345);
        manager.saveState('pendingTransactions', ['tx1', 'tx2']);

        manager.simulateDisconnect();

        expect(manager.getState('lastBlockHeight')).toBe(12345);
        expect(manager.getState('pendingTransactions')).toEqual(['tx1', 'tx2']);
      });

      test('should replay pending operations after reconnect', async () => {
        jest.useRealTimers(); // Use real timers for async operations
        const manager = new MockNetworkConnectionManager();
        const replayHandler = jest.fn();
        const completeHandler = jest.fn();

        manager.on('operation_replayed', replayHandler);
        manager.on('operations_replay_complete', completeHandler);

        let op1Executed = false;
        let op2Executed = false;

        manager.simulateDisconnect();
        manager.queueOperation('op1', async () => {
          op1Executed = true;
        });
        manager.queueOperation('op2', async () => {
          op2Executed = true;
        });

        expect(manager.getPendingOperationsCount()).toBe(2);

        manager.simulateConnect();

        // Wait for async operations
        await new Promise(resolve => setTimeout(resolve, 10));

        expect(op1Executed).toBe(true);
        expect(op2Executed).toBe(true);
        expect(manager.getPendingOperationsCount()).toBe(0);
        jest.useFakeTimers(); // Restore fake timers
      });

      test('should emit events for replayed operations', async () => {
        jest.useRealTimers(); // Use real timers for async operations
        const manager = new MockNetworkConnectionManager();
        const replayedEvents: Array<{ id: string; success: boolean }> = [];

        manager.on('operation_replayed', data => {
          replayedEvents.push(data);
        });

        manager.simulateDisconnect();
        manager.queueOperation('success-op', async () => {
          /* success */
        });
        manager.queueOperation('fail-op', async () => {
          throw new Error('Operation failed');
        });

        manager.simulateConnect();
        await new Promise(resolve => setTimeout(resolve, 10));

        expect(replayedEvents).toHaveLength(2);
        expect(replayedEvents[0]).toEqual({ id: 'success-op', success: true });
        expect(replayedEvents[1]).toMatchObject({
          id: 'fail-op',
          success: false,
        });
        jest.useFakeTimers(); // Restore fake timers
      });

      test('should clear state explicitly when requested', () => {
        const manager = new MockNetworkConnectionManager();

        manager.saveState('key1', 'value1');
        manager.saveState('key2', 'value2');

        expect(manager.hasState()).toBe(true);

        manager.clearState();

        expect(manager.hasState()).toBe(false);
        expect(manager.getState('key1')).toBeUndefined();
      });
    });

    describe('Heartbeat monitoring', () => {
      test('should send heartbeat at configured interval (5s default)', () => {
        const manager = new MockNetworkConnectionManager({
          heartbeatInterval: 5000,
        });

        const heartbeatHandler = jest.fn();
        manager.on('heartbeat', heartbeatHandler);

        manager.startHeartbeat();

        // No heartbeat immediately
        expect(heartbeatHandler).not.toHaveBeenCalled();

        // Advance 5 seconds
        jest.advanceTimersByTime(5000);
        expect(heartbeatHandler).toHaveBeenCalledTimes(1);

        // Advance another 5 seconds
        jest.advanceTimersByTime(5000);
        expect(heartbeatHandler).toHaveBeenCalledTimes(2);

        manager.destroy();
      });

      test('should reset failure count on successful heartbeat', () => {
        const manager = new MockNetworkConnectionManager();

        manager.heartbeatFailure();
        manager.heartbeatFailure();
        expect(manager.getConsecutiveHeartbeatFailures()).toBe(2);

        manager.heartbeatSuccess();
        expect(manager.getConsecutiveHeartbeatFailures()).toBe(0);
      });

      test('should trigger disconnect after 3 consecutive failures (default threshold)', () => {
        const manager = new MockNetworkConnectionManager({
          heartbeatFailureThreshold: 3,
        });

        const disconnectHandler = jest.fn();
        manager.on('disconnect', disconnectHandler);

        manager.heartbeatFailure();
        manager.heartbeatFailure();
        expect(disconnectHandler).not.toHaveBeenCalled();

        manager.heartbeatFailure();
        expect(disconnectHandler).toHaveBeenCalled();
        expect(manager.isConnected()).toBe(false);
      });

      test('should stop heartbeat when disconnected', () => {
        const manager = new MockNetworkConnectionManager({
          heartbeatInterval: 1000,
        });

        const heartbeatHandler = jest.fn();
        manager.on('heartbeat', heartbeatHandler);

        manager.startHeartbeat();
        jest.advanceTimersByTime(2000);
        expect(heartbeatHandler).toHaveBeenCalledTimes(2);

        manager.simulateDisconnect();
        heartbeatHandler.mockClear();

        jest.advanceTimersByTime(5000);
        expect(heartbeatHandler).not.toHaveBeenCalled();

        manager.destroy();
      });

      test('should resume heartbeat after reconnection', () => {
        const manager = new MockNetworkConnectionManager({
          heartbeatInterval: 1000,
        });

        const heartbeatHandler = jest.fn();
        manager.on('heartbeat', heartbeatHandler);

        manager.simulateDisconnect();
        manager.simulateConnect();

        jest.advanceTimersByTime(3000);
        expect(heartbeatHandler).toHaveBeenCalledTimes(3);

        manager.destroy();
      });
    });

    describe('Exponential backoff on repeated failures', () => {
      test('should double delay with each attempt', () => {
        const manager = new MockNetworkConnectionManager({
          baseReconnectDelay: 1000,
          maxReconnectDelay: 60000,
        });

        const delays: number[] = [];
        manager.on('reconnect_attempt', data => {
          delays.push(data.delay);
        });

        manager.simulateDisconnect(); // attempt 1
        manager.continueReconnection(); // attempt 2
        manager.continueReconnection(); // attempt 3
        manager.continueReconnection(); // attempt 4

        expect(delays[0]).toBe(1000); // 1s
        expect(delays[1]).toBe(2000); // 2s
        expect(delays[2]).toBe(4000); // 4s
        expect(delays[3]).toBe(8000); // 8s
      });

      test('should cap delay at maximum value', () => {
        const manager = new MockNetworkConnectionManager({
          baseReconnectDelay: 1000,
          maxReconnectDelay: 5000,
        });

        const delays: number[] = [];
        manager.on('reconnect_attempt', data => {
          delays.push(data.delay);
        });

        manager.simulateDisconnect(); // 1s
        manager.continueReconnection(); // 2s
        manager.continueReconnection(); // 4s
        manager.continueReconnection(); // 5s (capped)
        manager.continueReconnection(); // 5s (capped)

        expect(delays[3]).toBe(5000);
        expect(delays[4]).toBe(5000);
      });

      test('should schedule reconnection attempt with calculated delay', () => {
        const manager = new MockNetworkConnectionManager({
          baseReconnectDelay: 100,
          maxReconnectDelay: 1000,
        });

        const tryHandler = jest.fn();
        manager.on('reconnect_try', tryHandler);

        manager.simulateDisconnect();

        // Attempt should not happen immediately
        expect(tryHandler).not.toHaveBeenCalled();

        // Advance timer to first attempt delay
        jest.advanceTimersByTime(100);
        expect(tryHandler).toHaveBeenCalledWith({ attempt: 1 });

        manager.destroy();
      });
    });

    describe('Partition healing detection', () => {
      /**
       * Partition Healer - detects when network partition is resolved
       */
      class PartitionHealer extends EventEmitter {
        private partitioned: boolean = false;
        private healingCheckInterval: number = 2000;
        private healingTimer: NodeJS.Timeout | null = null;
        private partitionStartTime: number = 0;
        private partitionDuration: number = 0;

        isPartitioned(): boolean {
          return this.partitioned;
        }

        getPartitionDuration(): number {
          return this.partitionDuration;
        }

        detectPartition(): void {
          if (!this.partitioned) {
            this.partitioned = true;
            this.partitionStartTime = Date.now();
            this.emit('partition_detected', {
              timestamp: this.partitionStartTime,
            });
            this.startHealingCheck();
          }
        }

        private startHealingCheck(): void {
          this.healingTimer = setInterval(() => {
            this.emit('healing_check', { timestamp: Date.now() });
          }, this.healingCheckInterval);
        }

        // Called when connectivity is restored
        confirmHealing(): void {
          if (this.partitioned) {
            this.partitioned = false;
            this.partitionDuration = Date.now() - this.partitionStartTime;

            if (this.healingTimer) {
              clearInterval(this.healingTimer);
              this.healingTimer = null;
            }

            this.emit('partition_healed', {
              duration: this.partitionDuration,
              timestamp: Date.now(),
            });
          }
        }

        destroy(): void {
          if (this.healingTimer) {
            clearInterval(this.healingTimer);
          }
          this.removeAllListeners();
        }
      }

      test('should detect network partition', () => {
        const healer = new PartitionHealer();
        const partitionHandler = jest.fn();

        healer.on('partition_detected', partitionHandler);
        healer.detectPartition();

        expect(healer.isPartitioned()).toBe(true);
        expect(partitionHandler).toHaveBeenCalledWith(
          expect.objectContaining({
            timestamp: expect.any(Number),
          })
        );

        healer.destroy();
      });

      test('should emit healing check events while partitioned', () => {
        const healer = new PartitionHealer();
        const checkHandler = jest.fn();

        healer.on('healing_check', checkHandler);
        healer.detectPartition();

        jest.advanceTimersByTime(6000);

        expect(checkHandler).toHaveBeenCalledTimes(3);

        healer.destroy();
      });

      test('should detect partition healing', () => {
        const healer = new PartitionHealer();
        const healHandler = jest.fn();

        healer.on('partition_healed', healHandler);
        healer.detectPartition();

        jest.advanceTimersByTime(5000);
        healer.confirmHealing();

        expect(healer.isPartitioned()).toBe(false);
        expect(healHandler).toHaveBeenCalledWith(
          expect.objectContaining({
            duration: expect.any(Number),
            timestamp: expect.any(Number),
          })
        );

        healer.destroy();
      });

      test('should track partition duration', () => {
        const healer = new PartitionHealer();

        healer.detectPartition();
        jest.advanceTimersByTime(10000);
        healer.confirmHealing();

        expect(healer.getPartitionDuration()).toBeGreaterThanOrEqual(10000);

        healer.destroy();
      });

      test('should stop healing checks after partition is healed', () => {
        const healer = new PartitionHealer();
        const checkHandler = jest.fn();

        healer.on('healing_check', checkHandler);
        healer.detectPartition();

        jest.advanceTimersByTime(4000);
        expect(checkHandler).toHaveBeenCalledTimes(2);

        healer.confirmHealing();
        checkHandler.mockClear();

        jest.advanceTimersByTime(10000);
        expect(checkHandler).not.toHaveBeenCalled();

        healer.destroy();
      });

      test('should not double-detect partition', () => {
        const healer = new PartitionHealer();
        const partitionHandler = jest.fn();

        healer.on('partition_detected', partitionHandler);

        healer.detectPartition();
        healer.detectPartition();
        healer.detectPartition();

        expect(partitionHandler).toHaveBeenCalledTimes(1);

        healer.destroy();
      });
    });

    describe('Integration with NetworkPartitionDetector', () => {
      // Skipped: imports backend/shared/network-partition-detector, which was
      // extracted to wrfcoin/backend (#373) and no longer exists in this repo.
      // Tracked for porting alongside the other orphaned suites — see the
      // porting issue referenced in tests/jest.config.cjs.
      test.skip('should integrate connection manager with partition detector', async () => {
        // Import actual NetworkPartitionDetector if available
        const { NetworkPartitionDetector } =
          await import('../../../backend/shared/network-partition-detector');

        const detector = new NetworkPartitionDetector({
          heartbeatInterval: 100,
          failureThreshold: 2,
          partitionThreshold: 50,
          validatorWeightFactor: 2,
          detectionWindowMs: 60000,
          severityThresholds: {
            minor: 10,
            moderate: 25,
            severe: 50,
            critical: 75,
          },
        });

        const partitionHandler = jest.fn();
        detector.on('partition_detected', partitionHandler);

        // Start detector - will fail to connect
        detector.start();

        // Advance timers to trigger heartbeat failures
        jest.advanceTimersByTime(300);

        detector.stop();

        // Partition should be detected due to failed probes
        // (depends on actual implementation behavior)
      });
    });

    describe('Edge cases and error handling', () => {
      test('should handle rapid connect/disconnect cycles', () => {
        const manager = new MockNetworkConnectionManager();
        const events: string[] = [];

        manager.on('disconnect', () => events.push('disconnect'));
        manager.on('connect', () => events.push('connect'));
        manager.on('reconnect_attempt', () => events.push('reconnect_attempt'));

        // Rapid cycle
        manager.simulateDisconnect();
        manager.simulateConnect();
        manager.simulateDisconnect();
        manager.simulateConnect();

        expect(events).toEqual([
          'disconnect',
          'reconnect_attempt',
          'connect',
          'disconnect',
          'reconnect_attempt',
          'connect',
        ]);
      });

      test('should handle destroy during reconnection', () => {
        const manager = new MockNetworkConnectionManager();

        manager.simulateDisconnect();
        expect(manager.isReconnecting()).toBe(true);

        // Should not throw
        expect(() => manager.destroy()).not.toThrow();
      });

      test('should handle empty pending operations queue', async () => {
        jest.useRealTimers(); // Use real timers for async operations
        const manager = new MockNetworkConnectionManager();
        const completeHandler = jest.fn();

        manager.on('operations_replay_complete', completeHandler);

        manager.simulateDisconnect();
        // No operations queued
        manager.simulateConnect();

        await new Promise(resolve => setTimeout(resolve, 10));

        expect(completeHandler).toHaveBeenCalledWith({ total: 0 });
        // Under real timers simulateConnect() starts a real heartbeat
        // interval; without destroy() the leaked handle keeps the jest
        // process alive after the run (#2233).
        manager.destroy();
        jest.useFakeTimers(); // Restore fake timers
      });

      test('should handle multiple simultaneous disconnect calls', () => {
        const manager = new MockNetworkConnectionManager();
        const disconnectHandler = jest.fn();

        manager.on('disconnect', disconnectHandler);

        manager.simulateDisconnect();
        manager.simulateDisconnect();
        manager.simulateDisconnect();

        // Should only disconnect once
        expect(disconnectHandler).toHaveBeenCalledTimes(1);
      });

      test('should handle connect when already connected', () => {
        const manager = new MockNetworkConnectionManager();
        const connectHandler = jest.fn();

        manager.on('connect', connectHandler);

        // Already connected by default
        expect(manager.isConnected()).toBe(true);

        // This should be a no-op
        manager.simulateConnect();

        expect(connectHandler).not.toHaveBeenCalled();
      });
    });

    describe('Performance and memory', () => {
      test('should not accumulate event listeners', () => {
        const manager = new MockNetworkConnectionManager();
        const initialListenerCount = manager.listenerCount('heartbeat');

        manager.startHeartbeat();
        manager.stopHeartbeat();
        manager.startHeartbeat();
        manager.stopHeartbeat();
        manager.startHeartbeat();

        // Should not accumulate listeners from timer callbacks
        expect(manager.listenerCount('heartbeat')).toBe(initialListenerCount);

        manager.destroy();
      });

      test('should clean up timers on destroy', () => {
        const manager = new MockNetworkConnectionManager({
          heartbeatInterval: 100,
        });

        const heartbeatHandler = jest.fn();
        manager.on('heartbeat', heartbeatHandler);

        manager.startHeartbeat();
        jest.advanceTimersByTime(200);
        expect(heartbeatHandler).toHaveBeenCalledTimes(2);

        manager.destroy();
        heartbeatHandler.mockClear();

        jest.advanceTimersByTime(500);
        expect(heartbeatHandler).not.toHaveBeenCalled();
      });

      test('should bound pending operations queue implicitly', async () => {
        jest.useRealTimers(); // Use real timers for async operations
        const manager = new MockNetworkConnectionManager();

        manager.simulateDisconnect();

        // Queue many operations
        for (let i = 0; i < 100; i++) {
          manager.queueOperation(`op-${i}`, async () => {});
        }

        expect(manager.getPendingOperationsCount()).toBe(100);

        // Reconnect clears queue
        manager.simulateConnect();
        await new Promise(resolve => setTimeout(resolve, 50));

        expect(manager.getPendingOperationsCount()).toBe(0);
        // See above: destroy() the manager so its real-timer heartbeat
        // interval cannot outlive the test and hang the runner (#2233).
        manager.destroy();
        jest.useFakeTimers(); // Restore fake timers
      });
    });
  });
});
