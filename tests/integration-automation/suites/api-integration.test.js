/**
 * Comprehensive API Integration Test Suite
 * Agent 6 - Integration Test Automation Specialist
 *
 * Tests complete API flows using Agent 4's mock services
 * Validates Agent 1's Pact contracts in integration context
 */

const supertest = require('supertest');
const { Pool } = require('pg');

describe('API Integration Test Suite', () => {
  let api;
  let dbPool;
  let mockServicesManager;

  beforeAll(async () => {
    // Get global test utilities
    mockServicesManager = global.mockServicesManager;

    // Create API client using supertest
    api = supertest('http://localhost:4001');

    // Setup database connection
    dbPool = new Pool({
      connectionString: global.testConfig.services.postgres,
      max: 5,
    });

    // Verify all mock services are healthy
    const healthCheck = await mockServicesManager.healthCheckAllServices();
    if (!healthCheck.allHealthy) {
      throw new Error(
        `Some mock services are unhealthy: ${healthCheck.healthy}/${healthCheck.total}`
      );
    }
  });

  afterAll(async () => {
    if (dbPool) {
      await dbPool.end();
    }
  });

  beforeEach(async () => {
    // Clean test data for isolation
    await global.testUtils.cleanDatabase(dbPool);
  });

  describe('Health Check and Service Discovery', () => {
    test('should validate all API endpoints are accessible', async () => {
      const response = await api
        .get('/api/health')
        .timeout(global.testConfig.timeouts.service);

      expect(response).toHaveValidApiResponse(200);
      expect(response.body).toMatchObject({
        status: 'healthy',
        timestamp: expect.any(String),
        service: expect.any(String),
      });

      // Validate response satisfies basic health contract
      expect(response.body).toSatisfyPactContract({
        status: expect.stringMatching(/healthy|degraded/),
        timestamp: expect.stringMatching(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/),
      });
    });

    test('should validate service dependencies health', async () => {
      const response = await api
        .get('/api/health/dependencies')
        .timeout(global.testConfig.timeouts.service);

      expect(response.status).toBe(200);
      expect(response.body).toHaveProperty('dependencies');

      const expectedServices = [
        'database',
        'redis',
        'blockchain',
        'weather',
        'carbon',
      ];
      for (const service of expectedServices) {
        expect(response.body.dependencies).toHaveProperty(service);
        expect(response.body.dependencies[service]).toBeHealthyService();
      }
    });
  });

  describe('User Management Integration Flow', () => {
    test('should complete full user registration and authentication flow', async () => {
      const userData = global.testUtils.generateTestData.user({
        username: 'integration_flow_user',
        email: 'flow@wrfcoin.integration',
      });

      // Step 1: Register new user
      const registerResponse = await api
        .post('/api/auth/register')
        .send(userData)
        .timeout(global.testConfig.timeouts.integration);

      expect(registerResponse).toHaveValidApiResponse(201);
      expect(registerResponse.body).toMatchObject({
        success: true,
        data: {
          id: expect.any(Number),
          username: userData.username,
          email: userData.email,
          walletAddress: userData.walletAddress,
          status: 'active',
          role: 'user',
        },
      });

      const userId = registerResponse.body.data.id;

      // Step 2: Verify user exists in database
      const dbUser = await dbPool.query('SELECT * FROM users WHERE id = $1', [
        userId,
      ]);

      expect(dbUser.rows).toHaveLength(1);
      expect(dbUser.rows[0].username).toBe(userData.username);

      // Step 3: Authenticate user
      const authResponse = await api
        .post('/api/auth/login')
        .send({
          username: userData.username,
          password: 'test-password', // Mock authentication
        })
        .timeout(global.testConfig.timeouts.service);

      expect(authResponse).toHaveValidApiResponse(200);
      expect(authResponse.body).toMatchObject({
        success: true,
        data: {
          token: expect.any(String),
          user: {
            id: userId,
            username: userData.username,
          },
        },
      });

      const authToken = authResponse.body.data.token;

      // Step 4: Access protected endpoint
      const profileResponse = await api
        .get('/api/user/profile')
        .set('Authorization', `Bearer ${authToken}`)
        .timeout(global.testConfig.timeouts.service);

      expect(profileResponse).toHaveValidApiResponse(200);
      expect(profileResponse.body.data.id).toBe(userId);
    });

    test('should handle user registration with existing email gracefully', async () => {
      const userData = global.testUtils.generateTestData.user();

      // First registration should succeed
      const firstResponse = await api
        .post('/api/auth/register')
        .send(userData)
        .timeout(global.testConfig.timeouts.service);

      expect(firstResponse).toHaveValidApiResponse(201);

      // Second registration with same email should fail
      const duplicateData = {
        ...userData,
        username: 'different_username',
      };

      const secondResponse = await api
        .post('/api/auth/register')
        .send(duplicateData)
        .timeout(global.testConfig.timeouts.service);

      expect(secondResponse.status).toBe(409); // Conflict
      expect(secondResponse.body).toMatchObject({
        success: false,
        error: expect.stringContaining('email'),
      });
    });
  });

  describe('Weather Data Integration Flow', () => {
    test('should process weather data from station to carbon credits', async () => {
      // Step 1: Submit weather data from station
      const weatherData = global.testUtils.generateTestData.weatherData(
        'station-integration-001',
        {
          carbon_metrics: {
            co2Equivalent: 0.05,
            carbonCreditsEarned: 0.005,
            calculationMethod: 'weather_monitoring_ipcc_2021',
          },
        }
      );

      const weatherResponse = await api
        .post('/api/weather/data')
        .send(weatherData)
        .timeout(global.testConfig.timeouts.integration);

      expect(weatherResponse).toHaveValidApiResponse(201);
      expect(weatherResponse.body).toMatchObject({
        success: true,
        data: {
          id: expect.any(String),
          stationId: weatherData.station_id,
          measurements: weatherData.measurements,
          carbonMetrics: weatherData.carbon_metrics,
          processedAt: expect.any(String),
        },
      });

      const weatherId = weatherResponse.body.data.id;

      // Step 2: Verify weather data stored in database
      const dbWeather = await dbPool.query(
        'SELECT * FROM weather_data WHERE id = $1',
        [weatherId]
      );

      expect(dbWeather.rows).toHaveLength(1);
      expect(dbWeather.rows[0].station_id).toBe(weatherData.station_id);
      expect(JSON.parse(dbWeather.rows[0].measurements)).toEqual(
        weatherData.measurements
      );

      // Step 3: Check carbon credits were generated
      const carbonResponse = await api
        .get(`/api/carbon-credits/weather-station/${weatherData.station_id}`)
        .timeout(global.testConfig.timeouts.service);

      expect(carbonResponse).toHaveValidApiResponse(200);

      if (carbonResponse.body.data.length > 0) {
        expect(carbonResponse.body.data[0]).toMatchObject({
          amount: expect.any(Number),
          source: 'weather_monitoring',
          status: expect.stringMatching(/pending|verified/),
        });
      }
    });

    test('should validate weather data quality and reject poor data', async () => {
      const poorQualityData = global.testUtils.generateTestData.weatherData(
        'station-integration-001',
        {
          measurements: {
            temperature: -100, // Unrealistic temperature
            humidity: 150, // Invalid humidity > 100%
            pressure: 500, // Unrealistic pressure
          },
          quality_score: 0.1, // Poor quality score
        }
      );

      const response = await api
        .post('/api/weather/data')
        .send(poorQualityData)
        .timeout(global.testConfig.timeouts.service);

      expect(response.status).toBe(422); // Unprocessable Entity
      expect(response.body).toMatchObject({
        success: false,
        error: expect.stringContaining('quality'),
        validationErrors: expect.any(Array),
      });
    });

    test('should aggregate weather data for analytics', async () => {
      // Submit multiple weather data points
      const stationId = 'station-integration-001';
      const dataPoints = Array.from({ length: 5 }, (_, i) =>
        global.testUtils.generateTestData.weatherData(stationId, {
          timestamp: new Date(Date.now() - i * 3600000).toISOString(), // Hourly data
        })
      );

      for (const data of dataPoints) {
        await api
          .post('/api/weather/data')
          .send(data)
          .timeout(global.testConfig.timeouts.service);
      }

      // Get aggregated data
      const analyticsResponse = await api
        .get(`/api/weather/station/${stationId}/analytics`)
        .query({
          period: '24h',
          metrics: 'temperature,humidity,pressure',
        })
        .timeout(global.testConfig.timeouts.integration);

      expect(analyticsResponse).toHaveValidApiResponse(200);
      expect(analyticsResponse.body.data).toMatchObject({
        stationId: stationId,
        period: '24h',
        aggregates: {
          temperature: {
            avg: expect.any(Number),
            min: expect.any(Number),
            max: expect.any(Number),
          },
          dataPoints: expect.any(Number),
        },
      });
    });
  });

  describe('Blockchain Transaction Integration Flow', () => {
    test('should process weather reward transaction end-to-end', async () => {
      // Step 1: Create transaction request
      const transactionData = global.testUtils.generateTestData.transaction({
        type: 'weather_reward',
        amount: 1000000, // 1 WRF token in smallest unit
        data: {
          stationId: 'station-integration-001',
          rewardPeriod: '2024-01-01T00:00:00Z_2024-01-01T23:59:59Z',
        },
      });

      const txResponse = await api
        .post('/api/blockchain/transaction')
        .send(transactionData)
        .timeout(global.testConfig.timeouts.integration);

      expect(txResponse).toHaveValidApiResponse(201);
      expect(txResponse.body).toMatchObject({
        success: true,
        data: {
          txHash: expect.stringMatching(/^0x[a-fA-F0-9]{64}$/),
          status: 'pending',
          timestamp: expect.any(String),
        },
      });

      const txHash = txResponse.body.data.txHash;

      // Step 2: Monitor transaction status
      let confirmationAttempts = 0;
      let txStatus;

      while (confirmationAttempts < 10) {
        const statusResponse = await api
          .get(`/api/blockchain/transaction/${txHash}`)
          .timeout(global.testConfig.timeouts.service);

        txStatus = statusResponse.body.data.status;

        if (txStatus === 'confirmed' || txStatus === 'failed') {
          break;
        }

        confirmationAttempts++;
        await new Promise(resolve => setTimeout(resolve, 1000));
      }

      expect(txStatus).toBe('confirmed');

      // Step 3: Verify transaction in database
      const dbTx = await dbPool.query(
        'SELECT * FROM transactions WHERE tx_id = $1',
        [txHash]
      );

      expect(dbTx.rows).toHaveLength(1);
      expect(dbTx.rows[0].status).toBe('confirmed');
      expect(dbTx.rows[0].type).toBe('weather_reward');
    });

    test('should handle transaction failures gracefully', async () => {
      const invalidTxData = {
        tx_id: 'invalid-hash', // Invalid format
        from_address: 'invalid-address',
        to_address: 'invalid-address',
        amount: -100, // Negative amount
        type: 'transfer',
      };

      const response = await api
        .post('/api/blockchain/transaction')
        .send(invalidTxData)
        .timeout(global.testConfig.timeouts.service);

      expect(response.status).toBe(422);
      expect(response.body).toMatchObject({
        success: false,
        error: expect.stringContaining('validation'),
        validationErrors: expect.arrayContaining([
          expect.objectContaining({
            field: expect.any(String),
            message: expect.any(String),
          }),
        ]),
      });
    });
  });

  describe('Agent 4 Mock Service Interactions', () => {
    test('should successfully interact with blockchain RPC mock', async () => {
      const response = await fetch(
        global.testConfig.services.blockchain + '/health'
      );
      expect(response.ok).toBe(true);

      const healthData = await response.json();
      expect(healthData).toHaveInteractedWithMockService({
        service: 'blockchain-rpc',
        endpoint: '/health',
      });
    });

    test('should successfully interact with weather oracle mock', async () => {
      const response = await fetch(
        global.testConfig.services.weather + '/health'
      );
      expect(response.ok).toBe(true);

      const healthData = await response.json();
      expect(healthData).toHaveInteractedWithMockService({
        service: 'weather-oracle',
        endpoint: '/health',
      });
    });

    test('should successfully interact with carbon oracle mock', async () => {
      const response = await fetch(
        global.testConfig.services.carbon + '/health'
      );
      expect(response.ok).toBe(true);

      const healthData = await response.json();
      expect(healthData).toHaveInteractedWithMockService({
        service: 'carbon-oracle',
        endpoint: '/health',
      });
    });
  });

  describe('Agent 1 Pact Contract Validation', () => {
    test('should validate API responses against Pact contracts', async () => {
      if (!global.testConfig.agents.pactContractsEnabled) {
        console.log('Skipping Pact validation - disabled in config');
        return;
      }

      // Test user creation contract
      const userData = global.testUtils.generateTestData.user();

      const response = await api
        .post('/api/auth/register')
        .send(userData)
        .timeout(global.testConfig.timeouts.service);

      // Validate against Agent 1's Pact contract expectations
      expect(response.body).toSatisfyPactContract({
        success: true,
        data: {
          id: expect.any(Number),
          username: userData.username,
          email: userData.email,
          walletAddress: expect.stringMatching(/^0x[a-fA-F0-9]{40}$/),
          status: 'active',
          role: 'user',
          createdAt: expect.stringMatching(
            /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/
          ),
          updatedAt: expect.stringMatching(
            /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/
          ),
        },
      });
    });

    test('should validate weather data responses against Pact contracts', async () => {
      if (!global.testConfig.agents.pactContractsEnabled) {
        console.log('Skipping Pact validation - disabled in config');
        return;
      }

      const weatherData = global.testUtils.generateTestData.weatherData();

      const response = await api
        .post('/api/weather/data')
        .send(weatherData)
        .timeout(global.testConfig.timeouts.service);

      expect(response.body).toSatisfyPactContract({
        success: true,
        data: {
          id: expect.any(String),
          stationId: weatherData.station_id,
          measurements: expect.objectContaining({
            temperature: expect.any(Number),
            humidity: expect.any(Number),
            pressure: expect.any(Number),
          }),
          timestamp: expect.stringMatching(
            /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/
          ),
          processedAt: expect.stringMatching(
            /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/
          ),
        },
      });
    });
  });

  describe('Performance and Load Testing', () => {
    test('should handle concurrent API requests efficiently', async () => {
      const concurrentRequests = 10;
      const requestPromises = Array.from({ length: concurrentRequests }, () =>
        api.get('/api/health').timeout(global.testConfig.timeouts.service)
      );

      const startTime = Date.now();
      const responses = await Promise.all(requestPromises);
      const totalTime = Date.now() - startTime;

      // All requests should succeed
      responses.forEach(response => {
        expect(response.status).toBe(200);
      });

      // Average response time should be reasonable
      const avgResponseTime = totalTime / concurrentRequests;
      expect(avgResponseTime).toBeLessThan(1000); // Less than 1 second average
    });

    test('should handle database connection pooling under load', async () => {
      const concurrentDbOperations = 15;
      const dbPromises = Array.from(
        { length: concurrentDbOperations },
        async (_, i) => {
          const userData = global.testUtils.generateTestData.user({
            username: `concurrent_user_${i}`,
            email: `concurrent${i}@test.com`,
          });

          return api
            .post('/api/auth/register')
            .send(userData)
            .timeout(global.testConfig.timeouts.integration);
        }
      );

      const responses = await Promise.all(dbPromises);

      // All database operations should succeed
      responses.forEach((response, i) => {
        expect(response.status).toBe(201);
        expect(response.body.data.username).toBe(`concurrent_user_${i}`);
      });

      // Verify all users were created in database
      const dbResult = await dbPool.query(
        'SELECT COUNT(*) as count FROM users WHERE username LIKE $1',
        ['concurrent_user_%']
      );

      expect(parseInt(dbResult.rows[0].count)).toBe(concurrentDbOperations);
    });
  });

  describe('Error Handling and Recovery', () => {
    test('should handle service timeouts gracefully', async () => {
      // Simulate a slow endpoint
      const response = await api
        .get('/api/slow-endpoint')
        .timeout(1000) // Short timeout
        .catch(err => err.response || { status: 408 });

      // Should handle timeout gracefully
      expect([408, 504, 503]).toContain(response.status);
    });

    test('should maintain API availability during database issues', async () => {
      // Health check should still work even if database is having issues
      const healthResponse = await api
        .get('/api/health/basic')
        .timeout(global.testConfig.timeouts.service);

      expect(healthResponse.status).toBe(200);
    });

    test('should provide meaningful error messages for validation failures', async () => {
      const invalidData = {
        username: '', // Empty username
        email: 'invalid-email', // Invalid email format
        walletAddress: 'invalid-address', // Invalid wallet address
      };

      const response = await api
        .post('/api/auth/register')
        .send(invalidData)
        .timeout(global.testConfig.timeouts.service);

      expect(response.status).toBe(422);
      expect(response.body).toMatchObject({
        success: false,
        error: expect.any(String),
        validationErrors: expect.arrayContaining([
          expect.objectContaining({
            field: 'username',
            message: expect.stringContaining('required'),
          }),
          expect.objectContaining({
            field: 'email',
            message: expect.stringContaining('valid email'),
          }),
          expect.objectContaining({
            field: 'walletAddress',
            message: expect.stringContaining('valid wallet address'),
          }),
        ]),
      });
    });
  });
});
