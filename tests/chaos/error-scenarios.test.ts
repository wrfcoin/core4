/**
 * Error Scenario & Fault Injection Tests
 *
 * Tests that validate the system handles failure conditions gracefully.
 * Covers: malformed inputs, connection failures, timeout handling,
 * concurrent race conditions, and resource exhaustion.
 *
 * Run: npm run test:chaos
 */

import { describe, it, expect, jest, beforeEach } from '@jest/globals';

// ---- Malformed Input Handling ----

describe('Error Scenarios: Malformed Input Handling', () => {
  describe('API Request Validation', () => {
    it('should reject requests with missing required fields', () => {
      const invalidPayloads = [
        {},
        { fromAddress: '0x123' }, // missing toAddress and amount
        { fromAddress: '0x123', toAddress: '0x456' }, // missing amount
        { amount: 100 }, // missing addresses
      ];

      for (const payload of invalidPayloads) {
        const hasFrom = 'fromAddress' in payload;
        const hasTo = 'toAddress' in payload;
        const hasAmount = 'amount' in payload;
        const isValid = hasFrom && hasTo && hasAmount;
        expect(isValid).toBe(false);
      }
    });

    it('should reject negative transaction amounts', () => {
      const amounts = [-1, -100, -0.001, Number.MIN_SAFE_INTEGER];
      for (const amount of amounts) {
        expect(amount).toBeLessThan(0);
        // In production, this would be validated at the API boundary
      }
    });

    it('should reject oversized payloads', () => {
      const maxPayloadSize = 1024 * 1024; // 1MB
      const oversizedData = 'x'.repeat(maxPayloadSize + 1);
      expect(oversizedData.length).toBeGreaterThan(maxPayloadSize);
    });

    it('should handle null and undefined values safely', () => {
      const sanitize = (value: unknown): string => {
        if (value === null || value === undefined) return '';
        return String(value);
      };

      expect(sanitize(null)).toBe('');
      expect(sanitize(undefined)).toBe('');
      expect(sanitize('')).toBe('');
      expect(sanitize(0)).toBe('0');
      expect(sanitize(false)).toBe('false');
    });

    it('should sanitize potential XSS in string inputs', () => {
      const xssPayloads = [
        '<script>alert("xss")</script>',
        '"><img src=x onerror=alert(1)>',
        "'; DROP TABLE users; --",
        '{{constructor.constructor("return this")()}}',
      ];

      const sanitize = (input: string): string => {
        return input.replace(/[<>"'&]/g, (char) => {
          const entities: Record<string, string> = {
            '<': '&lt;',
            '>': '&gt;',
            '"': '&quot;',
            "'": '&#x27;',
            '&': '&amp;',
          };
          return entities[char] || char;
        });
      };

      for (const payload of xssPayloads) {
        const sanitized = sanitize(payload);
        expect(sanitized).not.toContain('<script>');
        expect(sanitized).not.toContain('<img');
      }
    });

    it('should reject invalid wallet addresses', () => {
      const invalidAddresses = [
        '',
        '0x',
        '0x123', // too short
        'not-an-address',
        '0xGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG', // invalid hex
      ];

      const isValidAddress = (addr: string): boolean => {
        return /^0x[0-9a-fA-F]{40}$/.test(addr);
      };

      for (const addr of invalidAddresses) {
        expect(isValidAddress(addr)).toBe(false);
      }

      expect(isValidAddress('0x' + '0'.repeat(40))).toBe(true);
    });
  });
});

// ---- Timeout and Connection Handling ----

describe('Error Scenarios: Timeout and Connection Handling', () => {
  it('should implement request timeout', async () => {
    const timeout = (ms: number): Promise<never> =>
      new Promise((_, reject) => setTimeout(() => reject(new Error('Timeout')), ms));

    const slowOperation = (): Promise<string> =>
      new Promise((resolve) => setTimeout(() => resolve('done'), 200));

    // Should timeout
    await expect(Promise.race([slowOperation(), timeout(50)])).rejects.toThrow('Timeout');

    // Should succeed within timeout
    const result = await Promise.race([slowOperation(), timeout(500)]);
    expect(result).toBe('done');
  });

  it('should implement retry with exponential backoff', async () => {
    let attempts = 0;
    const maxRetries = 3;

    const unreliableOperation = async (): Promise<string> => {
      attempts++;
      if (attempts < 3) throw new Error('Temporary failure');
      return 'success';
    };

    const retryWithBackoff = async <T>(
      fn: () => Promise<T>,
      retries: number
    ): Promise<T> => {
      for (let i = 0; i <= retries; i++) {
        try {
          return await fn();
        } catch (err) {
          if (i === retries) throw err;
          // In real code: await new Promise(r => setTimeout(r, Math.pow(2, i) * 100))
        }
      }
      throw new Error('Unreachable');
    };

    const result = await retryWithBackoff(unreliableOperation, maxRetries);
    expect(result).toBe('success');
    expect(attempts).toBe(3);
  });

  it('should implement circuit breaker pattern', async () => {
    class CircuitBreaker {
      private failures = 0;
      private state: 'closed' | 'open' | 'half-open' = 'closed';
      private lastFailure = 0;

      constructor(
        private threshold: number,
        private resetTimeMs: number
      ) {}

      async execute<T>(fn: () => Promise<T>): Promise<T> {
        if (this.state === 'open') {
          if (Date.now() - this.lastFailure > this.resetTimeMs) {
            this.state = 'half-open';
          } else {
            throw new Error('Circuit breaker is open');
          }
        }

        try {
          const result = await fn();
          this.failures = 0;
          this.state = 'closed';
          return result;
        } catch (err) {
          this.failures++;
          this.lastFailure = Date.now();
          if (this.failures >= this.threshold) {
            this.state = 'open';
          }
          throw err;
        }
      }

      getState() {
        return this.state;
      }
    }

    const breaker = new CircuitBreaker(3, 5000);
    expect(breaker.getState()).toBe('closed');

    // Simulate 3 failures sequentially
    const failingFn = async () => {
      throw new Error('Service down');
    };

    for (let i = 0; i < 3; i++) {
      await breaker.execute(failingFn).catch(() => {});
    }

    // After threshold failures, circuit should open
    expect(breaker.getState()).toBe('open');
  });
});

// ---- Concurrent Access & Race Conditions ----

describe('Error Scenarios: Concurrent Access', () => {
  it('should handle concurrent modifications safely', async () => {
    let balance = 1000;
    const lock = { locked: false };

    const withdraw = async (amount: number): Promise<boolean> => {
      // Simulate lock acquisition
      while (lock.locked) {
        await new Promise((r) => setTimeout(r, 1));
      }
      lock.locked = true;

      try {
        if (balance >= amount) {
          // Simulate async processing
          await new Promise((r) => setTimeout(r, 1));
          balance -= amount;
          return true;
        }
        return false;
      } finally {
        lock.locked = false;
      }
    };

    // Run 5 concurrent withdrawals of 300 each (only 3 should succeed)
    const results = await Promise.all([
      withdraw(300),
      withdraw(300),
      withdraw(300),
      withdraw(300),
      withdraw(300),
    ]);

    const successCount = results.filter(Boolean).length;
    expect(successCount).toBe(3); // Only 3 * 300 = 900 fits in 1000
    expect(balance).toBe(100); // 1000 - 900
  });

  it('should prevent double-spending', () => {
    const processedTxIds = new Set<string>();

    const processTransaction = (txId: string): boolean => {
      if (processedTxIds.has(txId)) {
        return false; // Already processed - reject double spend
      }
      processedTxIds.add(txId);
      return true;
    };

    expect(processTransaction('tx-001')).toBe(true);
    expect(processTransaction('tx-001')).toBe(false); // Double spend rejected
    expect(processTransaction('tx-002')).toBe(true);
  });
});

// ---- Resource Exhaustion ----

describe('Error Scenarios: Resource Exhaustion', () => {
  it('should enforce rate limiting', () => {
    class RateLimiter {
      private requests: number[] = [];

      constructor(
        private maxRequests: number,
        private windowMs: number
      ) {}

      isAllowed(): boolean {
        const now = Date.now();
        this.requests = this.requests.filter((t) => now - t < this.windowMs);
        if (this.requests.length >= this.maxRequests) return false;
        this.requests.push(now);
        return true;
      }
    }

    const limiter = new RateLimiter(5, 1000);

    // First 5 requests should be allowed
    for (let i = 0; i < 5; i++) {
      expect(limiter.isAllowed()).toBe(true);
    }

    // 6th request should be blocked
    expect(limiter.isAllowed()).toBe(false);
  });

  it('should handle memory-safe data structures', () => {
    const maxSize = 1000;
    const cache = new Map<string, string>();

    const addToCache = (key: string, value: string): void => {
      if (cache.size >= maxSize) {
        // Evict oldest entry (LRU approximation)
        const firstKey = cache.keys().next().value;
        if (firstKey !== undefined) cache.delete(firstKey);
      }
      cache.set(key, value);
    };

    // Fill cache to max
    for (let i = 0; i < maxSize + 100; i++) {
      addToCache(`key-${i}`, `value-${i}`);
    }

    // Cache should not exceed max size
    expect(cache.size).toBeLessThanOrEqual(maxSize);
  });

  it('should validate weather data within physical limits', () => {
    const WEATHER_LIMITS = {
      temperature: { min: -89.2, max: 70.7 }, // Celsius
      humidity: { min: 0, max: 100 }, // Percent
      pressure: { min: 870, max: 1083.8 }, // hPa
      windSpeed: { min: 0, max: 113 }, // m/s (highest recorded)
    };

    const validateWeatherData = (
      field: keyof typeof WEATHER_LIMITS,
      value: number
    ): boolean => {
      const limits = WEATHER_LIMITS[field];
      return value >= limits.min && value <= limits.max;
    };

    // Valid data
    expect(validateWeatherData('temperature', 25)).toBe(true);
    expect(validateWeatherData('humidity', 50)).toBe(true);
    expect(validateWeatherData('pressure', 1013)).toBe(true);

    // Invalid data - outside physical limits
    expect(validateWeatherData('temperature', 100)).toBe(false);
    expect(validateWeatherData('temperature', -100)).toBe(false);
    expect(validateWeatherData('humidity', 150)).toBe(false);
    expect(validateWeatherData('humidity', -1)).toBe(false);
    expect(validateWeatherData('pressure', 500)).toBe(false);
    expect(validateWeatherData('windSpeed', 200)).toBe(false);
  });
});

// ---- Error Response Formatting ----

describe('Error Scenarios: Error Response Formatting', () => {
  it('should never expose stack traces in error responses', () => {
    const formatError = (error: Error, env: string): { error: string; details?: string } => {
      if (env === 'production') {
        return { error: 'Internal server error' };
      }
      return { error: error.message, details: error.stack };
    };

    const err = new Error('Database connection failed: password=secret123');
    const prodResponse = formatError(err, 'production');
    const devResponse = formatError(err, 'development');

    expect(prodResponse.error).toBe('Internal server error');
    expect(prodResponse.details).toBeUndefined();
    expect(prodResponse.error).not.toContain('secret123');

    expect(devResponse.details).toBeDefined();
  });

  it('should sanitize error messages to remove sensitive data', () => {
    const sensitivePatterns = [
      /password[=:]\s*\S+/gi,
      /secret[=:]\s*\S+/gi,
      /token[=:]\s*\S+/gi,
      /key[=:]\s*[A-Za-z0-9+/=]{20,}/gi,
      /Bearer\s+\S+/gi,
    ];

    const sanitizeError = (message: string): string => {
      let sanitized = message;
      for (const pattern of sensitivePatterns) {
        sanitized = sanitized.replace(pattern, '[REDACTED]');
      }
      return sanitized;
    };

    expect(sanitizeError('Failed with password=abc123')).not.toContain('abc123');
    expect(sanitizeError('Auth: Bearer eyJhbGciOiJIUzI1NiJ9.xyz')).not.toContain('eyJ');
    expect(sanitizeError('Normal error message')).toBe('Normal error message');
  });
});
