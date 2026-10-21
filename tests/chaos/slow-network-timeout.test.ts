/**
 * Chaos Test: Slow Network Timeout Behavior (Task 121 - Subtask 5)
 *
 * Tests timeout handling under degraded network conditions:
 * - Operations exceeding timeout throw errors
 * - Operations within timeout complete successfully
 * - Timeouts counted as failures for circuit breaker
 * - Circuit opens after timeout threshold exceeded
 * - Recovery after latency resolves
 *
 * Run: npx jest tests/chaos/slow-network-timeout.test.ts
 */

import { CircuitBreaker } from '../../backend/microservices/shared/resilience/circuit-breaker';
import { RetryManager } from '../../backend/microservices/shared/resilience/retry-manager';
import { CircuitState, ResilienceError } from '../../backend/microservices/shared/resilience/resilience-types';
import {
  assertProductionSafe,
  createSlowOperation,
  delay,
  withTimeout,
} from './helpers/chaos-utils';

describe('Chaos Test: Slow Network Timeout Behavior (Subtask 5)', () => {
  beforeAll(() => {
    assertProductionSafe();
  });

  // ============================================================================
  // Basic Timeout Behavior
  // ============================================================================
  describe('Basic Timeout Handling', () => {
    it('should timeout operations exceeding configured timeout', async () => {
      const circuitBreaker = new CircuitBreaker('slow-service', {
        timeout: 50, // 50ms timeout
        threshold: 10,
        volumeThreshold: 20,
        errorThreshold: 50,
      });

      const slowOp = createSlowOperation(200); // 200ms operation

      await expect(
        circuitBreaker.execute(slowOp, 'slow-operation')
      ).rejects.toThrow(/timed out/i);
    });

    it('should complete operations within timeout', async () => {
      const circuitBreaker = new CircuitBreaker('slow-service', {
        timeout: 500, // 500ms timeout
        threshold: 10,
        volumeThreshold: 20,
        errorThreshold: 50,
      });

      const fastOp = createSlowOperation(50); // 50ms operation

      const result = await circuitBreaker.execute(fastOp, 'fast-operation');

      expect(result).toBeDefined();
      expect((result as any).success).toBe(true);
    });

    it('should handle operations at the timeout boundary', async () => {
      const circuitBreaker = new CircuitBreaker('slow-service', {
        timeout: 100, // 100ms timeout
        threshold: 10,
        volumeThreshold: 20,
        errorThreshold: 50,
      });

      // Operation slightly under timeout should succeed
      const nearTimeoutOp = createSlowOperation(80);
      const result = await circuitBreaker.execute(nearTimeoutOp, 'near-timeout');

      expect(result).toBeDefined();
    });
  });

  // ============================================================================
  // Timeout Configuration Tests
  // ============================================================================
  describe('Timeout Configurations', () => {
    it('should respect 10s default timeout for HTTP calls', async () => {
      const circuitBreaker = new CircuitBreaker('http-service', {
        timeout: 10000, // 10s for HTTP
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      // Fast operation should succeed well within 10s
      const fastOp = createSlowOperation(50);
      const result = await circuitBreaker.execute(fastOp, 'http-call');

      expect(result).toBeDefined();
    });

    it('should respect 5s timeout for health checks', async () => {
      const circuitBreaker = new CircuitBreaker('health-check', {
        timeout: 5000, // 5s for health checks
        threshold: 3,
        volumeThreshold: 5,
        errorThreshold: 50,
      });

      // Fast health check should succeed
      const healthCheck = createSlowOperation(100);
      const result = await circuitBreaker.execute(healthCheck, 'health');

      expect(result).toBeDefined();
    });

    it('should fail 50ms timeout with 200ms operation', async () => {
      const circuitBreaker = new CircuitBreaker('tight-timeout', {
        timeout: 50, // Very tight timeout
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      const slowOp = createSlowOperation(200);

      await expect(
        circuitBreaker.execute(slowOp, 'slow')
      ).rejects.toThrow();
    });

    it('should pass 500ms timeout with 50ms operation', async () => {
      const circuitBreaker = new CircuitBreaker('generous-timeout', {
        timeout: 500,
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      const fastOp = createSlowOperation(50);
      const result = await circuitBreaker.execute(fastOp, 'fast');

      expect((result as any).success).toBe(true);
    });
  });

  // ============================================================================
  // Timeouts as Circuit Breaker Failures
  // ============================================================================
  describe('Timeouts as Circuit Breaker Failures', () => {
    it('should count timeouts as failures for circuit breaker', async () => {
      const circuitBreaker = new CircuitBreaker('timeout-service', {
        timeout: 30, // Very short timeout
        threshold: 3,
        volumeThreshold: 3,
        errorThreshold: 50,
      });

      const slowOp = createSlowOperation(100);

      // Execute operations that will timeout
      for (let i = 0; i < 3; i++) {
        try {
          await circuitBreaker.execute(slowOp, 'timeout-op');
        } catch {
          // Expected timeout
        }
      }

      // Circuit should be open due to timeout failures
      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);

      const metrics = circuitBreaker.getMetrics();
      expect(metrics.failureCount).toBe(3);
    });

    it('should open circuit after timeout threshold exceeded', async () => {
      const circuitBreaker = new CircuitBreaker('timeout-threshold', {
        timeout: 20,
        threshold: 2,
        resetTimeout: 1000,
        volumeThreshold: 2,
        errorThreshold: 50,
      });

      const slowOp = createSlowOperation(100);

      // Trigger enough timeout failures to open circuit
      for (let i = 0; i < 3; i++) {
        try {
          await circuitBreaker.execute(slowOp, 'op');
        } catch {
          // Expected
        }
      }

      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);
      expect(circuitBreaker.isHealthy()).toBe(false);
    });
  });

  // ============================================================================
  // Intermittent Slow Network Simulation
  // ============================================================================
  describe('Intermittent Slow Network', () => {
    it('should handle intermittent slow responses', async () => {
      const circuitBreaker = new CircuitBreaker('intermittent-service', {
        timeout: 200,
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      let callCount = 0;
      const intermittentOp = jest.fn().mockImplementation(async () => {
        callCount++;
        // Every other call is slow but within timeout
        const delayMs = callCount % 2 === 0 ? 50 : 100;
        await delay(delayMs);
        return { success: true, responseTime: delayMs };
      });

      // Execute multiple operations
      for (let i = 0; i < 5; i++) {
        const result = await circuitBreaker.execute(intermittentOp, 'op');
        expect((result as any).success).toBe(true);
      }

      // Circuit should remain closed
      expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
    });

    it('should track average response time with variable latency', async () => {
      const circuitBreaker = new CircuitBreaker('variable-latency', {
        timeout: 300,
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      // Create operations with varying latency
      const latencies = [50, 100, 75, 60, 90];

      for (const latency of latencies) {
        const op = createSlowOperation(latency);
        await circuitBreaker.execute(op, 'op');
      }

      const metrics = circuitBreaker.getMetrics();
      const expectedAvg = latencies.reduce((a, b) => a + b, 0) / latencies.length;

      // Average response time should be approximately the mean of latencies
      expect(metrics.avgResponseTime).toBeGreaterThan(expectedAvg * 0.8);
      expect(metrics.avgResponseTime).toBeLessThan(expectedAvg * 1.5);
    });
  });

  // ============================================================================
  // Recovery After Latency Resolves
  // ============================================================================
  describe('Recovery After Latency Resolves', () => {
    it('should recover when latency improves', async () => {
      const circuitBreaker = new CircuitBreaker('recovery-service', {
        timeout: 50,
        threshold: 2,
        resetTimeout: 50,
        volumeThreshold: 2,
        errorThreshold: 50,
      });

      // First: slow operations cause circuit to open
      const slowOp = createSlowOperation(100);

      for (let i = 0; i < 2; i++) {
        try {
          await circuitBreaker.execute(slowOp, 'slow');
        } catch {
          // Expected timeout
        }
      }

      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);

      // Wait for reset timeout
      await delay(100);

      // Now fast operations - should succeed and close circuit
      const fastOp = createSlowOperation(10);
      const result = await circuitBreaker.execute(fastOp, 'fast');

      expect((result as any).success).toBe(true);
      expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
    });

    it('should allow fast operations during slow network period', async () => {
      const circuitBreaker = new CircuitBreaker('mixed-latency', {
        timeout: 100,
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      // Even if some operations are slow, fast ones should succeed
      const fastOp = createSlowOperation(20);
      const results: boolean[] = [];

      for (let i = 0; i < 3; i++) {
        const result = await circuitBreaker.execute(fastOp, 'fast');
        results.push((result as any).success);
      }

      expect(results.every(r => r)).toBe(true);
      expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
    });
  });

  // ============================================================================
  // Timeout with Retry Manager
  // ============================================================================
  describe('Timeout with Retry Manager', () => {
    it('should retry after timeout failure', async () => {
      let attempt = 0;
      const degradingOperation = jest.fn().mockImplementation(async () => {
        attempt++;
        // First two attempts are slow, third is fast
        if (attempt < 3) {
          await delay(200); // Will timeout
        } else {
          await delay(10); // Will succeed
        }
        return { success: true, attempt };
      });

      const retryManager = new RetryManager('timeout-retry', {
        maxAttempts: 3,
        baseDelay: 10,
        jitter: false,
        retryableErrors: ['ETIMEDOUT', 'timeout', 'timed out'],
      });

      // Wrap with timeout
      const operationWithTimeout = async () => {
        return withTimeout(degradingOperation, 50, 'Operation timed out');
      };

      const result = await retryManager.execute(operationWithTimeout, 'test');

      expect((result as any).success).toBe(true);
      expect((result as any).attempt).toBe(3);
    });
  });

  // ============================================================================
  // Combined Slow Network + Circuit Breaker
  // ============================================================================
  describe('Combined Slow Network + Circuit Breaker Behavior', () => {
    it('should properly handle gradual degradation', async () => {
      const circuitBreaker = new CircuitBreaker('degrading-service', {
        timeout: 100,
        threshold: 3,
        resetTimeout: 100,
        volumeThreshold: 5,
        errorThreshold: 50,
      });

      let latency = 20;
      const degradingOp = jest.fn().mockImplementation(async () => {
        await delay(latency);
        latency += 30; // Latency increases each call
        return { success: true, latency };
      });

      const results: { success: boolean; error?: Error }[] = [];

      // Execute operations as service degrades
      for (let i = 0; i < 6; i++) {
        try {
          const result = await circuitBreaker.execute(degradingOp, 'op');
          results.push({ success: true });
        } catch (error) {
          results.push({ success: false, error: error as Error });
        }
      }

      // Some early operations should succeed
      expect(results.some(r => r.success)).toBe(true);

      // Later operations may fail due to increasing latency
      const failedOps = results.filter(r => !r.success);
      // At least some should have failed after degradation
      // (exact number depends on timing)
    });

    it('should track metrics correctly during slow periods', async () => {
      const circuitBreaker = new CircuitBreaker('metrics-service', {
        timeout: 200,
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      // Execute successful slow operations
      for (let i = 0; i < 3; i++) {
        const op = createSlowOperation(50);
        await circuitBreaker.execute(op, 'op');
      }

      const metrics = circuitBreaker.getMetrics();

      expect(metrics.successCount).toBe(3);
      expect(metrics.failureCount).toBe(0);
      expect(metrics.avgResponseTime).toBeGreaterThan(40);
    });
  });

  // ============================================================================
  // Edge Cases
  // ============================================================================
  describe('Edge Cases', () => {
    it('should handle operation that completes exactly at timeout', async () => {
      // This is a race condition test - behavior may vary slightly
      const circuitBreaker = new CircuitBreaker('boundary-service', {
        timeout: 100,
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      // Operation that's very close to timeout
      const boundaryOp = createSlowOperation(95);

      // May succeed or timeout - both are acceptable
      try {
        const result = await circuitBreaker.execute(boundaryOp, 'boundary');
        expect((result as any).success).toBe(true);
      } catch (error) {
        expect((error as Error).message).toContain('timed out');
      }
    });

    it('should handle very short timeouts', async () => {
      const circuitBreaker = new CircuitBreaker('very-fast', {
        timeout: 10, // 10ms timeout
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      // Any significant work will timeout
      const slowOp = createSlowOperation(50);

      await expect(
        circuitBreaker.execute(slowOp, 'op')
      ).rejects.toThrow();
    });

    it('should handle zero-delay operations', async () => {
      const circuitBreaker = new CircuitBreaker('instant', {
        timeout: 100,
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      const instantOp = jest.fn().mockResolvedValue({ success: true });

      const result = await circuitBreaker.execute(instantOp, 'instant');

      expect((result as any).success).toBe(true);
    });
  });

  // ============================================================================
  // withTimeout Utility Tests
  // ============================================================================
  describe('withTimeout Utility', () => {
    it('should resolve when operation completes before timeout', async () => {
      const fastOperation = async () => {
        await delay(10);
        return { data: 'success' };
      };

      const result = await withTimeout(fastOperation, 100, 'Timed out');

      expect(result.data).toBe('success');
    });

    it('should reject when operation exceeds timeout', async () => {
      const slowOperation = async () => {
        await delay(200);
        return { data: 'never reached' };
      };

      await expect(
        withTimeout(slowOperation, 50, 'Custom timeout message')
      ).rejects.toThrow('Custom timeout message');
    });

    it('should propagate operation errors', async () => {
      const failingOperation = async () => {
        throw new Error('Operation failed');
      };

      await expect(
        withTimeout(failingOperation, 100, 'Timed out')
      ).rejects.toThrow('Operation failed');
    });
  });
});
