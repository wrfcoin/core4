/**
 * Mock Services Setup for Integration Test Automation
 * Agent 6 - Integration Test Automation Specialist
 *
 * Integrates with Agent 4's comprehensive mock harness
 * Provides service discovery and health monitoring
 */

const { Pool } = require('pg');
const Redis = require('redis');

class MockServicesManager {
  constructor() {
    this.services = new Map();
    this.healthChecks = new Map();
    this.connectionPools = new Map();

    // Initialize service configurations from global test config
    this.initializeServices();
  }

  initializeServices() {
    const serviceConfigs = {
      postgres: {
        url: global.testConfig.services.postgres,
        healthEndpoint: null, // Direct connection check
        type: 'database',
        retries: 5,
      },
      redis: {
        url: global.testConfig.services.redis,
        healthEndpoint: null, // Direct connection check
        type: 'cache',
        retries: 3,
      },
      blockchain: {
        url: global.testConfig.services.blockchain,
        healthEndpoint: '/health',
        type: 'rpc',
        retries: 5,
      },
      weather: {
        url: global.testConfig.services.weather,
        healthEndpoint: '/health',
        type: 'api',
        retries: 3,
      },
      carbon: {
        url: global.testConfig.services.carbon,
        healthEndpoint: '/health',
        type: 'api',
        retries: 3,
      },
      mockserver: {
        url: global.testConfig.services.mockserver,
        healthEndpoint: '/mockserver/status',
        type: 'mock',
        retries: 3,
      },
    };

    for (const [name, config] of Object.entries(serviceConfigs)) {
      this.services.set(name, {
        ...config,
        status: 'unknown',
        lastCheck: null,
        failures: 0,
      });
    }
  }

  async setupConnectionPools() {
    console.log('🔗 Setting up connection pools for integration testing...');

    try {
      // PostgreSQL connection pool
      const pgPool = new Pool({
        connectionString: global.testConfig.services.postgres,
        max: 10,
        idleTimeoutMillis: 30000,
        connectionTimeoutMillis: 2000,
      });

      // Test PostgreSQL connection
      const client = await pgPool.connect();
      await client.query('SELECT 1');
      client.release();

      this.connectionPools.set('postgres', pgPool);
      console.log('   ✓ PostgreSQL connection pool ready');

      // Redis connection pool
      const redisClient = Redis.createClient({
        url: global.testConfig.services.redis,
        socket: {
          connectTimeout: 3000,
          lazyConnect: true,
        },
      });

      await redisClient.connect();
      await redisClient.ping();

      this.connectionPools.set('redis', redisClient);
      console.log('   ✓ Redis connection pool ready');
    } catch (error) {
      throw new Error(`Connection pool setup failed: ${error.message}`);
    }
  }

  async healthCheckAllServices() {
    console.log('🩺 Running comprehensive health checks on mock services...');

    const healthPromises = Array.from(this.services.keys()).map(serviceName =>
      this.healthCheckService(serviceName)
    );

    const results = await Promise.allSettled(healthPromises);
    const healthySevices = results.filter(
      result => result.status === 'fulfilled'
    ).length;
    const totalServices = results.length;

    console.log(
      `   📊 Health Check Results: ${healthySevices}/${totalServices} services healthy`
    );

    if (healthySevices < totalServices) {
      const failedServices = results
        .map((result, index) => ({
          result,
          name: Array.from(this.services.keys())[index],
        }))
        .filter(({ result }) => result.status === 'rejected')
        .map(
          ({ name, result }) =>
            `${name}: ${result.reason?.message || 'Unknown error'}`
        );

      console.warn('   ⚠️  Failed services:', failedServices);
    }

    return {
      healthy: healthySevices,
      total: totalServices,
      allHealthy: healthySevices === totalServices,
    };
  }

  async healthCheckService(serviceName) {
    const service = this.services.get(serviceName);
    if (!service) {
      throw new Error(`Unknown service: ${serviceName}`);
    }

    try {
      let isHealthy = false;
      const startTime = Date.now();

      switch (service.type) {
        case 'database':
          if (serviceName === 'postgres') {
            const pool = this.connectionPools.get('postgres');
            if (pool) {
              const client = await pool.connect();
              await client.query('SELECT 1');
              client.release();
              isHealthy = true;
            }
          }
          break;

        case 'cache':
          if (serviceName === 'redis') {
            const client = this.connectionPools.get('redis');
            if (client) {
              await client.ping();
              isHealthy = true;
            }
          }
          break;

        case 'api':
        case 'rpc':
        case 'mock':
          const healthUrl = service.url + service.healthEndpoint;
          const response = await fetch(healthUrl, {
            timeout: 3000,
            headers: {
              'User-Agent': 'WRFCoin-Integration-Test-Agent-6',
            },
          });
          isHealthy = response.ok;
          break;

        default:
          throw new Error(`Unknown service type: ${service.type}`);
      }

      const responseTime = Date.now() - startTime;

      service.status = 'healthy';
      service.lastCheck = new Date().toISOString();
      service.responseTime = responseTime;
      service.failures = 0;

      return { service: serviceName, healthy: true, responseTime };
    } catch (error) {
      service.status = 'unhealthy';
      service.lastCheck = new Date().toISOString();
      service.failures += 1;
      service.lastError = error.message;

      throw new Error(`${serviceName} health check failed: ${error.message}`);
    }
  }

  async setupMockServerExpectations() {
    console.log(
      '🎭 Setting up MockServer expectations for Agent 4 integration...'
    );

    try {
      const mockServerUrl = global.testConfig.services.mockserver;

      // Standard API response expectations
      const expectations = [
        {
          httpRequest: {
            method: 'GET',
            path: '/api/health',
          },
          httpResponse: {
            statusCode: 200,
            headers: {
              'Content-Type': ['application/json'],
            },
            body: JSON.stringify({
              status: 'healthy',
              timestamp: new Date().toISOString(),
              service: 'wrfcoin-api',
              version: '1.0.0',
            }),
          },
        },

        // Weather API expectations
        {
          httpRequest: {
            method: 'GET',
            path: '/api/weather/.*',
            pathParameters: {
              stationId: ['.*'],
            },
          },
          httpResponse: {
            statusCode: 200,
            headers: {
              'Content-Type': ['application/json'],
            },
            body: JSON.stringify({
              success: true,
              data: {
                stationId: 'station-integration-001',
                measurements: {
                  temperature: 22.5,
                  humidity: 65.0,
                  pressure: 1013.25,
                },
                timestamp: new Date().toISOString(),
              },
            }),
          },
        },

        // Blockchain API expectations
        {
          httpRequest: {
            method: 'POST',
            path: '/api/blockchain/transaction',
          },
          httpResponse: {
            statusCode: 201,
            headers: {
              'Content-Type': ['application/json'],
            },
            body: JSON.stringify({
              success: true,
              data: {
                txHash:
                  '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef',
                status: 'pending',
                timestamp: new Date().toISOString(),
              },
            }),
          },
        },
      ];

      const response = await fetch(`${mockServerUrl}/mockserver/expectation`, {
        method: 'PUT',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(expectations),
      });

      if (response.ok) {
        console.log('   ✓ MockServer expectations configured successfully');
      } else {
        console.warn('   ⚠️  MockServer expectations setup failed');
      }
    } catch (error) {
      console.warn(
        '   ⚠️  MockServer expectations setup error:',
        error.message
      );
      // Don't fail setup for MockServer issues
    }
  }

  async validateAgentIntegrations() {
    console.log('🔍 Validating Agent integrations...');

    const validations = [];

    // Agent 1 Pact Contract Integration
    if (global.testConfig.agents.pactContractsEnabled) {
      try {
        const fs = require('fs');
        const path = require('path');
        const contractPath = path.join(
          process.cwd(),
          global.testConfig.agents.agent1ContractPath
        );

        if (fs.existsSync(contractPath)) {
          console.log('   ✓ Agent 1 Pact contracts directory found');
          validations.push({ agent: 'Agent1-Pact', status: 'available' });
        } else {
          console.log('   ⚠️  Agent 1 Pact contracts directory not found');
          validations.push({ agent: 'Agent1-Pact', status: 'missing' });
        }
      } catch (error) {
        console.log(
          '   ❌ Agent 1 Pact integration validation failed:',
          error.message
        );
        validations.push({
          agent: 'Agent1-Pact',
          status: 'error',
          error: error.message,
        });
      }
    } else {
      console.log('   ⏭️  Agent 1 Pact contracts disabled');
      validations.push({ agent: 'Agent1-Pact', status: 'disabled' });
    }

    // Agent 4 Mock Services Integration
    if (global.testConfig.agents.mockServicesEnabled) {
      try {
        const healthResult = await this.healthCheckAllServices();

        if (healthResult.allHealthy) {
          console.log('   ✅ Agent 4 Mock Services all healthy');
          validations.push({
            agent: 'Agent4-Mocks',
            status: 'healthy',
            services: healthResult.total,
          });
        } else {
          console.log('   ⚠️  Agent 4 Mock Services partially healthy');
          validations.push({
            agent: 'Agent4-Mocks',
            status: 'partial',
            healthy: healthResult.healthy,
            total: healthResult.total,
          });
        }
      } catch (error) {
        console.log(
          '   ❌ Agent 4 Mock Services validation failed:',
          error.message
        );
        validations.push({
          agent: 'Agent4-Mocks',
          status: 'error',
          error: error.message,
        });
      }
    } else {
      console.log('   ⏭️  Agent 4 Mock Services disabled');
      validations.push({ agent: 'Agent4-Mocks', status: 'disabled' });
    }

    return validations;
  }

  getServiceStatus(serviceName) {
    return this.services.get(serviceName);
  }

  getAllServiceStatuses() {
    const statuses = {};
    for (const [name, service] of this.services.entries()) {
      statuses[name] = {
        status: service.status,
        lastCheck: service.lastCheck,
        failures: service.failures,
        responseTime: service.responseTime,
        lastError: service.lastError,
      };
    }
    return statuses;
  }

  async cleanup() {
    console.log('🧹 Cleaning up mock services connections...');

    try {
      // Close PostgreSQL pool
      const pgPool = this.connectionPools.get('postgres');
      if (pgPool) {
        await pgPool.end();
        console.log('   ✓ PostgreSQL pool closed');
      }

      // Close Redis connection
      const redisClient = this.connectionPools.get('redis');
      if (redisClient) {
        await redisClient.disconnect();
        console.log('   ✓ Redis connection closed');
      }
    } catch (error) {
      console.warn('   ⚠️  Cleanup warning:', error.message);
    }
  }
}

// Global instance for use in tests
global.mockServicesManager = new MockServicesManager();

// Setup hook for Jest
beforeAll(async () => {
  console.log('🚀 Initializing Mock Services for Integration Tests...');

  try {
    await global.mockServicesManager.setupConnectionPools();
    await global.mockServicesManager.setupMockServerExpectations();
    const validationResults =
      await global.mockServicesManager.validateAgentIntegrations();

    console.log('✅ Mock Services Integration Setup Complete');
    console.log('   Integration Validation Results:');
    validationResults.forEach(result => {
      const status =
        result.status === 'healthy'
          ? '✅'
          : result.status === 'available'
            ? '✅'
            : result.status === 'disabled'
              ? '⏭️'
              : result.status === 'partial'
                ? '⚠️'
                : '❌';
      console.log(`     ${status} ${result.agent}: ${result.status}`);
    });
  } catch (error) {
    console.error('❌ Mock Services setup failed:', error.message);
    throw error;
  }
});

// Cleanup hook for Jest
afterAll(async () => {
  if (global.mockServicesManager) {
    await global.mockServicesManager.cleanup();
  }
});

module.exports = MockServicesManager;
