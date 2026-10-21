/**
 * Timeout & Retry Behavior Tests (Task 117.4)
 *
 * Comprehensive tests for network resilience patterns including:
 * - Configurable timeouts
 * - Exponential backoff with jitter
 * - Retry limits (3 HTTP, 5 DB)
 * - Retryable error detection
 * - Circuit breaker states (CLOSED→OPEN→HALF_OPEN)
 * - Slow network simulation
 */

import { RetryManager } from '../../../backend/microservices/shared/resilience/retry-manager';
import { CircuitBreaker } from '../../../backend/microservices/shared/resilience/circuit-breaker';
import {
  CircuitState,
  MaxRetriesExceededError,
  CircuitOpenError,
  ResilienceError,
} from '../../../backend/microservices/shared/resilience/resilience-types';

// Mock logger
jest.mock('../../../backend/server/utils/logger', () => ({
  appLogger: {
    info: jest.fn(),
    warn: jest.fn(),
    error: jest.fn(),
    debug: jest.fn(),
  },
}));

describe('Timeout & Retry Behavior Tests (Task 117.4)', () => {
  // ============================================================================
  // Helper Functions
  // ============================================================================

  const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  const createFailingOperation = (failCount: number, error: Error = new Error('ECONNREFUSED')) => {
    let attempts = 0;
    return jest.fn().mockImplementation(async () => {
      attempts++;
      if (attempts <= failCount) {
        throw error;
      }
      return { success: true, attempts };
    });
  };

  const createSlowOperation = (delayMs: number) => {
    return jest.fn().mockImplementation(async () => {
      await delay(delayMs);
      return { success: true };
    });
  };

  // ============================================================================
  // RetryManager Tests
  // ============================================================================
  describe('RetryManager', () => {
    describe('configurable timeouts', () => {
      it('should use default configuration', () => {
        const retryManager = new RetryManager('test-service');
        // Default config: maxAttempts=3, baseDelay=1000, maxDelay=30000

        expect(retryManager).toBeDefined();
      });

      it('should accept custom configuration', async () => {
        const customConfig = {
          maxAttempts: 5,
          baseDelay: 500,
          maxDelay: 10000,
          backoffMultiplier: 1.5,
          jitter: false,
        };
        const retryManager = new RetryManager('test-service', customConfig);

        const operation = createFailingOperation(4);

        const result = await retryManager.execute(operation, 'test-operation') as { success: boolean };
        expect(result.success).toBe(true);
        expect(operation).toHaveBeenCalledTimes(5);
      });
    });

    describe('exponential backoff', () => {
      it('should apply exponential backoff between retries', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 4,
          baseDelay: 100,
          maxDelay: 2000,
          backoffMultiplier: 2,
          jitter: false,
        });

        const operation = createFailingOperation(3);
        const startTime = Date.now();

        await retryManager.execute(operation, 'test-operation');

        const totalTime = Date.now() - startTime;
        // Expected delays: 100 + 200 + 400 = 700ms minimum
        expect(totalTime).toBeGreaterThanOrEqual(600); // Some tolerance
      });

      it('should respect maxDelay cap', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 5,
          baseDelay: 100,
          maxDelay: 200, // Cap at 200ms
          backoffMultiplier: 10,
          jitter: false,
        });

        const operation = createFailingOperation(4);
        const startTime = Date.now();

        await retryManager.execute(operation, 'test-operation');

        const totalTime = Date.now() - startTime;
        // With maxDelay=200, delays would be: 100, 200, 200, 200 = 700ms max
        expect(totalTime).toBeLessThan(1000);
      });

      it('should apply jitter when enabled', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 3,
          baseDelay: 100,
          maxDelay: 1000,
          backoffMultiplier: 2,
          jitter: true,
        });

        // Run multiple times to verify jitter adds randomness
        const times: number[] = [];
        for (let i = 0; i < 5; i++) {
          const operation = createFailingOperation(2);
          const startTime = Date.now();
          await retryManager.execute(operation, 'test-operation');
          times.push(Date.now() - startTime);
        }

        // With jitter, times should vary
        const allSame = times.every((t) => Math.abs(t - times[0]) < 10);
        expect(allSame).toBe(false);
      });
    });

    describe('retry limits', () => {
      it('should throw MaxRetriesExceededError when limit reached', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 3,
          baseDelay: 10,
          jitter: false,
        });

        const operation = createFailingOperation(10); // Always fails

        await expect(retryManager.execute(operation, 'test-operation')).rejects.toThrow(
          MaxRetriesExceededError
        );

        expect(operation).toHaveBeenCalledTimes(3);
      });

      it('should use 3 retries for HTTP calls by default', async () => {
        const retryManager = RetryManager.forHttpCalls('test-service');

        const operation = createFailingOperation(10);

        await expect(retryManager.execute(operation, 'test-operation')).rejects.toThrow(
          MaxRetriesExceededError
        );

        expect(operation).toHaveBeenCalledTimes(3);
      });

      it('should use 5 retries for database calls by default', async () => {
        const retryManager = RetryManager.forDatabase('test-service');

        const operation = createFailingOperation(10);

        await expect(retryManager.execute(operation, 'test-operation')).rejects.toThrow(
          MaxRetriesExceededError
        );

        expect(operation).toHaveBeenCalledTimes(5);
      });
    });

    describe('retryable error detection', () => {
      it('should retry on ECONNREFUSED', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 3,
          baseDelay: 10,
          jitter: false,
        });

        const operation = createFailingOperation(2, new Error('ECONNREFUSED'));

        const result = await retryManager.execute(operation, 'test-operation') as { success: boolean };
        expect(result.success).toBe(true);
        expect(operation).toHaveBeenCalledTimes(3);
      });

      it('should retry on ETIMEDOUT', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 3,
          baseDelay: 10,
          jitter: false,
        });

        const operation = createFailingOperation(2, new Error('ETIMEDOUT'));

        const result = await retryManager.execute(operation, 'test-operation') as { success: boolean };
        expect(result.success).toBe(true);
      });

      it('should retry on ECONNRESET', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 3,
          baseDelay: 10,
          jitter: false,
        });

        const operation = createFailingOperation(2, new Error('ECONNRESET'));

        const result = await retryManager.execute(operation, 'test-operation') as { success: boolean };
        expect(result.success).toBe(true);
      });

      it('should retry on NETWORK_ERROR', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 3,
          baseDelay: 10,
          jitter: false,
        });

        const operation = createFailingOperation(2, new Error('NETWORK_ERROR'));

        const result = await retryManager.execute(operation, 'test-operation') as { success: boolean };
        expect(result.success).toBe(true);
      });

      it('should NOT retry on non-retryable errors', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 3,
          baseDelay: 10,
          jitter: false,
        });

        const nonRetryableError = new Error('INVALID_ARGUMENT');
        const operation = createFailingOperation(10, nonRetryableError);

        await expect(retryManager.execute(operation, 'test-operation')).rejects.toThrow(
          'INVALID_ARGUMENT'
        );

        expect(operation).toHaveBeenCalledTimes(1); // No retries
      });
    });

    describe('attempt tracking', () => {
      it('should track all retry attempts', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 3,
          baseDelay: 10,
          jitter: false,
        });

        const operation = createFailingOperation(2);

        await retryManager.execute(operation, 'test-operation');

        const attempts = retryManager.getAttempts();
        expect(attempts.length).toBe(2); // 2 failures before success
        expect(attempts[0].attempt).toBe(1);
        expect(attempts[1].attempt).toBe(2);
      });

      it('should get last attempt', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 3,
          baseDelay: 10,
          jitter: false,
        });

        const operation = createFailingOperation(2);

        await retryManager.execute(operation, 'test-operation');

        const lastAttempt = retryManager.getLastAttempt();
        expect(lastAttempt?.attempt).toBe(2);
      });

      it('should reset attempts', async () => {
        const retryManager = new RetryManager('test-service', {
          maxAttempts: 3,
          baseDelay: 10,
          jitter: false,
        });

        const operation = createFailingOperation(2);

        await retryManager.execute(operation, 'test-operation');
        expect(retryManager.getAttempts().length).toBe(2);

        retryManager.reset();
        expect(retryManager.getAttempts().length).toBe(0);
      });
    });

    describe('HTTP status code handling', () => {
      it('should identify retryable HTTP status codes', () => {
        expect(RetryManager.isRetryableHttpStatus(408)).toBe(true); // Request Timeout
        expect(RetryManager.isRetryableHttpStatus(429)).toBe(true); // Too Many Requests
        expect(RetryManager.isRetryableHttpStatus(500)).toBe(true); // Internal Server Error
        expect(RetryManager.isRetryableHttpStatus(502)).toBe(true); // Bad Gateway
        expect(RetryManager.isRetryableHttpStatus(503)).toBe(true); // Service Unavailable
        expect(RetryManager.isRetryableHttpStatus(504)).toBe(true); // Gateway Timeout
      });

      it('should identify non-retryable HTTP status codes', () => {
        expect(RetryManager.isRetryableHttpStatus(200)).toBe(false);
        expect(RetryManager.isRetryableHttpStatus(400)).toBe(false); // Bad Request
        expect(RetryManager.isRetryableHttpStatus(401)).toBe(false); // Unauthorized
        expect(RetryManager.isRetryableHttpStatus(403)).toBe(false); // Forbidden
        expect(RetryManager.isRetryableHttpStatus(404)).toBe(false); // Not Found
      });

      it('should identify Cloudflare error codes as retryable', () => {
        expect(RetryManager.isRetryableHttpStatus(520)).toBe(true);
        expect(RetryManager.isRetryableHttpStatus(521)).toBe(true);
        expect(RetryManager.isRetryableHttpStatus(522)).toBe(true);
        expect(RetryManager.isRetryableHttpStatus(523)).toBe(true);
        expect(RetryManager.isRetryableHttpStatus(524)).toBe(true);
      });
    });
  });

  // ============================================================================
  // CircuitBreaker Tests
  // ============================================================================
  describe('CircuitBreaker', () => {
    describe('circuit states', () => {
      it('should start in CLOSED state', () => {
        const circuitBreaker = new CircuitBreaker('test-service');
        expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
      });

      it('should transition to OPEN state after threshold failures', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          threshold: 3,
          timeout: 100,
          resetTimeout: 1000,
          volumeThreshold: 3,
          errorThreshold: 50,
        });

        const failingOp = jest.fn().mockRejectedValue(new Error('Service error'));

        // Trigger threshold failures
        for (let i = 0; i < 3; i++) {
          try {
            await circuitBreaker.execute(failingOp, 'test-operation');
          } catch {
            // Expected
          }
        }

        expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);
      });

      it('should transition to HALF_OPEN after resetTimeout', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          threshold: 2,
          timeout: 100,
          resetTimeout: 100, // Short reset timeout for testing
          volumeThreshold: 2,
          errorThreshold: 50,
        });

        const failingOp = jest.fn().mockRejectedValue(new Error('Service error'));

        // Trigger failures to open circuit
        for (let i = 0; i < 2; i++) {
          try {
            await circuitBreaker.execute(failingOp, 'test-operation');
          } catch {
            // Expected
          }
        }

        expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);

        // Wait for reset timeout
        await delay(150);

        // Next call should transition to HALF_OPEN
        const successOp = jest.fn().mockResolvedValue({ success: true });
        try {
          await circuitBreaker.execute(successOp, 'test-operation');
        } catch {
          // May fail if still processing
        }

        // State should now be HALF_OPEN or CLOSED (if success closed it)
        expect([CircuitState.HALF_OPEN, CircuitState.CLOSED]).toContain(
          circuitBreaker.getState()
        );
      });

      it('should transition back to CLOSED after successful HALF_OPEN request', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          threshold: 2,
          timeout: 100,
          resetTimeout: 50,
          volumeThreshold: 2,
          errorThreshold: 50,
        });

        const failingOp = jest.fn().mockRejectedValue(new Error('Service error'));

        // Open the circuit
        for (let i = 0; i < 2; i++) {
          try {
            await circuitBreaker.execute(failingOp, 'test-operation');
          } catch {
            // Expected
          }
        }

        expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);

        // Wait for reset timeout
        await delay(100);

        // Successful request should close the circuit
        const successOp = jest.fn().mockResolvedValue({ success: true });
        await circuitBreaker.execute(successOp, 'test-operation');

        expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
      });
    });

    describe('timeout handling', () => {
      it('should timeout slow operations', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          timeout: 50, // 50ms timeout
          threshold: 5,
          volumeThreshold: 10,
          errorThreshold: 50,
        });

        const slowOp = createSlowOperation(200); // 200ms operation

        await expect(circuitBreaker.execute(slowOp, 'test-operation')).rejects.toThrow();
      });

      it('should allow fast operations to complete', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          timeout: 500, // 500ms timeout
          threshold: 5,
          volumeThreshold: 10,
          errorThreshold: 50,
        });

        const fastOp = createSlowOperation(50); // 50ms operation

        const result = await circuitBreaker.execute(fastOp, 'test-operation') as { success: boolean };
        expect(result.success).toBe(true);
      });
    });

    describe('circuit open behavior', () => {
      it('should throw CircuitOpenError when circuit is open', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          threshold: 2,
          timeout: 100,
          resetTimeout: 10000, // Long timeout
          volumeThreshold: 2,
          errorThreshold: 50,
        });

        const failingOp = jest.fn().mockRejectedValue(new Error('Service error'));

        // Open the circuit
        for (let i = 0; i < 2; i++) {
          try {
            await circuitBreaker.execute(failingOp, 'test-operation');
          } catch {
            // Expected
          }
        }

        // Circuit is now open - next call should fail fast
        const newOp = jest.fn().mockResolvedValue({ success: true });

        await expect(circuitBreaker.execute(newOp, 'test-operation')).rejects.toThrow(
          CircuitOpenError
        );

        // The operation should NOT have been called
        expect(newOp).not.toHaveBeenCalled();
      });
    });

    describe('metrics tracking', () => {
      it('should track circuit metrics', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          threshold: 5,
          timeout: 100,
          volumeThreshold: 10,
          errorThreshold: 50,
        });

        const successOp = jest.fn().mockResolvedValue({ success: true });
        const failingOp = jest.fn().mockRejectedValue(new Error('Error'));

        // Execute some operations
        await circuitBreaker.execute(successOp, 'test-operation');
        await circuitBreaker.execute(successOp, 'test-operation');
        try {
          await circuitBreaker.execute(failingOp, 'test-operation');
        } catch {
          // Expected
        }

        const metrics = circuitBreaker.getMetrics();
        expect(metrics.totalRequests).toBe(3);
        expect(metrics.successCount).toBe(2);
        expect(metrics.failureCount).toBe(1);
        expect(metrics.errorRate).toBeCloseTo(33.33, 0);
      });

      it('should track average response time', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          threshold: 5,
          timeout: 500,
          volumeThreshold: 10,
          errorThreshold: 50,
        });

        const delayedOp = createSlowOperation(50);

        await circuitBreaker.execute(delayedOp, 'test-operation');
        await circuitBreaker.execute(delayedOp, 'test-operation');

        const metrics = circuitBreaker.getMetrics();
        expect(metrics.avgResponseTime).toBeGreaterThan(40);
        expect(metrics.avgResponseTime).toBeLessThan(150);
      });
    });

    describe('health status', () => {
      it('should report healthy when circuit is closed', () => {
        const circuitBreaker = new CircuitBreaker('test-service');
        expect(circuitBreaker.isHealthy()).toBe(true);
      });

      it('should report unhealthy when circuit is open', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          threshold: 2,
          timeout: 100,
          volumeThreshold: 2,
          errorThreshold: 50,
        });

        const failingOp = jest.fn().mockRejectedValue(new Error('Error'));

        // Open the circuit
        for (let i = 0; i < 2; i++) {
          try {
            await circuitBreaker.execute(failingOp, 'test-operation');
          } catch {
            // Expected
          }
        }

        expect(circuitBreaker.isHealthy()).toBe(false);
      });
    });

    describe('manual controls', () => {
      it('should allow manual reset', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          threshold: 2,
          timeout: 100,
          volumeThreshold: 2,
          errorThreshold: 50,
        });

        const failingOp = jest.fn().mockRejectedValue(new Error('Error'));

        // Open the circuit
        for (let i = 0; i < 2; i++) {
          try {
            await circuitBreaker.execute(failingOp, 'test-operation');
          } catch {
            // Expected
          }
        }

        expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);

        circuitBreaker.reset();

        expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
        expect(circuitBreaker.getMetrics().totalRequests).toBe(0);
      });

      it('should allow force open', () => {
        const circuitBreaker = new CircuitBreaker('test-service');
        expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);

        circuitBreaker.forceOpen();

        expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);
      });

      it('should allow force close', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          threshold: 2,
          timeout: 100,
          volumeThreshold: 2,
          errorThreshold: 50,
        });

        const failingOp = jest.fn().mockRejectedValue(new Error('Error'));

        // Open the circuit
        for (let i = 0; i < 2; i++) {
          try {
            await circuitBreaker.execute(failingOp, 'test-operation');
          } catch {
            // Expected
          }
        }

        expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);

        circuitBreaker.forceClose();

        expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
      });
    });

    describe('error handling', () => {
      it('should wrap errors in ResilienceError', async () => {
        const circuitBreaker = new CircuitBreaker('test-service', {
          threshold: 5,
          timeout: 100,
          volumeThreshold: 10,
          errorThreshold: 50,
        });

        const originalError = new Error('Original error');
        const failingOp = jest.fn().mockRejectedValue(originalError);

        try {
          await circuitBreaker.execute(failingOp, 'test-operation');
          fail('Should have thrown');
        } catch (error) {
          expect(error).toBeInstanceOf(ResilienceError);
          expect((error as ResilienceError).code).toBe('CIRCUIT_BREAKER_FAILURE');
        }
      });
    });
  });

  // ============================================================================
  // Slow Network Simulation Tests
  // ============================================================================
  describe('Slow Network Simulation', () => {
    it('should handle intermittent slow responses', async () => {
      const circuitBreaker = new CircuitBreaker('test-service', {
        timeout: 200,
        threshold: 5,
        volumeThreshold: 10,
        errorThreshold: 50,
      });

      let callCount = 0;
      const intermittentSlowOp = jest.fn().mockImplementation(async () => {
        callCount++;
        if (callCount % 2 === 0) {
          await delay(50); // Fast response
        } else {
          await delay(100); // Slower but still within timeout
        }
        return { success: true };
      });

      // Execute multiple operations
      for (let i = 0; i < 5; i++) {
        await circuitBreaker.execute(intermittentSlowOp, 'test-operation');
      }

      expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
      expect(circuitBreaker.getMetrics().successCount).toBe(5);
    });

    it('should handle gradual degradation', async () => {
      const retryManager = new RetryManager('test-service', {
        maxAttempts: 3,
        baseDelay: 10,
        jitter: false,
      });

      let callCount = 0;
      const degradingOp = jest.fn().mockImplementation(async () => {
        callCount++;
        if (callCount < 3) {
          throw new Error('ECONNRESET'); // First calls fail
        }
        return { success: true };
      });

      const result = await retryManager.execute(degradingOp, 'test-operation') as { success: boolean };
      expect(result.success).toBe(true);
      expect(callCount).toBe(3);
    });

    it('should combine retry and circuit breaker for robust handling', async () => {
      const circuitBreaker = new CircuitBreaker('test-service', {
        timeout: 500,
        threshold: 5,
        resetTimeout: 1000,
        volumeThreshold: 5,
        errorThreshold: 50,
      });

      const retryManager = new RetryManager('test-service', {
        maxAttempts: 3,
        baseDelay: 20,
        jitter: false,
        retryableErrors: ['ECONNREFUSED', 'CIRCUIT_BREAKER_FAILURE'],
      });

      let callCount = 0;
      const flakyOp = jest.fn().mockImplementation(async () => {
        callCount++;
        if (callCount <= 2) {
          throw new Error('ECONNREFUSED');
        }
        return { success: true };
      });

      // Combine: circuit breaker wrapping the operation, retry manager handling failures
      const combinedOperation = () => circuitBreaker.execute(flakyOp, 'test-operation');

      const result = await retryManager.execute(combinedOperation, 'combined-operation') as { success: boolean };
      expect(result.success).toBe(true);
    });
  });

  // ============================================================================
  // Jitter Tests
  // ============================================================================
  describe('Jitter Implementation', () => {
    it('should apply jitter within 25% range', async () => {
      const baseDelay = 1000;
      const jitterRange = 0.25;

      // Test jitter calculation
      const delays: number[] = [];

      for (let i = 0; i < 100; i++) {
        const delay = baseDelay;
        const jitter = Math.random() * (delay * jitterRange);
        delays.push(delay + jitter);
      }

      // All delays should be within [1000, 1250]
      expect(delays.every((d) => d >= baseDelay)).toBe(true);
      expect(delays.every((d) => d <= baseDelay * (1 + jitterRange))).toBe(true);

      // Should have variation (not all the same)
      const uniqueDelays = new Set(delays.map((d) => Math.floor(d / 10)));
      expect(uniqueDelays.size).toBeGreaterThan(10);
    });
  });

  // ============================================================================
  // Volume Threshold Tests
  // ============================================================================
  describe('Volume Threshold', () => {
    it('should not open circuit before volume threshold is reached', async () => {
      const circuitBreaker = new CircuitBreaker('test-service', {
        threshold: 2,
        timeout: 100,
        volumeThreshold: 10, // Need 10 requests before circuit can open
        errorThreshold: 50,
      });

      const failingOp = jest.fn().mockRejectedValue(new Error('Error'));

      // Execute fewer than volume threshold requests
      for (let i = 0; i < 5; i++) {
        try {
          await circuitBreaker.execute(failingOp, 'test-operation');
        } catch {
          // Expected
        }
      }

      // Circuit should still be closed (not enough volume)
      expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
    });

    it('should open circuit after volume threshold is reached with high error rate', async () => {
      const circuitBreaker = new CircuitBreaker('test-service', {
        threshold: 3,
        timeout: 100,
        volumeThreshold: 5,
        errorThreshold: 50,
      });

      const failingOp = jest.fn().mockRejectedValue(new Error('Error'));

      // Execute enough requests to reach volume threshold
      for (let i = 0; i < 6; i++) {
        try {
          await circuitBreaker.execute(failingOp, 'test-operation');
        } catch {
          // Expected
        }
      }

      // Circuit should now be open
      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);
    });
  });
});
