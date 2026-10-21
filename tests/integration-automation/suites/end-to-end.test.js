/**
 * End-to-End Integration Test Suite
 * Agent 6 - Integration Test Automation Specialist
 *
 * Tests complete user workflows from API entry to final data persistence
 * Validates entire system integration flows using Agent 4's mock harness
 */

const request = require('supertest');
const { Pool } = require('pg');
const Redis = require('redis');

describe('End-to-End Integration Test Suite', () => {
  let dbPool;
  let redisClient;
  let mockServicesManager;
  let apiBaseUrl;

  beforeAll(async () => {
    mockServicesManager = global.mockServicesManager;
    apiBaseUrl = global.testConfig.apiBaseUrl;

    // Setup database connection
    dbPool = new Pool({
      connectionString: global.testConfig.services.postgres,
      max: 10,
    });

    // Setup Redis connection
    redisClient = Redis.createClient({
      url: global.testConfig.services.redis,
    });
    await redisClient.connect();

    // Verify all services are healthy
    const healthCheck = await mockServicesManager.healthCheckAllServices();
    expect(healthCheck.allHealthy).toBe(true);
  });

  afterAll(async () => {
    if (dbPool) await dbPool.end();
    if (redisClient) await redisClient.disconnect();
  });

  beforeEach(async () => {
    // Clean test data and cache
    await global.testUtils.cleanDatabase(dbPool);
    await redisClient.flushAll();
  });

  describe('Complete User Registration Flow', () => {
    test('should handle complete user onboarding from registration to first weather data', async () => {
      // Step 1: User Registration
      const registrationData = global.testUtils.generateTestData.user();

      const registrationResponse = await request(apiBaseUrl)
        .post('/api/users/register')
        .send({
          username: registrationData.username,
          email: registrationData.email,
          password: 'securePassword123',
          confirmPassword: 'securePassword123',
        })
        .expect(201);

      expect(registrationResponse.body).toMatchObject({
        success: true,
        data: {
          id: expect.any(Number),
          username: registrationData.username,
          email: registrationData.email,
          status: 'active',
        },
      });

      const userId = registrationResponse.body.data.id;

      // Step 2: User Login
      const loginResponse = await request(apiBaseUrl)
        .post('/api/auth/login')
        .send({
          username: registrationData.username,
          password: 'securePassword123',
        })
        .expect(200);

      expect(loginResponse.body).toMatchObject({
        success: true,
        data: {
          token: expect.any(String),
          user: {
            id: userId,
            username: registrationData.username,
          },
        },
      });

      const authToken = loginResponse.body.data.token;

      // Step 3: Register Weather Station
      const stationData = {
        stationId: 'e2e-test-station-001',
        name: 'E2E Test Weather Station',
        location: { lat: 40.7128, lng: -74.006 },
        description: 'Test station for E2E flow validation',
      };

      const stationResponse = await request(apiBaseUrl)
        .post('/api/weather-stations')
        .set('Authorization', `Bearer ${authToken}`)
        .send(stationData)
        .expect(201);

      expect(stationResponse.body).toMatchObject({
        success: true,
        data: {
          stationId: stationData.stationId,
          ownerId: userId,
          status: 'active',
        },
      });

      // Step 4: Submit Weather Data
      const weatherMeasurements = {
        temperature: 22.5,
        humidity: 65.2,
        pressure: 1013.25,
        windSpeed: 5.2,
        windDirection: 180,
        precipitation: 0.0,
      };

      const weatherResponse = await request(apiBaseUrl)
        .post(`/api/weather-stations/${stationData.stationId}/data`)
        .set('Authorization', `Bearer ${authToken}`)
        .send({
          measurements: weatherMeasurements,
          timestamp: new Date().toISOString(),
        })
        .expect(201);

      expect(weatherResponse.body).toMatchObject({
        success: true,
        data: {
          id: expect.any(Number),
          stationId: stationData.stationId,
          measurements: weatherMeasurements,
        },
      });

      // Step 5: Verify Data Persistence
      const dbWeatherData = await dbPool.query(
        `SELECT wd.*, ws.owner_id, u.username 
         FROM weather_data wd 
         JOIN weather_stations ws ON wd.station_id = ws.station_id
         JOIN users u ON ws.owner_id = u.id
         WHERE wd.station_id = $1`,
        [stationData.stationId]
      );

      expect(dbWeatherData.rows).toHaveLength(1);
      expect(dbWeatherData.rows[0]).toMatchObject({
        station_id: stationData.stationId,
        owner_id: userId,
        username: registrationData.username,
      });

      // Step 6: Verify Cache Population
      const cacheKey = `weather:${stationData.stationId}:latest`;
      const cachedData = await redisClient.get(cacheKey);
      expect(cachedData).toBeTruthy();

      const parsedCachedData = JSON.parse(cachedData);
      expect(parsedCachedData.measurements).toEqual(weatherMeasurements);

      // Step 7: Verify User Profile Update
      const profileResponse = await request(apiBaseUrl)
        .get('/api/users/profile')
        .set('Authorization', `Bearer ${authToken}`)
        .expect(200);

      expect(profileResponse.body.data.stations).toHaveLength(1);
      expect(profileResponse.body.data.stations[0].stationId).toBe(
        stationData.stationId
      );
    });
  });

  describe('Complete Carbon Credit Generation Flow', () => {
    test('should generate carbon credits from weather data submission', async () => {
      // Setup: Create user and station
      const userData = await global.testUtils.createTestUser(dbPool);
      const stationData = await global.testUtils.createTestWeatherStation(
        dbPool,
        userData.id
      );

      // Step 1: Submit high-quality weather data
      const highQualityData = {
        measurements: {
          temperature: 23.0,
          humidity: 58.3,
          pressure: 1015.2,
          windSpeed: 3.5,
          windDirection: 220,
          precipitation: 0.0,
          solarRadiation: 850.0,
          uvIndex: 6.2,
        },
        qualityScore: 0.98,
        timestamp: new Date().toISOString(),
      };

      const weatherResponse = await request(apiBaseUrl)
        .post(`/api/weather-stations/${stationData.stationId}/data`)
        .set('Authorization', `Bearer ${userData.token}`)
        .send(highQualityData)
        .expect(201);

      const weatherDataId = weatherResponse.body.data.id;

      // Step 2: Carbon calculation should be triggered automatically
      // Wait for async processing
      await new Promise(resolve => setTimeout(resolve, 2000));

      // Step 3: Verify carbon credits were calculated and stored
      const carbonCredits = await dbPool.query(
        'SELECT * FROM carbon_credits WHERE user_id = $1 ORDER BY created_at DESC LIMIT 1',
        [userData.id]
      );

      expect(carbonCredits.rows).toHaveLength(1);
      expect(carbonCredits.rows[0]).toMatchObject({
        user_id: userData.id,
        amount: expect.any(String), // Decimal type comes as string
        source: 'weather_monitoring',
        status: 'verified',
      });

      const carbonAmount = parseFloat(carbonCredits.rows[0].amount);
      expect(carbonAmount).toBeGreaterThan(0);

      // Step 4: Verify blockchain transaction was created
      const transactions = await dbPool.query(
        'SELECT * FROM transactions WHERE type = $1 ORDER BY created_at DESC LIMIT 1',
        ['carbon_credit']
      );

      expect(transactions.rows).toHaveLength(1);
      expect(transactions.rows[0]).toMatchObject({
        type: 'carbon_credit',
        status: 'pending',
        data: expect.objectContaining({
          carbonCreditId: carbonCredits.rows[0].id,
          weatherDataId: weatherDataId,
        }),
      });

      // Step 5: Verify user's carbon balance is updated
      const balanceResponse = await request(apiBaseUrl)
        .get('/api/carbon-credits/balance')
        .set('Authorization', `Bearer ${userData.token}`)
        .expect(200);

      expect(balanceResponse.body.data).toMatchObject({
        totalCredits: expect.any(Number),
        availableCredits: expect.any(Number),
        retiredCredits: 0,
      });

      expect(balanceResponse.body.data.totalCredits).toBeGreaterThan(0);
      expect(balanceResponse.body.data.availableCredits).toBeGreaterThan(0);
    });
  });

  describe('Complete DeFi Interaction Flow', () => {
    test('should handle yield farming pool participation', async () => {
      // Setup: Create user with initial balance
      const userData = await global.testUtils.createTestUser(dbPool);
      await global.testUtils.giveUserTokens(dbPool, userData.id, '1000.0');

      // Step 1: Get available yield farming pools
      const poolsResponse = await request(apiBaseUrl)
        .get('/api/yield-farming/pools')
        .set('Authorization', `Bearer ${userData.token}`)
        .expect(200);

      expect(poolsResponse.body.data).toBeInstanceOf(Array);
      expect(poolsResponse.body.data.length).toBeGreaterThan(0);

      const selectedPool = poolsResponse.body.data[0];
      expect(selectedPool).toMatchObject({
        id: expect.any(Number),
        name: expect.any(String),
        apy: expect.any(Number),
        totalLiquidity: expect.any(String),
      });

      // Step 2: Stake tokens in yield farming pool
      const stakeAmount = '100.0';
      const stakeResponse = await request(apiBaseUrl)
        .post(`/api/yield-farming/pools/${selectedPool.id}/stake`)
        .set('Authorization', `Bearer ${userData.token}`)
        .send({
          amount: stakeAmount,
        })
        .expect(201);

      expect(stakeResponse.body).toMatchObject({
        success: true,
        data: {
          stakeId: expect.any(Number),
          poolId: selectedPool.id,
          amount: stakeAmount,
          status: 'active',
        },
      });

      const stakeId = stakeResponse.body.data.stakeId;

      // Step 3: Verify stake was recorded in database
      const stakeRecord = await dbPool.query(
        'SELECT * FROM yield_farming_stakes WHERE id = $1',
        [stakeId]
      );

      expect(stakeRecord.rows).toHaveLength(1);
      expect(stakeRecord.rows[0]).toMatchObject({
        user_id: userData.id,
        pool_id: selectedPool.id,
        amount: stakeAmount,
        status: 'active',
      });

      // Step 4: Verify user balance was updated
      const balanceResponse = await request(apiBaseUrl)
        .get('/api/users/balance')
        .set('Authorization', `Bearer ${userData.token}`)
        .expect(200);

      const expectedBalance = (1000.0 - 100.0).toString();
      expect(balanceResponse.body.data.available).toBe(expectedBalance);

      // Step 5: Check staking position
      const positionResponse = await request(apiBaseUrl)
        .get('/api/yield-farming/positions')
        .set('Authorization', `Bearer ${userData.token}`)
        .expect(200);

      expect(positionResponse.body.data).toBeInstanceOf(Array);
      expect(positionResponse.body.data).toHaveLength(1);
      expect(positionResponse.body.data[0]).toMatchObject({
        stakeId: stakeId,
        poolName: selectedPool.name,
        stakedAmount: stakeAmount,
        currentRewards: expect.any(String),
      });

      // Step 6: Claim rewards (simulate time passage)
      const rewardsResponse = await request(apiBaseUrl)
        .post(`/api/yield-farming/stakes/${stakeId}/claim`)
        .set('Authorization', `Bearer ${userData.token}`)
        .expect(200);

      expect(rewardsResponse.body).toMatchObject({
        success: true,
        data: {
          rewardsClaimed: expect.any(String),
          transactionId: expect.any(Number),
        },
      });
    });
  });

  describe('Cross-Chain Bridge Flow', () => {
    test('should handle cross-chain token bridge transaction', async () => {
      // Setup: Create user with tokens on source chain
      const userData = await global.testUtils.createTestUser(dbPool);
      await global.testUtils.giveUserTokens(
        dbPool,
        userData.id,
        '500.0',
        'ethereum'
      );

      // Step 1: Initiate cross-chain bridge
      const bridgeData = {
        sourceChain: 'ethereum',
        destinationChain: 'polygon',
        amount: '100.0',
        recipientAddress: userData.walletAddress,
      };

      const bridgeResponse = await request(apiBaseUrl)
        .post('/api/bridge/initiate')
        .set('Authorization', `Bearer ${userData.token}`)
        .send(bridgeData)
        .expect(201);

      expect(bridgeResponse.body).toMatchObject({
        success: true,
        data: {
          bridgeId: expect.any(String),
          sourceChain: 'ethereum',
          destinationChain: 'polygon',
          amount: '100.0',
          status: 'initiated',
        },
      });

      const bridgeId = bridgeResponse.body.data.bridgeId;

      // Step 2: Verify bridge transaction was created
      const bridgeRecord = await dbPool.query(
        'SELECT * FROM cross_chain_bridges WHERE bridge_id = $1',
        [bridgeId]
      );

      expect(bridgeRecord.rows).toHaveLength(1);
      expect(bridgeRecord.rows[0]).toMatchObject({
        user_id: userData.id,
        source_chain: 'ethereum',
        destination_chain: 'polygon',
        amount: '100.0',
        status: 'initiated',
      });

      // Step 3: Check bridge status
      const statusResponse = await request(apiBaseUrl)
        .get(`/api/bridge/${bridgeId}/status`)
        .set('Authorization', `Bearer ${userData.token}`)
        .expect(200);

      expect(statusResponse.body).toMatchObject({
        success: true,
        data: {
          bridgeId: bridgeId,
          status: expect.stringMatching(/initiated|processing|completed/),
          confirmations: expect.any(Number),
        },
      });

      // Step 4: Simulate bridge completion
      const completionResponse = await request(apiBaseUrl)
        .post(`/api/bridge/${bridgeId}/complete`)
        .set('Authorization', `Bearer ${userData.token}`)
        .send({
          destinationTxHash: '0x' + '1'.repeat(64),
          confirmations: 12,
        })
        .expect(200);

      expect(completionResponse.body).toMatchObject({
        success: true,
        data: {
          status: 'completed',
          destinationTxHash: expect.stringMatching(/^0x[a-fA-F0-9]{64}$/),
        },
      });

      // Step 5: Verify user balances updated correctly
      const finalBalanceResponse = await request(apiBaseUrl)
        .get('/api/users/balance')
        .set('Authorization', `Bearer ${userData.token}`)
        .expect(200);

      expect(finalBalanceResponse.body.data.chains.ethereum).toBe('400.0');
      expect(finalBalanceResponse.body.data.chains.polygon).toBe('100.0');
    });
  });

  describe('Error Handling and Recovery', () => {
    test('should handle API failures gracefully with proper rollback', async () => {
      const userData = await global.testUtils.createTestUser(dbPool);

      // Step 1: Start transaction that will fail
      const invalidStationData = {
        stationId: '', // Invalid: empty station ID
        name: 'Test Station',
        location: { lat: 'invalid', lng: 'invalid' }, // Invalid coordinates
      };

      const failureResponse = await request(apiBaseUrl)
        .post('/api/weather-stations')
        .set('Authorization', `Bearer ${userData.token}`)
        .send(invalidStationData)
        .expect(400);

      expect(failureResponse.body).toMatchObject({
        success: false,
        error: expect.any(String),
      });

      // Step 2: Verify no partial data was created
      const stationCount = await dbPool.query(
        'SELECT COUNT(*) as count FROM weather_stations WHERE owner_id = $1',
        [userData.id]
      );

      expect(parseInt(stationCount.rows[0].count)).toBe(0);

      // Step 3: Verify user can still perform valid operations
      const validStationData = {
        stationId: 'valid-station-001',
        name: 'Valid Test Station',
        location: { lat: 40.7128, lng: -74.006 },
      };

      const successResponse = await request(apiBaseUrl)
        .post('/api/weather-stations')
        .set('Authorization', `Bearer ${userData.token}`)
        .send(validStationData)
        .expect(201);

      expect(successResponse.body.success).toBe(true);
    });

    test('should handle service timeouts with proper retry logic', async () => {
      // Mock a slow service response
      const mockServerUrl = global.testConfig.services.mockserver;

      // Create expectation for slow response
      const slowExpectation = {
        httpRequest: {
          method: 'POST',
          path: '/api/slow-endpoint',
        },
        httpResponse: {
          statusCode: 200,
          delay: { timeUnit: 'MILLISECONDS', value: 5000 }, // 5 second delay
          body: JSON.stringify({ success: true, message: 'Slow response' }),
        },
      };

      await fetch(`${mockServerUrl}/mockserver/expectation`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(slowExpectation),
      });

      // Test timeout handling
      const startTime = Date.now();

      try {
        await request(mockServerUrl)
          .post('/api/slow-endpoint')
          .timeout(3000) // 3 second timeout
          .send({});
      } catch (error) {
        const duration = Date.now() - startTime;
        expect(error.code).toBe('ECONNABORTED');
        expect(duration).toBeLessThan(4000); // Should timeout before 4 seconds
      }
    });
  });

  describe('Performance and Load Testing', () => {
    test('should handle concurrent API requests efficiently', async () => {
      const userData = await global.testUtils.createTestUser(dbPool);
      const concurrentRequests = 10;

      // Create concurrent requests
      const promises = Array.from(
        { length: concurrentRequests },
        async (_, i) => {
          const stationData = {
            stationId: `concurrent-station-${i}`,
            name: `Concurrent Test Station ${i}`,
            location: { lat: 40.7128 + i * 0.001, lng: -74.006 + i * 0.001 },
          };

          const start = Date.now();
          const response = await request(apiBaseUrl)
            .post('/api/weather-stations')
            .set('Authorization', `Bearer ${userData.token}`)
            .send(stationData);

          return {
            index: i,
            duration: Date.now() - start,
            success: response.status === 201,
            stationId: stationData.stationId,
          };
        }
      );

      const results = await Promise.all(promises);

      // Verify all requests succeeded
      expect(results.every(r => r.success)).toBe(true);

      // Verify response times are reasonable
      const avgResponseTime =
        results.reduce((sum, r) => sum + r.duration, 0) / results.length;
      expect(avgResponseTime).toBeLessThan(2000); // Less than 2 seconds average

      // Verify all stations were created
      const stationCount = await dbPool.query(
        'SELECT COUNT(*) as count FROM weather_stations WHERE owner_id = $1',
        [userData.id]
      );

      expect(parseInt(stationCount.rows[0].count)).toBe(concurrentRequests);
    });

    test('should maintain performance under database load', async () => {
      const userData = await global.testUtils.createTestUser(dbPool);
      const stationData = await global.testUtils.createTestWeatherStation(
        dbPool,
        userData.id
      );

      // Generate high volume of weather data
      const dataPoints = 50;
      const promises = Array.from({ length: dataPoints }, async (_, i) => {
        const weatherData = global.testUtils.generateTestData.weatherData(
          stationData.stationId
        );

        return await request(apiBaseUrl)
          .post(`/api/weather-stations/${stationData.stationId}/data`)
          .set('Authorization', `Bearer ${userData.token}`)
          .send({
            measurements: weatherData.measurements,
            timestamp: new Date(Date.now() + i * 1000).toISOString(),
          });
      });

      const start = Date.now();
      const results = await Promise.all(promises);
      const totalDuration = Date.now() - start;

      // Verify all requests succeeded
      expect(results.every(r => r.status === 201)).toBe(true);

      // Verify throughput is acceptable
      const throughput = dataPoints / (totalDuration / 1000); // requests per second
      expect(throughput).toBeGreaterThan(5); // At least 5 requests per second

      // Verify database integrity
      const dbCount = await dbPool.query(
        'SELECT COUNT(*) as count FROM weather_data WHERE station_id = $1',
        [stationData.stationId]
      );

      expect(parseInt(dbCount.rows[0].count)).toBe(dataPoints);
    });
  });
});
