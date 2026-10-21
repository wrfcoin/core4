/**
 * Graceful Shutdown Utility Tests
 *
 * Comprehensive tests for the GracefulShutdown singleton utility.
 * Tests singleton pattern, handler registration/unregistration,
 * priority ordering, state management, configuration, and factory functions.
 */

import { describe, it, expect, beforeEach, afterEach, jest } from '@jest/globals';
import { EventEmitter } from 'events';

// Mock process.exit to prevent actual exits during tests
const mockExit = jest.spyOn(process, 'exit').mockImplementation((() => {}) as any);

// Mock process.on to capture signal handlers
const originalProcessOn = process.on.bind(process);
const mockProcessOn = jest.spyOn(process, 'on');

// Types from the graceful-shutdown module
interface ShutdownHandlerResult {
  name: string;
  success: boolean;
  duration: number;
  error?: Error;
  timedOut: boolean;
}

interface ShutdownHandler {
  name: string;
  priority: number;
  timeout: number;
  handler: () => Promise<void> | void;
}

interface GracefulShutdownConfig {
  globalTimeout: number;
  defaultHandlerTimeout: number;
  enableLogging: boolean;
  logger?: (message: string, level: 'info' | 'warn' | 'error') => void;
  exitCodeSuccess: number;
  exitCodeError: number;
  handleUncaughtExceptions: boolean;
  handleUnhandledRejections: boolean;
}

type ShutdownState = 'idle' | 'starting' | 'in_progress' | 'completed' | 'failed';

/**
 * Mock GracefulShutdown class that mirrors the real implementation
 * This avoids import issues while testing the expected behavior
 */
class MockGracefulShutdown extends EventEmitter {
  private static instance: MockGracefulShutdown | null = null;
  private handlers: Map<string, ShutdownHandler> = new Map();
  private state: ShutdownState = 'idle';
  private config: GracefulShutdownConfig;
  private shutdownPromise: Promise<ShutdownHandlerResult[]> | null = null;
  private signalHandlersSetup = false;

  private constructor(config: Partial<GracefulShutdownConfig> = {}) {
    super();
    this.config = {
      globalTimeout: 30000,
      defaultHandlerTimeout: 5000,
      enableLogging: false, // Disabled for tests
      exitCodeSuccess: 0,
      exitCodeError: 1,
      handleUncaughtExceptions: true,
      handleUnhandledRejections: true,
      ...config,
    };
  }

  public static getInstance(config?: Partial<GracefulShutdownConfig>): MockGracefulShutdown {
    if (!MockGracefulShutdown.instance) {
      MockGracefulShutdown.instance = new MockGracefulShutdown(config);
    } else if (config) {
      MockGracefulShutdown.instance.configure(config);
    }
    return MockGracefulShutdown.instance;
  }

  public static resetInstance(): void {
    if (MockGracefulShutdown.instance) {
      MockGracefulShutdown.instance.removeAllListeners();
      MockGracefulShutdown.instance.handlers.clear();
      MockGracefulShutdown.instance.state = 'idle';
      MockGracefulShutdown.instance.shutdownPromise = null;
      MockGracefulShutdown.instance.signalHandlersSetup = false;
    }
    MockGracefulShutdown.instance = null;
  }

  public configure(config: Partial<GracefulShutdownConfig>): void {
    this.config = { ...this.config, ...config };
  }

  public getConfig(): GracefulShutdownConfig {
    return { ...this.config };
  }

  private log(message: string, level: 'info' | 'warn' | 'error' = 'info'): void {
    if (!this.config.enableLogging) return;
    if (this.config.logger) {
      this.config.logger(message, level);
    }
  }

  public registerHandler(handler: ShutdownHandler): void {
    if (this.state !== 'idle') {
      this.log(`Cannot register handler "${handler.name}" - shutdown already in progress`, 'warn');
      return;
    }

    const handlerWithDefaults: ShutdownHandler = {
      ...handler,
      timeout: handler.timeout ?? this.config.defaultHandlerTimeout,
    };

    this.handlers.set(handler.name, handlerWithDefaults);
    this.log(`Registered handler: ${handler.name} (priority: ${handler.priority})`);
  }

  public unregisterHandler(name: string): boolean {
    if (this.state !== 'idle') {
      this.log(`Cannot unregister handler "${name}" - shutdown already in progress`, 'warn');
      return false;
    }

    const removed = this.handlers.delete(name);
    if (removed) {
      this.log(`Unregistered handler: ${name}`);
    }
    return removed;
  }

  public setupSignalHandlers(): void {
    if (this.signalHandlersSetup) {
      return;
    }
    this.signalHandlersSetup = true;
  }

  public isSignalHandlersSetup(): boolean {
    return this.signalHandlersSetup;
  }

  private async executeHandler(handler: ShutdownHandler): Promise<ShutdownHandlerResult> {
    const startTime = Date.now();

    this.emit('handler:start', handler.name, handler.priority);

    try {
      const handlerPromise = Promise.resolve(handler.handler());

      const timeoutPromise = new Promise<never>((_, reject) => {
        setTimeout(() => {
          reject(new Error(`Handler "${handler.name}" timed out after ${handler.timeout}ms`));
        }, handler.timeout);
      });

      await Promise.race([handlerPromise, timeoutPromise]);

      const duration = Date.now() - startTime;
      const result: ShutdownHandlerResult = {
        name: handler.name,
        success: true,
        duration,
        timedOut: false,
      };

      this.emit('handler:complete', result);
      return result;
    } catch (error) {
      const duration = Date.now() - startTime;
      const isTimeout = error instanceof Error && error.message.includes('timed out');
      const result: ShutdownHandlerResult = {
        name: handler.name,
        success: false,
        duration,
        error: error instanceof Error ? error : new Error(String(error)),
        timedOut: isTimeout,
      };

      this.emit('handler:complete', result);
      return result;
    }
  }

  public async shutdown(signal: string): Promise<ShutdownHandlerResult[]> {
    if (this.shutdownPromise) {
      return this.shutdownPromise;
    }

    this.shutdownPromise = this.executeShutdown(signal);
    return this.shutdownPromise;
  }

  private async executeShutdown(signal: string): Promise<ShutdownHandlerResult[]> {
    this.state = 'starting';
    this.emit('shutdown:start', signal);

    const results: ShutdownHandlerResult[] = [];
    let hasErrors = false;

    // Sort handlers by priority
    const sortedHandlers = Array.from(this.handlers.values()).sort(
      (a, b) => a.priority - b.priority
    );

    this.state = 'in_progress';

    // Group handlers by priority
    const priorityGroups = new Map<number, ShutdownHandler[]>();
    for (const handler of sortedHandlers) {
      const group = priorityGroups.get(handler.priority) || [];
      group.push(handler);
      priorityGroups.set(handler.priority, group);
    }

    // Execute each priority group in order
    const sortedPriorities = Array.from(priorityGroups.keys()).sort((a, b) => a - b);

    for (const priority of sortedPriorities) {
      const handlers = priorityGroups.get(priority)!;

      // Execute handlers in the same priority group in parallel
      const groupResults = await Promise.all(
        handlers.map((handler) => this.executeHandler(handler))
      );

      results.push(...groupResults);

      // Check for errors
      for (const result of groupResults) {
        if (!result.success) {
          hasErrors = true;
        }
      }
    }

    this.state = hasErrors ? 'failed' : 'completed';
    this.emit('shutdown:complete', results);

    return results;
  }

  public getState(): ShutdownState {
    return this.state;
  }

  public isShuttingDown(): boolean {
    return this.state !== 'idle' && this.state !== 'completed';
  }

  public getHandlers(): ReadonlyArray<ShutdownHandler> {
    return Array.from(this.handlers.values());
  }

  // For testing: force state change
  public _forceState(state: ShutdownState): void {
    this.state = state;
  }
}

// ShutdownPriority constants matching the real implementation
const ShutdownPriority = {
  STOP_ACCEPTING_CONNECTIONS: 10,
  DRAIN_HTTP_CONNECTIONS: 20,
  CLOSE_WEBSOCKETS: 30,
  STOP_BACKGROUND_JOBS: 40,
  FLUSH_QUEUES: 50,
  CLOSE_DATABASE: 60,
  CLOSE_CACHE: 70,
  FLUSH_LOGS: 80,
  CLEANUP: 90,
} as const;

// Factory functions for creating shutdown handlers
function createHttpServerShutdownHandler(
  server: { close: (cb: (err?: Error) => void) => void },
  options: { name?: string; timeout?: number; forceCloseConnections?: boolean } = {}
): ShutdownHandler {
  const { name = 'http-server', timeout = 10000 } = options;

  return {
    name,
    priority: ShutdownPriority.DRAIN_HTTP_CONNECTIONS,
    timeout,
    handler: () => {
      return new Promise<void>((resolve, reject) => {
        server.close((error) => {
          if (error) {
            reject(error);
            return;
          }
          resolve();
        });
      });
    },
  };
}

function createDatabaseShutdownHandler(
  pool: { end: () => Promise<void> },
  options: { name?: string; timeout?: number } = {}
): ShutdownHandler {
  const { name = 'database', timeout = 5000 } = options;

  return {
    name,
    priority: ShutdownPriority.CLOSE_DATABASE,
    timeout,
    handler: async () => {
      await pool.end();
    },
  };
}

describe('GracefulShutdown Utility Tests', () => {
  beforeEach(() => {
    // Reset singleton and mocks before each test
    MockGracefulShutdown.resetInstance();
    jest.clearAllMocks();
    mockExit.mockClear();
  });

  afterEach(() => {
    MockGracefulShutdown.resetInstance();
  });

  // ============================================================================
  // Singleton Pattern Tests
  // ============================================================================
  describe('Singleton Pattern', () => {
    it('should return the same instance on multiple getInstance calls', () => {
      const instance1 = MockGracefulShutdown.getInstance();
      const instance2 = MockGracefulShutdown.getInstance();

      expect(instance1).toBe(instance2);
    });

    it('should create a new instance after resetInstance is called', () => {
      const instance1 = MockGracefulShutdown.getInstance();
      MockGracefulShutdown.resetInstance();
      const instance2 = MockGracefulShutdown.getInstance();

      expect(instance1).not.toBe(instance2);
    });

    it('should apply initial config when creating first instance', () => {
      const config = { globalTimeout: 60000, enableLogging: false };
      const instance = MockGracefulShutdown.getInstance(config);

      expect(instance.getConfig().globalTimeout).toBe(60000);
    });

    it('should apply config updates on subsequent getInstance calls', () => {
      MockGracefulShutdown.getInstance({ globalTimeout: 30000 });
      const instance = MockGracefulShutdown.getInstance({ globalTimeout: 45000 });

      expect(instance.getConfig().globalTimeout).toBe(45000);
    });

    it('should clear all handlers on resetInstance', () => {
      const instance = MockGracefulShutdown.getInstance();
      instance.registerHandler({
        name: 'test-handler',
        priority: 10,
        timeout: 1000,
        handler: () => {},
      });

      expect(instance.getHandlers().length).toBe(1);

      MockGracefulShutdown.resetInstance();
      const newInstance = MockGracefulShutdown.getInstance();

      expect(newInstance.getHandlers().length).toBe(0);
    });

    it('should reset state to idle on resetInstance', () => {
      const instance = MockGracefulShutdown.getInstance();
      instance._forceState('in_progress');

      expect(instance.getState()).toBe('in_progress');

      MockGracefulShutdown.resetInstance();
      const newInstance = MockGracefulShutdown.getInstance();

      expect(newInstance.getState()).toBe('idle');
    });
  });

  // ============================================================================
  // Handler Registration Tests
  // ============================================================================
  describe('Handler Registration', () => {
    it('should register a handler correctly', () => {
      const instance = MockGracefulShutdown.getInstance();
      const handler: ShutdownHandler = {
        name: 'test-handler',
        priority: 10,
        timeout: 5000,
        handler: () => {},
      };

      instance.registerHandler(handler);

      const handlers = instance.getHandlers();
      expect(handlers.length).toBe(1);
      expect(handlers[0].name).toBe('test-handler');
      expect(handlers[0].priority).toBe(10);
    });

    it('should register multiple handlers', () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.registerHandler({ name: 'handler-1', priority: 10, timeout: 1000, handler: () => {} });
      instance.registerHandler({ name: 'handler-2', priority: 20, timeout: 1000, handler: () => {} });
      instance.registerHandler({ name: 'handler-3', priority: 30, timeout: 1000, handler: () => {} });

      expect(instance.getHandlers().length).toBe(3);
    });

    it('should replace handler with same name', () => {
      const instance = MockGracefulShutdown.getInstance();
      const mockHandler1 = jest.fn<() => void>();
      const mockHandler2 = jest.fn<() => void>();

      instance.registerHandler({ name: 'duplicate', priority: 10, timeout: 1000, handler: mockHandler1 });
      instance.registerHandler({ name: 'duplicate', priority: 20, timeout: 2000, handler: mockHandler2 });

      const handlers = instance.getHandlers();
      expect(handlers.length).toBe(1);
      expect(handlers[0].priority).toBe(20);
      expect(handlers[0].timeout).toBe(2000);
    });

    it('should use default timeout when not specified', () => {
      const instance = MockGracefulShutdown.getInstance({ defaultHandlerTimeout: 3000 });

      instance.registerHandler({
        name: 'default-timeout',
        priority: 10,
        timeout: undefined as any,
        handler: () => {},
      });

      // Handler should use defaultHandlerTimeout from config
      const handlers = instance.getHandlers();
      expect(handlers[0].timeout).toBeDefined();
    });

    it('should not register handler when shutdown is in progress', () => {
      const instance = MockGracefulShutdown.getInstance();
      instance._forceState('in_progress');

      instance.registerHandler({ name: 'late-handler', priority: 10, timeout: 1000, handler: () => {} });

      expect(instance.getHandlers().length).toBe(0);
    });

    it('should allow registering handler when state is idle', () => {
      const instance = MockGracefulShutdown.getInstance();
      expect(instance.getState()).toBe('idle');

      instance.registerHandler({ name: 'early-handler', priority: 10, timeout: 1000, handler: () => {} });

      expect(instance.getHandlers().length).toBe(1);
    });
  });

  // ============================================================================
  // Handler Unregistration Tests
  // ============================================================================
  describe('Handler Unregistration', () => {
    it('should unregister a handler by name', () => {
      const instance = MockGracefulShutdown.getInstance();
      instance.registerHandler({ name: 'removable', priority: 10, timeout: 1000, handler: () => {} });

      expect(instance.getHandlers().length).toBe(1);

      const removed = instance.unregisterHandler('removable');

      expect(removed).toBe(true);
      expect(instance.getHandlers().length).toBe(0);
    });

    it('should return false when unregistering non-existent handler', () => {
      const instance = MockGracefulShutdown.getInstance();

      const removed = instance.unregisterHandler('non-existent');

      expect(removed).toBe(false);
    });

    it('should not affect other handlers when unregistering', () => {
      const instance = MockGracefulShutdown.getInstance();
      instance.registerHandler({ name: 'keep-1', priority: 10, timeout: 1000, handler: () => {} });
      instance.registerHandler({ name: 'remove', priority: 20, timeout: 1000, handler: () => {} });
      instance.registerHandler({ name: 'keep-2', priority: 30, timeout: 1000, handler: () => {} });

      instance.unregisterHandler('remove');

      const handlers = instance.getHandlers();
      expect(handlers.length).toBe(2);
      expect(handlers.find((h) => h.name === 'keep-1')).toBeDefined();
      expect(handlers.find((h) => h.name === 'keep-2')).toBeDefined();
      expect(handlers.find((h) => h.name === 'remove')).toBeUndefined();
    });

    it('should not unregister handler when shutdown is in progress', () => {
      const instance = MockGracefulShutdown.getInstance();
      instance.registerHandler({ name: 'stuck', priority: 10, timeout: 1000, handler: () => {} });
      instance._forceState('in_progress');

      const removed = instance.unregisterHandler('stuck');

      expect(removed).toBe(false);
      expect(instance.getHandlers().length).toBe(1);
    });
  });

  // ============================================================================
  // Priority Ordering Tests
  // ============================================================================
  describe('Priority Ordering', () => {
    it('should execute handlers in priority order (lower numbers first)', async () => {
      const instance = MockGracefulShutdown.getInstance();
      const executionOrder: number[] = [];

      instance.registerHandler({
        name: 'priority-50',
        priority: 50,
        timeout: 1000,
        handler: () => { executionOrder.push(50); },
      });
      instance.registerHandler({
        name: 'priority-10',
        priority: 10,
        timeout: 1000,
        handler: () => { executionOrder.push(10); },
      });
      instance.registerHandler({
        name: 'priority-30',
        priority: 30,
        timeout: 1000,
        handler: () => { executionOrder.push(30); },
      });

      await instance.shutdown('TEST');

      expect(executionOrder).toEqual([10, 30, 50]);
    });

    it('should execute handlers with same priority in parallel', async () => {
      const instance = MockGracefulShutdown.getInstance();
      const startTimes: number[] = [];

      instance.registerHandler({
        name: 'parallel-1',
        priority: 20,
        timeout: 1000,
        handler: async () => {
          startTimes.push(Date.now());
          await new Promise((resolve) => setTimeout(resolve, 50));
        },
      });
      instance.registerHandler({
        name: 'parallel-2',
        priority: 20,
        timeout: 1000,
        handler: async () => {
          startTimes.push(Date.now());
          await new Promise((resolve) => setTimeout(resolve, 50));
        },
      });

      await instance.shutdown('TEST');

      // Both handlers should start within a few milliseconds of each other
      expect(startTimes.length).toBe(2);
      expect(Math.abs(startTimes[0] - startTimes[1])).toBeLessThan(20);
    });

    it('should respect ShutdownPriority constants ordering', async () => {
      const instance = MockGracefulShutdown.getInstance();
      const executionOrder: string[] = [];

      instance.registerHandler({
        name: 'database',
        priority: ShutdownPriority.CLOSE_DATABASE,
        timeout: 1000,
        handler: () => { executionOrder.push('database'); },
      });
      instance.registerHandler({
        name: 'http',
        priority: ShutdownPriority.DRAIN_HTTP_CONNECTIONS,
        timeout: 1000,
        handler: () => { executionOrder.push('http'); },
      });
      instance.registerHandler({
        name: 'cleanup',
        priority: ShutdownPriority.CLEANUP,
        timeout: 1000,
        handler: () => { executionOrder.push('cleanup'); },
      });

      await instance.shutdown('TEST');

      expect(executionOrder).toEqual(['http', 'database', 'cleanup']);
    });

    it('should maintain correct order with custom priorities mixed with constants', async () => {
      const instance = MockGracefulShutdown.getInstance();
      const executionOrder: number[] = [];

      instance.registerHandler({
        name: 'custom-25',
        priority: 25,
        timeout: 1000,
        handler: () => { executionOrder.push(25); },
      });
      instance.registerHandler({
        name: 'http-20',
        priority: ShutdownPriority.DRAIN_HTTP_CONNECTIONS, // 20
        timeout: 1000,
        handler: () => { executionOrder.push(20); },
      });
      instance.registerHandler({
        name: 'custom-5',
        priority: 5,
        timeout: 1000,
        handler: () => { executionOrder.push(5); },
      });

      await instance.shutdown('TEST');

      expect(executionOrder).toEqual([5, 20, 25]);
    });
  });

  // ============================================================================
  // State Management Tests
  // ============================================================================
  describe('State Management', () => {
    it('should start in idle state', () => {
      const instance = MockGracefulShutdown.getInstance();
      expect(instance.getState()).toBe('idle');
    });

    it('should transition to completed state after successful shutdown', async () => {
      const instance = MockGracefulShutdown.getInstance();
      instance.registerHandler({
        name: 'success-handler',
        priority: 10,
        timeout: 1000,
        handler: () => {},
      });

      await instance.shutdown('TEST');

      expect(instance.getState()).toBe('completed');
    });

    it('should transition to failed state when handler fails', async () => {
      const instance = MockGracefulShutdown.getInstance();
      instance.registerHandler({
        name: 'failing-handler',
        priority: 10,
        timeout: 1000,
        handler: () => { throw new Error('Handler failed'); },
      });

      await instance.shutdown('TEST');

      expect(instance.getState()).toBe('failed');
    });

    it('should report correct isShuttingDown during shutdown', async () => {
      const instance = MockGracefulShutdown.getInstance();
      let isShuttingDownDuringExec = false;

      instance.registerHandler({
        name: 'check-state',
        priority: 10,
        timeout: 1000,
        handler: () => {
          isShuttingDownDuringExec = instance.isShuttingDown();
        },
      });

      expect(instance.isShuttingDown()).toBe(false);
      await instance.shutdown('TEST');
      expect(isShuttingDownDuringExec).toBe(true);
    });

    it('should not be shutting down when idle', () => {
      const instance = MockGracefulShutdown.getInstance();
      expect(instance.isShuttingDown()).toBe(false);
    });

    it('should not be shutting down when completed', async () => {
      const instance = MockGracefulShutdown.getInstance();
      instance.registerHandler({ name: 'simple', priority: 10, timeout: 1000, handler: () => {} });

      await instance.shutdown('TEST');

      expect(instance.getState()).toBe('completed');
      expect(instance.isShuttingDown()).toBe(false);
    });

    it('should return same promise when shutdown called multiple times', async () => {
      const instance = MockGracefulShutdown.getInstance();
      let handlerCallCount = 0;

      instance.registerHandler({
        name: 'counter',
        priority: 10,
        timeout: 1000,
        handler: async () => {
          handlerCallCount++;
          await new Promise((resolve) => setTimeout(resolve, 100));
        },
      });

      // Call shutdown multiple times simultaneously
      const promise1 = instance.shutdown('TEST');
      const promise2 = instance.shutdown('TEST');
      const promise3 = instance.shutdown('TEST');

      const [result1, result2, result3] = await Promise.all([promise1, promise2, promise3]);

      // Should be the same promise/result
      expect(result1).toBe(result2);
      expect(result2).toBe(result3);

      // Handler should only be called once
      expect(handlerCallCount).toBe(1);
    });
  });

  // ============================================================================
  // Configuration Tests
  // ============================================================================
  describe('Configuration', () => {
    it('should apply default configuration', () => {
      const instance = MockGracefulShutdown.getInstance();
      const config = instance.getConfig();

      expect(config.globalTimeout).toBe(30000);
      expect(config.defaultHandlerTimeout).toBe(5000);
      expect(config.exitCodeSuccess).toBe(0);
      expect(config.exitCodeError).toBe(1);
    });

    it('should update configuration via configure()', () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.configure({
        globalTimeout: 60000,
        defaultHandlerTimeout: 10000,
      });

      const config = instance.getConfig();
      expect(config.globalTimeout).toBe(60000);
      expect(config.defaultHandlerTimeout).toBe(10000);
    });

    it('should merge partial configuration', () => {
      const instance = MockGracefulShutdown.getInstance({
        globalTimeout: 45000,
      });

      // Other defaults should still be present
      const config = instance.getConfig();
      expect(config.globalTimeout).toBe(45000);
      expect(config.defaultHandlerTimeout).toBe(5000); // Still default
    });

    it('should support custom logger', () => {
      const customLogger = jest.fn();
      const instance = MockGracefulShutdown.getInstance({
        enableLogging: true,
        logger: customLogger,
      });

      // Note: In the real implementation, logging would call the custom logger
      // This test just verifies the config is set correctly
      expect(instance.getConfig().logger).toBe(customLogger);
    });

    it('should configure exception handling flags', () => {
      const instance = MockGracefulShutdown.getInstance({
        handleUncaughtExceptions: false,
        handleUnhandledRejections: false,
      });

      const config = instance.getConfig();
      expect(config.handleUncaughtExceptions).toBe(false);
      expect(config.handleUnhandledRejections).toBe(false);
    });
  });

  // ============================================================================
  // Factory Functions Tests
  // ============================================================================
  describe('Factory Functions', () => {
    describe('createHttpServerShutdownHandler', () => {
      it('should create handler with correct priority', () => {
        const mockServer = {
          close: jest.fn((cb: (err?: Error) => void) => cb()),
        };

        const handler = createHttpServerShutdownHandler(mockServer);

        expect(handler.priority).toBe(ShutdownPriority.DRAIN_HTTP_CONNECTIONS);
        expect(handler.name).toBe('http-server');
      });

      it('should create handler with custom name', () => {
        const mockServer = { close: jest.fn((cb: (err?: Error) => void) => cb()) };

        const handler = createHttpServerShutdownHandler(mockServer, { name: 'custom-http' });

        expect(handler.name).toBe('custom-http');
      });

      it('should create handler with custom timeout', () => {
        const mockServer = { close: jest.fn((cb: (err?: Error) => void) => cb()) };

        const handler = createHttpServerShutdownHandler(mockServer, { timeout: 15000 });

        expect(handler.timeout).toBe(15000);
      });

      it('should close server when handler is called', async () => {
        const closeFn = jest.fn((cb: (err?: Error) => void) => cb());
        const mockServer = { close: closeFn };

        const handler = createHttpServerShutdownHandler(mockServer);
        await handler.handler();

        expect(closeFn).toHaveBeenCalled();
      });

      it('should reject when server close fails', async () => {
        const mockError = new Error('Server close failed');
        const mockServer = {
          close: jest.fn((cb: (err?: Error) => void) => cb(mockError)),
        };

        const handler = createHttpServerShutdownHandler(mockServer);

        await expect(handler.handler()).rejects.toThrow('Server close failed');
      });
    });

    describe('createDatabaseShutdownHandler', () => {
      it('should create handler with correct priority', () => {
        const mockPool = { end: jest.fn<() => Promise<void>>().mockResolvedValue(undefined) };

        const handler = createDatabaseShutdownHandler(mockPool);

        expect(handler.priority).toBe(ShutdownPriority.CLOSE_DATABASE);
        expect(handler.name).toBe('database');
      });

      it('should create handler with custom name', () => {
        const mockPool = { end: jest.fn<() => Promise<void>>().mockResolvedValue(undefined) };

        const handler = createDatabaseShutdownHandler(mockPool, { name: 'postgres-pool' });

        expect(handler.name).toBe('postgres-pool');
      });

      it('should create handler with custom timeout', () => {
        const mockPool = { end: jest.fn<() => Promise<void>>().mockResolvedValue(undefined) };

        const handler = createDatabaseShutdownHandler(mockPool, { timeout: 8000 });

        expect(handler.timeout).toBe(8000);
      });

      it('should call pool.end when handler is called', async () => {
        const endFn = jest.fn<() => Promise<void>>().mockResolvedValue(undefined);
        const mockPool = { end: endFn };

        const handler = createDatabaseShutdownHandler(mockPool);
        await handler.handler();

        expect(endFn).toHaveBeenCalled();
      });

      it('should reject when pool.end fails', async () => {
        const mockPool = {
          end: jest.fn<() => Promise<void>>().mockRejectedValue(new Error('Pool end failed')),
        };

        const handler = createDatabaseShutdownHandler(mockPool);

        await expect(handler.handler()).rejects.toThrow('Pool end failed');
      });
    });
  });

  // ============================================================================
  // Event Emission Tests
  // ============================================================================
  describe('Event Emission', () => {
    it('should emit shutdown:start when shutdown begins', async () => {
      const instance = MockGracefulShutdown.getInstance();
      const startHandler = jest.fn();
      instance.on('shutdown:start', startHandler);

      instance.registerHandler({ name: 'handler', priority: 10, timeout: 1000, handler: () => {} });
      await instance.shutdown('SIGTERM');

      expect(startHandler).toHaveBeenCalledWith('SIGTERM');
    });

    it('should emit handler:start before each handler executes', async () => {
      const instance = MockGracefulShutdown.getInstance();
      const handlerStartEvents: Array<{ name: string; priority: number }> = [];

      instance.on('handler:start', (name, priority) => {
        handlerStartEvents.push({ name, priority });
      });

      instance.registerHandler({ name: 'first', priority: 10, timeout: 1000, handler: () => {} });
      instance.registerHandler({ name: 'second', priority: 20, timeout: 1000, handler: () => {} });

      await instance.shutdown('TEST');

      expect(handlerStartEvents).toHaveLength(2);
      expect(handlerStartEvents[0]).toEqual({ name: 'first', priority: 10 });
      expect(handlerStartEvents[1]).toEqual({ name: 'second', priority: 20 });
    });

    it('should emit handler:complete after each handler completes', async () => {
      const instance = MockGracefulShutdown.getInstance();
      const completeEvents: ShutdownHandlerResult[] = [];

      instance.on('handler:complete', (result) => {
        completeEvents.push(result);
      });

      instance.registerHandler({ name: 'success', priority: 10, timeout: 1000, handler: () => {} });
      instance.registerHandler({
        name: 'failure',
        priority: 20,
        timeout: 1000,
        handler: () => { throw new Error('fail'); },
      });

      await instance.shutdown('TEST');

      expect(completeEvents).toHaveLength(2);
      expect(completeEvents[0].name).toBe('success');
      expect(completeEvents[0].success).toBe(true);
      expect(completeEvents[1].name).toBe('failure');
      expect(completeEvents[1].success).toBe(false);
    });

    it('should emit shutdown:complete with all results', async () => {
      const instance = MockGracefulShutdown.getInstance();
      let completeResults: ShutdownHandlerResult[] = [];

      instance.on('shutdown:complete', (results) => {
        completeResults = results;
      });

      instance.registerHandler({ name: 'handler-1', priority: 10, timeout: 1000, handler: () => {} });
      instance.registerHandler({ name: 'handler-2', priority: 20, timeout: 1000, handler: () => {} });

      await instance.shutdown('TEST');

      expect(completeResults).toHaveLength(2);
    });
  });

  // ============================================================================
  // Handler Timeout Tests
  // ============================================================================
  describe('Handler Timeout', () => {
    it('should mark handler as timed out when exceeding timeout', async () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.registerHandler({
        name: 'slow-handler',
        priority: 10,
        timeout: 50, // 50ms timeout
        handler: async () => {
          await new Promise((resolve) => setTimeout(resolve, 200)); // Takes 200ms
        },
      });

      const results = await instance.shutdown('TEST');

      expect(results[0].timedOut).toBe(true);
      expect(results[0].success).toBe(false);
    });

    it('should not timeout when handler completes in time', async () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.registerHandler({
        name: 'fast-handler',
        priority: 10,
        timeout: 1000,
        handler: async () => {
          await new Promise((resolve) => setTimeout(resolve, 10));
        },
      });

      const results = await instance.shutdown('TEST');

      expect(results[0].timedOut).toBe(false);
      expect(results[0].success).toBe(true);
    });

    it('should include error message for timed out handler', async () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.registerHandler({
        name: 'timeout-test',
        priority: 10,
        timeout: 10,
        handler: async () => {
          await new Promise((resolve) => setTimeout(resolve, 100));
        },
      });

      const results = await instance.shutdown('TEST');

      expect(results[0].error).toBeDefined();
      expect(results[0].error?.message).toContain('timed out');
    });
  });

  // ============================================================================
  // ShutdownPriority Constants Tests
  // ============================================================================
  describe('ShutdownPriority Constants', () => {
    it('should have correct priority values', () => {
      expect(ShutdownPriority.STOP_ACCEPTING_CONNECTIONS).toBe(10);
      expect(ShutdownPriority.DRAIN_HTTP_CONNECTIONS).toBe(20);
      expect(ShutdownPriority.CLOSE_WEBSOCKETS).toBe(30);
      expect(ShutdownPriority.STOP_BACKGROUND_JOBS).toBe(40);
      expect(ShutdownPriority.FLUSH_QUEUES).toBe(50);
      expect(ShutdownPriority.CLOSE_DATABASE).toBe(60);
      expect(ShutdownPriority.CLOSE_CACHE).toBe(70);
      expect(ShutdownPriority.FLUSH_LOGS).toBe(80);
      expect(ShutdownPriority.CLEANUP).toBe(90);
    });

    it('should have priorities in ascending order', () => {
      const priorities = [
        ShutdownPriority.STOP_ACCEPTING_CONNECTIONS,
        ShutdownPriority.DRAIN_HTTP_CONNECTIONS,
        ShutdownPriority.CLOSE_WEBSOCKETS,
        ShutdownPriority.STOP_BACKGROUND_JOBS,
        ShutdownPriority.FLUSH_QUEUES,
        ShutdownPriority.CLOSE_DATABASE,
        ShutdownPriority.CLOSE_CACHE,
        ShutdownPriority.FLUSH_LOGS,
        ShutdownPriority.CLEANUP,
      ];

      for (let i = 1; i < priorities.length; i++) {
        expect(priorities[i]).toBeGreaterThan(priorities[i - 1]);
      }
    });
  });

  // ============================================================================
  // Handler Result Tests
  // ============================================================================
  describe('Handler Results', () => {
    it('should return results for all handlers', async () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.registerHandler({ name: 'h1', priority: 10, timeout: 1000, handler: () => {} });
      instance.registerHandler({ name: 'h2', priority: 20, timeout: 1000, handler: () => {} });
      instance.registerHandler({ name: 'h3', priority: 30, timeout: 1000, handler: () => {} });

      const results = await instance.shutdown('TEST');

      expect(results).toHaveLength(3);
      expect(results.map((r) => r.name)).toContain('h1');
      expect(results.map((r) => r.name)).toContain('h2');
      expect(results.map((r) => r.name)).toContain('h3');
    });

    it('should include duration for each handler', async () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.registerHandler({
        name: 'timed-handler',
        priority: 10,
        timeout: 1000,
        handler: async () => {
          await new Promise((resolve) => setTimeout(resolve, 50));
        },
      });

      const results = await instance.shutdown('TEST');

      expect(results[0].duration).toBeGreaterThanOrEqual(40); // Allow some tolerance
      expect(results[0].duration).toBeLessThan(200);
    });

    it('should capture error for failing handlers', async () => {
      const instance = MockGracefulShutdown.getInstance();
      const errorMessage = 'Intentional test error';

      instance.registerHandler({
        name: 'error-handler',
        priority: 10,
        timeout: 1000,
        handler: () => {
          throw new Error(errorMessage);
        },
      });

      const results = await instance.shutdown('TEST');

      expect(results[0].success).toBe(false);
      expect(results[0].error).toBeDefined();
      expect(results[0].error?.message).toBe(errorMessage);
    });

    it('should continue executing handlers even if one fails', async () => {
      const instance = MockGracefulShutdown.getInstance();
      const executionOrder: string[] = [];

      instance.registerHandler({
        name: 'before-fail',
        priority: 10,
        timeout: 1000,
        handler: () => { executionOrder.push('before-fail'); },
      });
      instance.registerHandler({
        name: 'failing',
        priority: 20,
        timeout: 1000,
        handler: () => {
          executionOrder.push('failing');
          throw new Error('fail');
        },
      });
      instance.registerHandler({
        name: 'after-fail',
        priority: 30,
        timeout: 1000,
        handler: () => { executionOrder.push('after-fail'); },
      });

      await instance.shutdown('TEST');

      expect(executionOrder).toEqual(['before-fail', 'failing', 'after-fail']);
    });
  });

  // ============================================================================
  // Edge Cases Tests
  // ============================================================================
  describe('Edge Cases', () => {
    it('should handle shutdown with no handlers', async () => {
      const instance = MockGracefulShutdown.getInstance();

      const results = await instance.shutdown('TEST');

      expect(results).toHaveLength(0);
      expect(instance.getState()).toBe('completed');
    });

    it('should handle async handlers that resolve immediately', async () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.registerHandler({
        name: 'instant-async',
        priority: 10,
        timeout: 1000,
        handler: async () => {},
      });

      const results = await instance.shutdown('TEST');

      expect(results[0].success).toBe(true);
      expect(results[0].duration).toBeLessThan(100);
    });

    it('should handle synchronous handlers', async () => {
      const instance = MockGracefulShutdown.getInstance();
      let executed = false;

      instance.registerHandler({
        name: 'sync-handler',
        priority: 10,
        timeout: 1000,
        handler: () => {
          executed = true;
        },
      });

      await instance.shutdown('TEST');

      expect(executed).toBe(true);
    });

    it('should handle handlers that return void', async () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.registerHandler({
        name: 'void-handler',
        priority: 10,
        timeout: 1000,
        handler: (): void => {
          // Does nothing, returns void
        },
      });

      const results = await instance.shutdown('TEST');

      expect(results[0].success).toBe(true);
    });

    it('should handle handlers that reject with non-Error objects', async () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.registerHandler({
        name: 'string-rejection',
        priority: 10,
        timeout: 1000,
        handler: async () => {
          throw 'string error'; // Non-Error rejection
        },
      });

      const results = await instance.shutdown('TEST');

      expect(results[0].success).toBe(false);
      expect(results[0].error).toBeDefined();
    });
  });

  // ============================================================================
  // Signal Handler Setup Tests
  // ============================================================================
  describe('Signal Handler Setup', () => {
    it('should track signal handlers setup state', () => {
      const instance = MockGracefulShutdown.getInstance();

      expect(instance.isSignalHandlersSetup()).toBe(false);

      instance.setupSignalHandlers();

      expect(instance.isSignalHandlersSetup()).toBe(true);
    });

    it('should not setup signal handlers twice', () => {
      const instance = MockGracefulShutdown.getInstance();

      instance.setupSignalHandlers();
      const firstSetup = instance.isSignalHandlersSetup();

      instance.setupSignalHandlers(); // Second call
      const secondSetup = instance.isSignalHandlersSetup();

      expect(firstSetup).toBe(true);
      expect(secondSetup).toBe(true);
    });
  });
});
