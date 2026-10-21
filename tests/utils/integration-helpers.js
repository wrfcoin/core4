/**
 * Integration Test Helpers
 * Utilities for setting up and managing integration test environments
 */

const { execSync } = require('child_process');
const fs = require('fs').promises;
const path = require('path');

class IntegrationTestHelper {
  constructor() {
    this.testDatabases = new Map();
    this.testServices = new Map();
    this.cleanup = [];
  }

  /**
   * Setup test database with unique name
   */
  async setupTestDatabase(testName) {
    const dbName = `wrfcoin_test_${testName}_${Date.now()}`;
    const dbUrl =
      process.env.DATABASE_URL?.replace(/\/[^\/]*$/, '') ||
      'postgresql://localhost:5432';

    try {
      // Create test database
      execSync(`createdb ${dbName}`, { stdio: 'pipe' });

      // Run migrations
      const testDbUrl = `${dbUrl}/${dbName}`;
      process.env[`TEST_DATABASE_URL_${testName}`] = testDbUrl;

      this.testDatabases.set(testName, {
        name: dbName,
        url: testDbUrl,
      });

      this.cleanup.push(() => this.cleanupTestDatabase(testName));

      return testDbUrl;
    } catch (error) {
      console.error(
        `Failed to setup test database for ${testName}:`,
        error.message
      );
      throw error;
    }
  }

  /**
   * Cleanup test database
   */
  async cleanupTestDatabase(testName) {
    const db = this.testDatabases.get(testName);
    if (db) {
      try {
        execSync(`dropdb ${db.name}`, { stdio: 'pipe' });
        this.testDatabases.delete(testName);
        delete process.env[`TEST_DATABASE_URL_${testName}`];
      } catch (error) {
        console.warn(
          `Failed to cleanup test database ${db.name}:`,
          error.message
        );
      }
    }
  }

  /**
   * Start test service (Redis, etc.)
   */
  async startTestService(serviceName, config = {}) {
    switch (serviceName) {
      case 'redis':
        return this.startRedisService(config);
      case 'ipfs':
        return this.startIPFSService(config);
      default:
        throw new Error(`Unknown service: ${serviceName}`);
    }
  }

  /**
   * Start Redis test instance
   */
  async startRedisService(config) {
    const port = config.port || 6380; // Different from default 6379
    const { spawn } = require('child_process');

    const redis = spawn(
      'redis-server',
      [
        '--port',
        port.toString(),
        '--save',
        '', // No persistence
        '--appendonly',
        'no',
      ],
      {
        stdio: 'pipe',
      }
    );

    // Wait for Redis to start
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error('Redis startup timeout'));
      }, 10000);

      redis.on('error', reject);

      // Check if Redis is ready
      const checkRedis = setInterval(() => {
        try {
          execSync(`redis-cli -p ${port} ping`, { stdio: 'pipe' });
          clearTimeout(timeout);
          clearInterval(checkRedis);
          resolve();
        } catch (error) {
          // Redis not ready yet
        }
      }, 500);
    });

    this.testServices.set('redis', { process: redis, port });
    this.cleanup.push(() => this.stopTestService('redis'));

    process.env.TEST_REDIS_URL = `redis://localhost:${port}`;

    return { url: `redis://localhost:${port}`, port };
  }

  /**
   * Start IPFS test instance
   */
  async startIPFSService(config) {
    // For now, use mock IPFS or skip if not available
    process.env.TEST_IPFS_URL = 'http://localhost:5001';
    return { url: 'http://localhost:5001' };
  }

  /**
   * Stop test service
   */
  async stopTestService(serviceName) {
    const service = this.testServices.get(serviceName);
    if (service?.process) {
      service.process.kill('SIGTERM');
      this.testServices.delete(serviceName);
    }
  }

  /**
   * Setup mock blockchain node
   */
  async setupMockBlockchain() {
    // Start Hardhat node in background for testing
    const { spawn } = require('child_process');

    const hardhatNode = spawn('npx', ['hardhat', 'node'], {
      stdio: 'pipe',
      cwd: process.cwd(),
    });

    // Wait for node to start
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error('Hardhat node startup timeout'));
      }, 30000);

      hardhatNode.stdout.on('data', data => {
        if (
          data.toString().includes('Started HTTP and WebSocket JSON-RPC server')
        ) {
          clearTimeout(timeout);
          resolve();
        }
      });

      hardhatNode.on('error', reject);
    });

    this.testServices.set('blockchain', { process: hardhatNode });
    this.cleanup.push(() => this.stopTestService('blockchain'));

    process.env.TEST_BLOCKCHAIN_URL = 'http://localhost:8545';

    return { url: 'http://localhost:8545' };
  }

  /**
   * Create test data fixtures
   */
  async createTestFixtures(testName, fixtures) {
    const fixturesDir = path.join(__dirname, '..', 'fixtures');
    await fs.mkdir(fixturesDir, { recursive: true });

    const fixtureFile = path.join(fixturesDir, `${testName}.json`);
    await fs.writeFile(fixtureFile, JSON.stringify(fixtures, null, 2));

    this.cleanup.push(() => this.cleanupTestFixtures(testName));

    return fixtureFile;
  }

  /**
   * Cleanup test fixtures
   */
  async cleanupTestFixtures(testName) {
    const fixtureFile = path.join(
      __dirname,
      '..',
      'fixtures',
      `${testName}.json`
    );
    try {
      await fs.unlink(fixtureFile);
    } catch (error) {
      // File doesn't exist, ignore
    }
  }

  /**
   * Wait for service to be ready
   */
  async waitForService(url, timeout = 30000) {
    const startTime = Date.now();

    while (Date.now() - startTime < timeout) {
      try {
        const response = await fetch(url);
        if (response.ok || response.status < 500) {
          return true;
        }
      } catch (error) {
        // Service not ready yet
      }

      await new Promise(resolve => setTimeout(resolve, 1000));
    }

    throw new Error(`Service at ${url} not ready within ${timeout}ms`);
  }

  /**
   * Cleanup all test resources
   */
  async cleanupAll() {
    console.log('🧹 Cleaning up integration test resources...');

    for (const cleanupFn of this.cleanup) {
      try {
        await cleanupFn();
      } catch (error) {
        console.warn('Cleanup warning:', error.message);
      }
    }

    this.cleanup.length = 0;
    this.testDatabases.clear();
    this.testServices.clear();
  }

  /**
   * Generate test report
   */
  generateTestReport(results) {
    return {
      timestamp: new Date().toISOString(),
      databases: Array.from(this.testDatabases.keys()),
      services: Array.from(this.testServices.keys()),
      results: results,
      cleanup: this.cleanup.length,
    };
  }
}

// Global helper instance
const globalHelper = new IntegrationTestHelper();

// Cleanup on process exit
process.on('exit', () => {
  globalHelper.cleanupAll().catch(console.error);
});

process.on('SIGINT', () => {
  globalHelper
    .cleanupAll()
    .then(() => process.exit(0))
    .catch(() => process.exit(1));
});

process.on('SIGTERM', () => {
  globalHelper
    .cleanupAll()
    .then(() => process.exit(0))
    .catch(() => process.exit(1));
});

module.exports = {
  IntegrationTestHelper,
  globalHelper,

  // Convenience functions
  setupTestDatabase: testName => globalHelper.setupTestDatabase(testName),
  startTestService: (serviceName, config) =>
    globalHelper.startTestService(serviceName, config),
  setupMockBlockchain: () => globalHelper.setupMockBlockchain(),
  createTestFixtures: (testName, fixtures) =>
    globalHelper.createTestFixtures(testName, fixtures),
  waitForService: (url, timeout) => globalHelper.waitForService(url, timeout),
  cleanupAll: () => globalHelper.cleanupAll(),
};
