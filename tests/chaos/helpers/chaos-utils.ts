/**
 * Chaos Testing Utilities
 *
 * Shared utilities for chaos/resilience testing including:
 * - Production safety assertions
 * - Failing/slow operation factories
 * - Memory monitoring
 * - Mock service simulation
 */

import { EventEmitter } from 'events';

// ============================================================================
// Production Safety
// ============================================================================

/**
 * Asserts that tests are not running in production environment.
 * Throws an error if NODE_ENV is 'production'.
 */
export function assertProductionSafe(): void {
  if (process.env.NODE_ENV === 'production') {
    throw new Error(
      'CHAOS TEST BLOCKED: Cannot run chaos tests in production environment. ' +
      'Set NODE_ENV to "test" or "development" to proceed.'
    );
  }
}

// ============================================================================
// Operation Factories
// ============================================================================

/**
 * Creates an operation that fails a specified number of times before succeeding.
 *
 * @param failCount - Number of times the operation should fail before succeeding
 * @param error - The error to throw on failure (defaults to ECONNREFUSED)
 * @returns A jest mock function that tracks attempts
 */
export function createFailingOperation<T = { success: boolean; attempts: number }>(
  failCount: number,
  error: Error = new Error('ECONNREFUSED')
): jest.Mock<Promise<T>> {
  let attempts = 0;
  return jest.fn().mockImplementation(async (): Promise<T> => {
    attempts++;
    if (attempts <= failCount) {
      throw error;
    }
    return { success: true, attempts } as T;
  });
}

/**
 * Creates an operation that completes after a specified delay.
 *
 * @param delayMs - Delay in milliseconds before the operation completes
 * @returns A jest mock function that simulates slow network
 */
export function createSlowOperation<T = { success: boolean }>(
  delayMs: number
): jest.Mock<Promise<T>> {
  return jest.fn().mockImplementation(async (): Promise<T> => {
    await delay(delayMs);
    return { success: true } as T;
  });
}

/**
 * Creates an operation that fails intermittently based on a failure rate.
 *
 * @param failureRate - Probability of failure (0 to 1)
 * @param error - The error to throw on failure
 * @returns A jest mock function with random failures
 */
export function createIntermittentOperation<T = { success: boolean }>(
  failureRate: number,
  error: Error = new Error('RANDOM_FAILURE')
): jest.Mock<Promise<T>> {
  return jest.fn().mockImplementation(async (): Promise<T> => {
    if (Math.random() < failureRate) {
      throw error;
    }
    return { success: true } as T;
  });
}

// ============================================================================
// Memory Monitoring
// ============================================================================

export interface MemoryUsage {
  heapUsed: number;
  heapTotal: number;
  rss: number;
  external: number;
}

/**
 * Gets current memory usage in megabytes.
 *
 * @returns Memory usage statistics in MB
 */
export function getMemoryUsageMB(): MemoryUsage {
  const used = process.memoryUsage();
  return {
    heapUsed: Math.round(used.heapUsed / 1024 / 1024),
    heapTotal: Math.round(used.heapTotal / 1024 / 1024),
    rss: Math.round(used.rss / 1024 / 1024),
    external: Math.round(used.external / 1024 / 1024),
  };
}

/**
 * Checks if memory usage is within acceptable limits.
 *
 * @param maxHeapMB - Maximum allowed heap memory in MB (default: 500)
 * @returns true if memory usage is acceptable
 */
export function isMemoryUsageAcceptable(maxHeapMB: number = 500): boolean {
  const usage = getMemoryUsageMB();
  return usage.heapUsed < maxHeapMB;
}

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Delays execution for specified milliseconds.
 *
 * @param ms - Delay in milliseconds
 */
export function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Executes an operation with a timeout.
 *
 * @param operation - The async operation to execute
 * @param timeoutMs - Timeout in milliseconds
 * @param timeoutError - Error message for timeout
 * @returns The result of the operation or throws on timeout
 */
export async function withTimeout<T>(
  operation: () => Promise<T>,
  timeoutMs: number,
  timeoutError: string = 'Operation timed out'
): Promise<T> {
  return Promise.race([
    operation(),
    new Promise<never>((_, reject) =>
      setTimeout(() => reject(new Error(timeoutError)), timeoutMs)
    ),
  ]);
}

// ============================================================================
// Mock Service Class
// ============================================================================

export interface ServiceHealth {
  isHealthy: boolean;
  lastCheckTime: Date;
  consecutiveFailures: number;
}

export interface MockServiceConfig {
  name: string;
  initiallyHealthy?: boolean;
  healthCheckInterval?: number;
}

/**
 * Mock service for simulating microservice behavior in chaos tests.
 * Supports controllable health status, operation simulation, and event emission.
 */
export class MockService extends EventEmitter {
  public readonly name: string;
  private _isHealthy: boolean;
  private _consecutiveFailures: number = 0;
  private _lastCheckTime: Date = new Date();
  private _operationQueue: Array<{ data: unknown; timestamp: Date }> = [];
  private _healthCheckInterval?: NodeJS.Timeout;

  constructor(config: MockServiceConfig) {
    super();
    this.name = config.name;
    this._isHealthy = config.initiallyHealthy ?? true;

    if (config.healthCheckInterval) {
      this.startHealthChecks(config.healthCheckInterval);
    }
  }

  get isHealthy(): boolean {
    return this._isHealthy;
  }

  get health(): ServiceHealth {
    return {
      isHealthy: this._isHealthy,
      lastCheckTime: this._lastCheckTime,
      consecutiveFailures: this._consecutiveFailures,
    };
  }

  get queuedOperations(): number {
    return this._operationQueue.length;
  }

  /**
   * Sets the health status of the service.
   * Emits 'healthChange' event when status changes.
   */
  setHealthy(healthy: boolean): void {
    const wasHealthy = this._isHealthy;
    this._isHealthy = healthy;
    this._lastCheckTime = new Date();

    if (healthy) {
      this._consecutiveFailures = 0;
    } else {
      this._consecutiveFailures++;
    }

    if (wasHealthy !== healthy) {
      this.emit('healthChange', { wasHealthy, isHealthy: healthy });
    }
  }

  /**
   * Simulates a service operation.
   * If unhealthy, queues the operation for later processing.
   */
  async executeOperation<T>(operation: () => Promise<T>): Promise<T | null> {
    if (!this._isHealthy) {
      // Queue operation for when service recovers
      this._operationQueue.push({
        data: operation,
        timestamp: new Date(),
      });
      this.emit('operationQueued', { queueSize: this._operationQueue.length });
      return null;
    }

    try {
      const result = await operation();
      this.emit('operationSuccess', { result });
      return result;
    } catch (error) {
      this._consecutiveFailures++;
      this.emit('operationFailure', { error, consecutiveFailures: this._consecutiveFailures });
      throw error;
    }
  }

  /**
   * Processes all queued operations after service recovery.
   */
  async processQueue(): Promise<number> {
    if (!this._isHealthy) {
      throw new Error('Cannot process queue while service is unhealthy');
    }

    let processed = 0;
    while (this._operationQueue.length > 0) {
      const item = this._operationQueue.shift();
      if (item && typeof item.data === 'function') {
        try {
          await item.data();
          processed++;
        } catch {
          // Log but continue processing queue
        }
      }
    }

    this.emit('queueProcessed', { processed });
    return processed;
  }

  /**
   * Starts periodic health checks.
   */
  private startHealthChecks(intervalMs: number): void {
    this._healthCheckInterval = setInterval(() => {
      this._lastCheckTime = new Date();
      this.emit('healthCheck', { healthy: this._isHealthy });
    }, intervalMs);
  }

  /**
   * Cleans up resources.
   */
  destroy(): void {
    if (this._healthCheckInterval) {
      clearInterval(this._healthCheckInterval);
    }
    this._operationQueue = [];
    this.removeAllListeners();
  }
}

// ============================================================================
// Network Partition Simulator
// ============================================================================

export interface PartitionConfig {
  detectionTimeMs: number;
  healingTimeMs: number;
}

/**
 * Simulates network partition for testing resilience.
 */
export class NetworkPartitionSimulator extends EventEmitter {
  private _isPartitioned: boolean = false;
  private _partitionStartTime?: Date;
  private _services: Map<string, MockService> = new Map();

  get isPartitioned(): boolean {
    return this._isPartitioned;
  }

  registerService(service: MockService): void {
    this._services.set(service.name, service);
  }

  /**
   * Simulates a network partition affecting all registered services.
   */
  async simulatePartition(config: PartitionConfig): Promise<void> {
    this._isPartitioned = true;
    this._partitionStartTime = new Date();

    // Mark all services as unhealthy
    for (const service of this._services.values()) {
      service.setHealthy(false);
    }

    this.emit('partitionStarted', { timestamp: this._partitionStartTime });

    // Wait for detection time
    await delay(config.detectionTimeMs);
    this.emit('partitionDetected', {
      detectionTime: config.detectionTimeMs,
      affectedServices: Array.from(this._services.keys()),
    });
  }

  /**
   * Heals the network partition.
   */
  async healPartition(config: PartitionConfig): Promise<void> {
    await delay(config.healingTimeMs);

    // Mark all services as healthy
    for (const service of this._services.values()) {
      service.setHealthy(true);
    }

    this._isPartitioned = false;
    this.emit('partitionHealed', {
      healingTime: config.healingTimeMs,
      duration: this._partitionStartTime
        ? Date.now() - this._partitionStartTime.getTime()
        : 0,
    });
  }
}

// ============================================================================
// Weather Data Validation Constants (WMO Standards)
// ============================================================================

export const WEATHER_VALIDATION_LIMITS = {
  temperature: { min: -89.2, max: 70.7 },  // Earth's recorded extremes (°C)
  humidity: { min: 0, max: 100 },          // Percentage
  pressure: { min: 870, max: 1084 },       // hPa
  windSpeed: { min: 0, max: 113.2 },       // m/s (Mt. Washington record)
  precipitation: { min: 0, max: 1825.8 },  // mm/hour (Unionville record)
} as const;

/**
 * Validates weather data against WMO physical limits.
 */
export function validateWeatherData(data: {
  temperature?: number;
  humidity?: number;
  pressure?: number;
  windSpeed?: number;
  precipitation?: number;
}): { valid: boolean; errors: string[] } {
  const errors: string[] = [];

  if (data.temperature !== undefined) {
    const limits = WEATHER_VALIDATION_LIMITS.temperature;
    if (data.temperature < limits.min || data.temperature > limits.max) {
      errors.push(`Temperature ${data.temperature}°C out of range [${limits.min}, ${limits.max}]`);
    }
  }

  if (data.humidity !== undefined) {
    const limits = WEATHER_VALIDATION_LIMITS.humidity;
    if (data.humidity < limits.min || data.humidity > limits.max) {
      errors.push(`Humidity ${data.humidity}% out of range [${limits.min}, ${limits.max}]`);
    }
  }

  if (data.pressure !== undefined) {
    const limits = WEATHER_VALIDATION_LIMITS.pressure;
    if (data.pressure < limits.min || data.pressure > limits.max) {
      errors.push(`Pressure ${data.pressure} hPa out of range [${limits.min}, ${limits.max}]`);
    }
  }

  if (data.windSpeed !== undefined) {
    const limits = WEATHER_VALIDATION_LIMITS.windSpeed;
    if (data.windSpeed < limits.min || data.windSpeed > limits.max) {
      errors.push(`Wind speed ${data.windSpeed} m/s out of range [${limits.min}, ${limits.max}]`);
    }
  }

  if (data.precipitation !== undefined) {
    const limits = WEATHER_VALIDATION_LIMITS.precipitation;
    if (data.precipitation < limits.min || data.precipitation > limits.max) {
      errors.push(`Precipitation ${data.precipitation} mm/hr out of range [${limits.min}, ${limits.max}]`);
    }
  }

  return {
    valid: errors.length === 0,
    errors,
  };
}
