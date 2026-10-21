// Test Data Lifecycle Management for WRFCoin Platform
import { Page } from '@playwright/test';
import { TestDataManager, TestDataCategory } from './test-data-manager';
import MockServiceManager from './mock-services';
import crypto from 'crypto';
import fs from 'fs/promises';
import path from 'path';

export interface TestSession {
  id: string;
  name: string;
  startTime: Date;
  endTime?: Date;
  testData: TestDataCategory;
  dataSnapshot?: string;
  mockServices: string[];
  cleanupTasks: (() => Promise<void>)[];
  status: 'active' | 'completed' | 'failed' | 'aborted';
}

export interface DataIsolationConfig {
  isolateUsers: boolean;
  isolateWallets: boolean;
  isolateTransactions: boolean;
  isolateDevices: boolean;
  useRandomPrefixes: boolean;
  cleanup: 'immediate' | 'test-end' | 'manual';
}

export interface DataRecoveryPoint {
  id: string;
  sessionId: string;
  timestamp: Date;
  description: string;
  dataSnapshot: string;
  pageState?: any;
  mockState?: string[];
}

export class TestDataLifecycleManager {
  private static instance: TestDataLifecycleManager;
  private dataManager: TestDataManager;
  private activeSessions: Map<string, TestSession> = new Map();
  private recoveryPoints: Map<string, DataRecoveryPoint> = new Map();
  private baseDataPath: string;

  private constructor() {
    this.dataManager = TestDataManager.getInstance();
    this.baseDataPath = path.join(
      process.cwd(),
      'tests',
      'e2e',
      'test-results',
      'data'
    );
  }

  static getInstance(): TestDataLifecycleManager {
    if (!TestDataLifecycleManager.instance) {
      TestDataLifecycleManager.instance = new TestDataLifecycleManager();
    }
    return TestDataLifecycleManager.instance;
  }

  // Start a new test session with isolated data
  async startTestSession(
    page: Page,
    sessionName: string,
    config: DataIsolationConfig = {
      isolateUsers: true,
      isolateWallets: true,
      isolateTransactions: true,
      isolateDevices: true,
      useRandomPrefixes: true,
      cleanup: 'test-end',
    }
  ): Promise<string> {
    const sessionId = this.generateSessionId(sessionName);
    console.log(`Starting test session: ${sessionId}`);

    // Generate isolated test data
    const testData = await this.generateIsolatedTestData(sessionId, config);

    // Setup mock services
    const mockManager = new MockServiceManager(page);
    await mockManager.setupMockServices();

    // Create test session
    const session: TestSession = {
      id: sessionId,
      name: sessionName,
      startTime: new Date(),
      testData,
      mockServices: mockManager.getActiveMocks(),
      cleanupTasks: [],
      status: 'active',
    };

    // Create initial data snapshot
    session.dataSnapshot = await this.dataManager.createDataSnapshot(sessionId);

    // Seed data into application
    await this.dataManager.seedData(page, testData);

    // Validate data consistency
    const isValid = await this.dataManager.validateData(page, testData);
    if (!isValid) {
      throw new Error(`Data validation failed for session: ${sessionId}`);
    }

    this.activeSessions.set(sessionId, session);

    // Setup automatic cleanup if configured
    if (config.cleanup !== 'manual') {
      this.scheduleCleanup(sessionId, config.cleanup);
    }

    console.log(`Test session ${sessionId} started successfully`);
    return sessionId;
  }

  // End test session and cleanup
  async endTestSession(sessionId: string): Promise<void> {
    const session = this.activeSessions.get(sessionId);
    if (!session) {
      console.warn(`Session not found: ${sessionId}`);
      return;
    }

    console.log(`Ending test session: ${sessionId}`);

    try {
      // Execute cleanup tasks
      for (const cleanupTask of session.cleanupTasks) {
        try {
          await cleanupTask();
        } catch (error) {
          console.error(`Cleanup task failed for session ${sessionId}:`, error);
        }
      }

      // Clean up test data
      await this.dataManager.cleanup(sessionId);

      // Update session status
      session.status = 'completed';
      session.endTime = new Date();

      // Archive session data
      await this.archiveSession(session);
    } catch (error) {
      session.status = 'failed';
      console.error(`Failed to end session ${sessionId}:`, error);
      throw error;
    } finally {
      this.activeSessions.delete(sessionId);
    }

    console.log(`Test session ${sessionId} ended successfully`);
  }

  // Create a recovery point during test execution
  async createRecoveryPoint(
    sessionId: string,
    page: Page,
    description: string
  ): Promise<string> {
    const session = this.activeSessions.get(sessionId);
    if (!session) {
      throw new Error(`Session not found: ${sessionId}`);
    }

    const recoveryId = `recovery_${sessionId}_${Date.now()}_${crypto.randomBytes(4).toString('hex')}`;

    // Create data snapshot
    const dataSnapshot = await this.dataManager.createDataSnapshot(sessionId);

    // Capture page state
    const pageState = await this.capturePageState(page);

    const recoveryPoint: DataRecoveryPoint = {
      id: recoveryId,
      sessionId,
      timestamp: new Date(),
      description,
      dataSnapshot,
      pageState,
    };

    this.recoveryPoints.set(recoveryId, recoveryPoint);

    console.log(`Recovery point created: ${recoveryId} - ${description}`);
    return recoveryId;
  }

  // Restore to a previous recovery point
  async restoreToRecoveryPoint(recoveryId: string, page: Page): Promise<void> {
    const recoveryPoint = this.recoveryPoints.get(recoveryId);
    if (!recoveryPoint) {
      throw new Error(`Recovery point not found: ${recoveryId}`);
    }

    console.log(`Restoring to recovery point: ${recoveryId}`);

    // Restore data state
    await this.dataManager.restoreFromSnapshot(
      recoveryPoint.dataSnapshot,
      page
    );

    // Restore page state if available
    if (recoveryPoint.pageState) {
      await this.restorePageState(page, recoveryPoint.pageState);
    }

    console.log(`Successfully restored to recovery point: ${recoveryId}`);
  }

  // Generate isolated test data
  private async generateIsolatedTestData(
    sessionId: string,
    config: DataIsolationConfig
  ): Promise<TestDataCategory> {
    const prefix = config.useRandomPrefixes
      ? `${sessionId}_${crypto.randomBytes(4).toString('hex')}_`
      : `${sessionId}_`;

    const testData = await this.dataManager.generateTestData(sessionId, {
      userPrefix: config.isolateUsers ? prefix : '',
      walletPrefix: config.isolateWallets ? prefix : '',
      devicePrefix: config.isolateDevices ? prefix : '',
      isolateTransactions: config.isolateTransactions,
    });

    return testData;
  }

  // Capture current page state
  private async capturePageState(page: Page): Promise<any> {
    return await page.evaluate(() => {
      return {
        url: window.location.href,
        localStorage: { ...localStorage },
        sessionStorage: { ...sessionStorage },
        cookies: document.cookie,
        title: document.title,
        viewport: {
          width: window.innerWidth,
          height: window.innerHeight,
        },
      };
    });
  }

  // Restore page state
  private async restorePageState(page: Page, pageState: any): Promise<void> {
    // Navigate to the saved URL
    if (pageState.url && page.url() !== pageState.url) {
      await page.goto(pageState.url);
    }

    // Restore localStorage
    if (pageState.localStorage) {
      await page.evaluate(storage => {
        localStorage.clear();
        for (const [key, value] of Object.entries(storage)) {
          localStorage.setItem(key, value as string);
        }
      }, pageState.localStorage);
    }

    // Restore sessionStorage
    if (pageState.sessionStorage) {
      await page.evaluate(storage => {
        sessionStorage.clear();
        for (const [key, value] of Object.entries(storage)) {
          sessionStorage.setItem(key, value as string);
        }
      }, pageState.sessionStorage);
    }

    // Restore viewport if different
    if (pageState.viewport) {
      await page.setViewportSize({
        width: pageState.viewport.width,
        height: pageState.viewport.height,
      });
    }
  }

  // Generate unique session ID
  private generateSessionId(sessionName: string): string {
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const hash = crypto
      .createHash('sha256')
      .update(sessionName)
      .digest('hex')
      .substring(0, 8);
    return `session_${sessionName}_${timestamp}_${hash}`;
  }

  // Schedule automatic cleanup
  private scheduleCleanup(
    sessionId: string,
    when: 'immediate' | 'test-end'
  ): void {
    if (when === 'immediate') {
      // Cleanup after current test completes
      setTimeout(() => {
        this.endTestSession(sessionId).catch(console.error);
      }, 0);
    }
    // 'test-end' cleanup is handled by the test framework
  }

  // Archive completed session
  private async archiveSession(session: TestSession): Promise<void> {
    try {
      await fs.mkdir(this.baseDataPath, { recursive: true });

      const archiveFile = path.join(this.baseDataPath, `${session.id}.json`);
      const archiveData = {
        ...session,
        testData: undefined, // Don't store full test data in archive
        archivedAt: new Date().toISOString(),
      };

      await fs.writeFile(archiveFile, JSON.stringify(archiveData, null, 2));
      console.log(`Session archived: ${archiveFile}`);
    } catch (error) {
      console.error(`Failed to archive session ${session.id}:`, error);
    }
  }

  // Get active session info
  getActiveSession(sessionId: string): TestSession | undefined {
    return this.activeSessions.get(sessionId);
  }

  // List all active sessions
  getActiveSessions(): TestSession[] {
    return Array.from(this.activeSessions.values());
  }

  // Emergency cleanup - force cleanup all active sessions
  async emergencyCleanup(): Promise<void> {
    console.log('Performing emergency cleanup of all active sessions...');

    const cleanupPromises = Array.from(this.activeSessions.keys()).map(
      sessionId =>
        this.endTestSession(sessionId).catch(error => {
          console.error(
            `Emergency cleanup failed for session ${sessionId}:`,
            error
          );
        })
    );

    await Promise.all(cleanupPromises);
    console.log('Emergency cleanup completed');
  }

  // Data consistency validation
  async validateSessionData(sessionId: string, page: Page): Promise<boolean> {
    const session = this.activeSessions.get(sessionId);
    if (!session) {
      return false;
    }

    return await this.dataManager.validateData(page, session.testData);
  }

  // Add cleanup task to session
  addCleanupTask(sessionId: string, task: () => Promise<void>): void {
    const session = this.activeSessions.get(sessionId);
    if (session) {
      session.cleanupTasks.push(task);
    }
  }

  // Performance monitoring for data operations
  async measureDataOperation<T>(
    operationName: string,
    operation: () => Promise<T>
  ): Promise<{ result: T; duration: number; memoryUsage: any }> {
    const startTime = Date.now();
    const startMemory = process.memoryUsage();

    try {
      const result = await operation();
      const endTime = Date.now();
      const endMemory = process.memoryUsage();

      const duration = endTime - startTime;
      const memoryDelta = {
        heapUsed: endMemory.heapUsed - startMemory.heapUsed,
        heapTotal: endMemory.heapTotal - startMemory.heapTotal,
        external: endMemory.external - startMemory.external,
        rss: endMemory.rss - startMemory.rss,
      };

      console.log(
        `Data operation '${operationName}' completed in ${duration}ms, memory delta:`,
        memoryDelta
      );

      return { result, duration, memoryUsage: memoryDelta };
    } catch (error) {
      const endTime = Date.now();
      console.error(
        `Data operation '${operationName}' failed after ${endTime - startTime}ms:`,
        error
      );
      throw error;
    }
  }

  // Health check for data management system
  async healthCheck(): Promise<{
    status: 'healthy' | 'degraded' | 'unhealthy';
    activeSessions: number;
    recoveryPoints: number;
    issues: string[];
  }> {
    const issues: string[] = [];
    let status: 'healthy' | 'degraded' | 'unhealthy' = 'healthy';

    // Check active sessions
    const activeSessions = this.activeSessions.size;
    const recoveryPoints = this.recoveryPoints.size;

    if (activeSessions > 10) {
      issues.push(`High number of active sessions: ${activeSessions}`);
      status = 'degraded';
    }

    if (recoveryPoints > 50) {
      issues.push(`High number of recovery points: ${recoveryPoints}`);
      status = 'degraded';
    }

    // Check memory usage
    const memUsage = process.memoryUsage();
    if (memUsage.heapUsed > 500 * 1024 * 1024) {
      // 500MB
      issues.push(
        `High memory usage: ${Math.round(memUsage.heapUsed / 1024 / 1024)}MB`
      );
      status = 'degraded';
    }

    // Check for stuck sessions (running > 1 hour)
    const now = Date.now();
    const stuckSessions = Array.from(this.activeSessions.values()).filter(
      session => now - session.startTime.getTime() > 60 * 60 * 1000
    );

    if (stuckSessions.length > 0) {
      issues.push(`Found ${stuckSessions.length} stuck sessions`);
      status = status === 'healthy' ? 'degraded' : 'unhealthy';
    }

    // Check data path accessibility
    try {
      await fs.access(this.baseDataPath);
    } catch {
      issues.push('Data path not accessible');
      status = 'unhealthy';
    }

    return {
      status,
      activeSessions,
      recoveryPoints,
      issues,
    };
  }

  // Cleanup old recovery points
  async cleanupOldRecoveryPoints(
    maxAge: number = 24 * 60 * 60 * 1000
  ): Promise<void> {
    const cutoffTime = Date.now() - maxAge;
    const toDelete: string[] = [];

    for (const [id, recoveryPoint] of this.recoveryPoints) {
      if (recoveryPoint.timestamp.getTime() < cutoffTime) {
        toDelete.push(id);
      }
    }

    for (const id of toDelete) {
      this.recoveryPoints.delete(id);
    }

    if (toDelete.length > 0) {
      console.log(`Cleaned up ${toDelete.length} old recovery points`);
    }
  }
}

// Global cleanup handler for process termination
process.on('SIGINT', async () => {
  console.log('Received SIGINT, performing emergency cleanup...');
  await TestDataLifecycleManager.getInstance().emergencyCleanup();
  process.exit(0);
});

process.on('SIGTERM', async () => {
  console.log('Received SIGTERM, performing emergency cleanup...');
  await TestDataLifecycleManager.getInstance().emergencyCleanup();
  process.exit(0);
});

export default TestDataLifecycleManager;
