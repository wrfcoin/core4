/**
 * Chaos Test: Blockchain RPC Drop/Retry Logic (Task 121 - Subtask 3)
 *
 * Tests RPC endpoint resilience:
 * - Primary endpoint failure with ECONNREFUSED
 * - Exponential backoff retry (1s, 2s, 4s)
 * - Failover to secondary endpoint
 * - Circuit breaker state transitions (CLOSED -> OPEN -> HALF_OPEN -> CLOSED)
 *
 * Run: npx jest tests/chaos/blockchain-rpc-failover.test.ts
 */

import { RetryManager } from '../../backend/microservices/shared/resilience/retry-manager';
import { CircuitBreaker } from '../../backend/microservices/shared/resilience/circuit-breaker';
import {
  CircuitState,
  MaxRetriesExceededError,
  CircuitOpenError,
} from '../../backend/microservices/shared/resilience/resilience-types';
import {
  assertProductionSafe,
  createFailingOperation,
  createSlowOperation,
  delay,
} from './helpers/chaos-utils';

describe('Chaos Test: Blockchain RPC Failover (Subtask 3)', () => {
  beforeAll(() => {
    assertProductionSafe();
  });

  // ============================================================================
  // RPC Endpoint Failure Simulation
  // ============================================================================
  describe('Primary RPC Endpoint Failure', () => {
    it('should handle ECONNREFUSED from primary endpoint', async () => {
      const retryManager = new RetryManager('blockchain-rpc', {
        maxAttempts: 3,
        baseDelay: 10,
        jitter: false,
      });

      const econnRefusedError = new Error('ECONNREFUSED: Connection refused');
      const failingRpc = createFailingOperation(2, econnRefusedError);

      const result = await retryManager.execute(failingRpc, 'rpc-call');

      expect(result).toBeDefined();
      expect((result as any).success).toBe(true);
      expect(failingRpc).toHaveBeenCalledTimes(3);
    });

    it('should handle ETIMEDOUT errors', async () => {
      const retryManager = new RetryManager('blockchain-rpc', {
        maxAttempts: 3,
        baseDelay: 10,
        jitter: false,
      });

      const timeoutError = new Error('ETIMEDOUT: Connection timed out');
      const failingRpc = createFailingOperation(2, timeoutError);

      const result = await retryManager.execute(failingRpc, 'rpc-call');

      expect(result).toBeDefined();
      expect((result as any).success).toBe(true);
    });

    it('should handle ECONNRESET errors', async () => {
      const retryManager = new RetryManager('blockchain-rpc', {
        maxAttempts: 3,
        baseDelay: 10,
        jitter: false,
      });

      const resetError = new Error('ECONNRESET: Connection reset by peer');
      const failingRpc = createFailingOperation(2, resetError);

      const result = await retryManager.execute(failingRpc, 'rpc-call');

      expect(result).toBeDefined();
      expect((result as any).success).toBe(true);
    });
  });

  // ============================================================================
  // Exponential Backoff Verification
  // ============================================================================
  describe('Exponential Backoff (1s, 2s, 4s pattern)', () => {
    it('should apply exponential backoff between retries', async () => {
      const retryManager = new RetryManager('blockchain-rpc', {
        maxAttempts: 4,
        baseDelay: 100, // 100ms base for faster tests
        maxDelay: 1000,
        backoffMultiplier: 2,
        jitter: false,
      });

      const failingRpc = createFailingOperation(3);
      const startTime = Date.now();

      await retryManager.execute(failingRpc, 'rpc-call');

      const totalTime = Date.now() - startTime;
      // Expected: 100 + 200 + 400 = 700ms minimum
      expect(totalTime).toBeGreaterThanOrEqual(600);
      expect(totalTime).toBeLessThan(1500);
    });

    it('should verify exponential delay pattern', async () => {
      const retryManager = new RetryManager('blockchain-rpc', {
        maxAttempts: 4,
        baseDelay: 50,
        maxDelay: 500,
        backoffMultiplier: 2,
        jitter: false,
      });

      const failingRpc = createFailingOperation(3);
      await retryManager.execute(failingRpc, 'rpc-call');

      const attempts = retryManager.getAttempts();
      expect(attempts.length).toBe(3);

      // Delays follow exponential pattern: baseDelay * (backoffMultiplier ^ attemptIndex)
      // First retry (attempts[0]) uses baseDelay: 50
      // Second retry (attempts[1]) uses: 50 * 2 = 100
      // Third retry (attempts[2]) uses: 50 * 4 = 200
      expect(attempts[0].delay).toBe(50);
      expect(attempts[1].delay).toBe(100);
      expect(attempts[2].delay).toBe(200);
    });

    it('should respect maxDelay cap', async () => {
      const retryManager = new RetryManager('blockchain-rpc', {
        maxAttempts: 5,
        baseDelay: 100,
        maxDelay: 150, // Cap at 150ms
        backoffMultiplier: 10, // Aggressive multiplier
        jitter: false,
      });

      const failingRpc = createFailingOperation(4);
      const startTime = Date.now();

      await retryManager.execute(failingRpc, 'rpc-call');

      const totalTime = Date.now() - startTime;
      // With maxDelay cap, delays: 100, 150, 150, 150 = 550ms max
      expect(totalTime).toBeLessThan(800);
    });
  });

  // ============================================================================
  // Retry Limits
  // ============================================================================
  describe('Retry Limits', () => {
    it('should use 3 retry attempts for HTTP (factory default)', async () => {
      const retryManager = RetryManager.forHttpCalls('blockchain-rpc');

      const alwaysFailingRpc = createFailingOperation(100);

      await expect(
        retryManager.execute(alwaysFailingRpc, 'rpc-call')
      ).rejects.toThrow(MaxRetriesExceededError);

      expect(alwaysFailingRpc).toHaveBeenCalledTimes(3);
    });

    it('should throw MaxRetriesExceededError when limit reached', async () => {
      const retryManager = new RetryManager('blockchain-rpc', {
        maxAttempts: 3,
        baseDelay: 10,
        jitter: false,
      });

      const alwaysFailingRpc = createFailingOperation(100);

      try {
        await retryManager.execute(alwaysFailingRpc, 'rpc-call');
        fail('Should have thrown MaxRetriesExceededError');
      } catch (error) {
        expect(error).toBeInstanceOf(MaxRetriesExceededError);
        expect((error as MaxRetriesExceededError).code).toBe('MAX_RETRIES_EXCEEDED');
      }
    });
  });

  // ============================================================================
  // HTTP Status Code Handling
  // ============================================================================
  describe('Retryable HTTP Status Codes', () => {
    it('should identify 503 Service Unavailable as retryable', () => {
      expect(RetryManager.isRetryableHttpStatus(503)).toBe(true);
    });

    it('should identify 502 Bad Gateway as retryable', () => {
      expect(RetryManager.isRetryableHttpStatus(502)).toBe(true);
    });

    it('should identify 504 Gateway Timeout as retryable', () => {
      expect(RetryManager.isRetryableHttpStatus(504)).toBe(true);
    });

    it('should identify 429 Too Many Requests as retryable', () => {
      expect(RetryManager.isRetryableHttpStatus(429)).toBe(true);
    });

    it('should NOT identify 400 Bad Request as retryable', () => {
      expect(RetryManager.isRetryableHttpStatus(400)).toBe(false);
    });

    it('should NOT identify 401 Unauthorized as retryable', () => {
      expect(RetryManager.isRetryableHttpStatus(401)).toBe(false);
    });

    it('should NOT identify 404 Not Found as retryable', () => {
      expect(RetryManager.isRetryableHttpStatus(404)).toBe(false);
    });
  });

  // ============================================================================
  // RPC Failover Simulation
  // ============================================================================
  describe('RPC Endpoint Failover', () => {
    it('should failover to secondary endpoint after primary failures', async () => {
      // Simulate primary and secondary RPC endpoints
      let primaryAttempts = 0;
      let secondaryAttempts = 0;

      const primaryEndpoint = jest.fn().mockImplementation(async () => {
        primaryAttempts++;
        throw new Error('ECONNREFUSED: Primary endpoint down');
      });

      const secondaryEndpoint = jest.fn().mockImplementation(async () => {
        secondaryAttempts++;
        return { success: true, endpoint: 'secondary' };
      });

      // Create RPC client with failover logic
      const executeWithFailover = async () => {
        const retryManager = new RetryManager('rpc-primary', {
          maxAttempts: 2,
          baseDelay: 10,
          jitter: false,
        });

        try {
          return await retryManager.execute(primaryEndpoint, 'primary-call');
        } catch {
          // Failover to secondary
          return await secondaryEndpoint();
        }
      };

      const result = await executeWithFailover();

      expect(result).toEqual({ success: true, endpoint: 'secondary' });
      expect(primaryAttempts).toBe(2); // Tried primary twice
      expect(secondaryAttempts).toBe(1); // Then succeeded on secondary
    });
  });

  // ============================================================================
  // Circuit Breaker State Transitions
  // ============================================================================
  describe('Circuit Breaker State Transitions', () => {
    it('should start in CLOSED state', () => {
      const circuitBreaker = new CircuitBreaker('rpc-service');

      expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
    });

    it('should transition CLOSED -> OPEN after threshold failures', async () => {
      const circuitBreaker = new CircuitBreaker('rpc-service', {
        threshold: 3,
        timeout: 100,
        resetTimeout: 1000,
        volumeThreshold: 3,
        errorThreshold: 50,
      });

      const failingOp = jest.fn().mockRejectedValue(new Error('RPC error'));

      // Trigger threshold failures
      for (let i = 0; i < 3; i++) {
        try {
          await circuitBreaker.execute(failingOp, 'rpc-call');
        } catch {
          // Expected
        }
      }

      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);
    });

    it('should reject requests immediately when circuit is OPEN', async () => {
      const circuitBreaker = new CircuitBreaker('rpc-service', {
        threshold: 2,
        timeout: 100,
        resetTimeout: 10000, // Long timeout
        volumeThreshold: 2,
        errorThreshold: 50,
      });

      const failingOp = jest.fn().mockRejectedValue(new Error('RPC error'));
      const successOp = jest.fn().mockResolvedValue({ success: true });

      // Open the circuit
      for (let i = 0; i < 2; i++) {
        try {
          await circuitBreaker.execute(failingOp, 'rpc-call');
        } catch {
          // Expected
        }
      }

      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);

      // Circuit is OPEN - should fail fast without calling the operation
      await expect(
        circuitBreaker.execute(successOp, 'rpc-call')
      ).rejects.toThrow(CircuitOpenError);

      expect(successOp).not.toHaveBeenCalled();
    });

    it('should transition OPEN -> HALF_OPEN after resetTimeout', async () => {
      const circuitBreaker = new CircuitBreaker('rpc-service', {
        threshold: 2,
        timeout: 100,
        resetTimeout: 50, // Short for testing
        volumeThreshold: 2,
        errorThreshold: 50,
      });

      const failingOp = jest.fn().mockRejectedValue(new Error('RPC error'));

      // Open the circuit
      for (let i = 0; i < 2; i++) {
        try {
          await circuitBreaker.execute(failingOp, 'rpc-call');
        } catch {
          // Expected
        }
      }

      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);

      // Wait for reset timeout
      await delay(100);

      // Next call should trigger HALF_OPEN
      const successOp = jest.fn().mockResolvedValue({ success: true });
      try {
        await circuitBreaker.execute(successOp, 'rpc-call');
      } catch {
        // May fail initially
      }

      // Should be HALF_OPEN or CLOSED (if success closed it)
      expect([CircuitState.HALF_OPEN, CircuitState.CLOSED]).toContain(
        circuitBreaker.getState()
      );
    });

    it('should transition HALF_OPEN -> CLOSED after successful request', async () => {
      const circuitBreaker = new CircuitBreaker('rpc-service', {
        threshold: 2,
        timeout: 500,
        resetTimeout: 50,
        volumeThreshold: 2,
        errorThreshold: 50,
      });

      const failingOp = jest.fn().mockRejectedValue(new Error('RPC error'));

      // Open the circuit
      for (let i = 0; i < 2; i++) {
        try {
          await circuitBreaker.execute(failingOp, 'rpc-call');
        } catch {
          // Expected
        }
      }

      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);

      // Wait for reset timeout
      await delay(100);

      // Successful request should close the circuit
      const successOp = jest.fn().mockResolvedValue({ success: true });
      await circuitBreaker.execute(successOp, 'rpc-call');

      expect(circuitBreaker.getState()).toBe(CircuitState.CLOSED);
    });

    it('should transition HALF_OPEN -> OPEN on failure', async () => {
      const circuitBreaker = new CircuitBreaker('rpc-service', {
        threshold: 2,
        timeout: 500,
        resetTimeout: 50,
        volumeThreshold: 2,
        errorThreshold: 50,
      });

      const failingOp = jest.fn().mockRejectedValue(new Error('RPC error'));

      // Open the circuit
      for (let i = 0; i < 2; i++) {
        try {
          await circuitBreaker.execute(failingOp, 'rpc-call');
        } catch {
          // Expected
        }
      }

      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);

      // Wait for reset timeout
      await delay(100);

      // Failing request in HALF_OPEN should reopen circuit
      try {
        await circuitBreaker.execute(failingOp, 'rpc-call');
      } catch {
        // Expected
      }

      expect(circuitBreaker.getState()).toBe(CircuitState.OPEN);
    });
  });

  // ============================================================================
  // Combined Retry + Circuit Breaker
  // ============================================================================
  describe('Combined Retry and Circuit Breaker', () => {
    it('should integrate retry manager with circuit breaker', async () => {
      const circuitBreaker = new CircuitBreaker('rpc-service', {
        timeout: 500,
        threshold: 5,
        resetTimeout: 1000,
        volumeThreshold: 5,
        errorThreshold: 50,
      });

      const retryManager = new RetryManager('rpc-service', {
        maxAttempts: 3,
        baseDelay: 10,
        jitter: false,
        retryableErrors: ['ECONNREFUSED', 'CIRCUIT_BREAKER_FAILURE'],
      });

      let callCount = 0;
      const flakyRpc = jest.fn().mockImplementation(async () => {
        callCount++;
        if (callCount <= 2) {
          throw new Error('ECONNREFUSED');
        }
        return { success: true, callCount };
      });

      // Circuit breaker wraps the operation, retry handles failures
      const combinedOperation = () => circuitBreaker.execute(flakyRpc, 'rpc-call');

      const result = await retryManager.execute(combinedOperation, 'combined');

      expect((result as any).success).toBe(true);
      expect(callCount).toBeGreaterThanOrEqual(3);
    });
  });

  // ============================================================================
  // Metrics Tracking
  // ============================================================================
  describe('Circuit Breaker Metrics', () => {
    it('should track request metrics correctly', async () => {
      const circuitBreaker = new CircuitBreaker('rpc-service', {
        threshold: 10,
        timeout: 100,
        volumeThreshold: 20,
        errorThreshold: 50,
      });

      const successOp = jest.fn().mockResolvedValue({ success: true });
      const failOp = jest.fn().mockRejectedValue(new Error('failure'));

      // Execute some operations
      await circuitBreaker.execute(successOp, 'op1');
      await circuitBreaker.execute(successOp, 'op2');
      await circuitBreaker.execute(successOp, 'op3');

      try {
        await circuitBreaker.execute(failOp, 'op4');
      } catch {
        // Expected
      }

      const metrics = circuitBreaker.getMetrics();

      expect(metrics.totalRequests).toBe(4);
      expect(metrics.successCount).toBe(3);
      expect(metrics.failureCount).toBe(1);
      expect(metrics.errorRate).toBeCloseTo(25, 0);
    });

    it('should track average response time', async () => {
      const circuitBreaker = new CircuitBreaker('rpc-service', {
        threshold: 10,
        timeout: 500,
        volumeThreshold: 20,
        errorThreshold: 50,
      });

      const slowOp = createSlowOperation(50);

      await circuitBreaker.execute(slowOp, 'op1');
      await circuitBreaker.execute(slowOp, 'op2');

      const metrics = circuitBreaker.getMetrics();

      expect(metrics.avgResponseTime).toBeGreaterThan(40);
      expect(metrics.avgResponseTime).toBeLessThan(150);
    });
  });

  // ============================================================================
  // Health Status
  // ============================================================================
  describe('Service Health Status', () => {
    it('should report healthy when circuit is closed', () => {
      const circuitBreaker = new CircuitBreaker('rpc-service');

      expect(circuitBreaker.isHealthy()).toBe(true);
    });

    it('should report unhealthy when circuit is open', async () => {
      const circuitBreaker = new CircuitBreaker('rpc-service', {
        threshold: 2,
        timeout: 100,
        volumeThreshold: 2,
        errorThreshold: 50,
      });

      const failOp = jest.fn().mockRejectedValue(new Error('error'));

      // Open the circuit
      for (let i = 0; i < 2; i++) {
        try {
          await circuitBreaker.execute(failOp, 'op');
        } catch {
          // Expected
        }
      }

      expect(circuitBreaker.isHealthy()).toBe(false);
    });
  });
});
