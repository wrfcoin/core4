/**
 * Service Interaction Integration Test Suite
 * Agent 6 - Integration Test Automation Specialist
 *
 * Tests direct service-to-service interactions using Agent 4's mock harness
 * Validates service communication patterns and data flow
 */

const { Pool } = require('pg');
const Redis = require('redis');

describe('Service Interaction Integration Test Suite', () => {
  let dbPool;
  let redisClient;
  let mockServicesManager;

  beforeAll(async () => {
    mockServicesManager = global.mockServicesManager;

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

  describe('Database Service Interactions', () => {
    test('should handle database CRUD operations with transaction integrity', async () => {
      const userData = global.testUtils.generateTestData.user();

      // Start transaction
      const client = await dbPool.connect();

      try {
        await client.query('BEGIN');

        // Insert user
        const insertResult = await client.query(
          `INSERT INTO users (username, email, wallet_address, status, role)
           VALUES ($1, $2, $3, $4, $5) RETURNING *`,
          [
            userData.username,
            userData.email,
            userData.walletAddress,
            userData.status,
            userData.role,
          ]
        );

        expect(insertResult.rows).toHaveLength(1);
        const userId = insertResult.rows[0].id;

        // Insert weather station for user
        const stationResult = await client.query(
          `INSERT INTO weather_stations (station_id, owner_id, name, location, status)
           VALUES ($1, $2, $3, $4, $5) RETURNING *`,
          [
            'test-station-tx',
            userId,
            'Test Transaction Station',
            '{"lat": 40.7128, "lng": -74.0060}',
            'active',
          ]
        );

        expect(stationResult.rows).toHaveLength(1);

        // Insert weather data for station
        const weatherData =
          global.testUtils.generateTestData.weatherData('test-station-tx');
        const weatherResult = await client.query(
          `INSERT INTO weather_data (station_id, measurements, quality_score, timestamp)
           VALUES ($1, $2, $3, $4) RETURNING *`,
          [
            weatherData.station_id,
            JSON.stringify(weatherData.measurements),
            weatherData.quality_score,
            weatherData.timestamp,
          ]
        );

        expect(weatherResult.rows).toHaveLength(1);

        // Commit transaction
        await client.query('COMMIT');

        // Verify all data exists
        const finalCheck = await client.query(
          `SELECT u.username, ws.station_id, wd.id as weather_id
           FROM users u
           JOIN weather_stations ws ON ws.owner_id = u.id
           JOIN weather_data wd ON wd.station_id = ws.station_id
           WHERE u.id = $1`,
          [userId]
        );

        expect(finalCheck.rows).toHaveLength(1);
        expect(finalCheck.rows[0].username).toBe(userData.username);
        expect(finalCheck.rows[0].station_id).toBe('test-station-tx');
      } catch (error) {
        await client.query('ROLLBACK');
        throw error;
      } finally {
        client.release();
      }
    });

    test('should handle database connection pooling under concurrent load', async () => {
      const concurrentOperations = 20;
      const operations = Array.from(
        { length: concurrentOperations },
        async (_, i) => {
          const userData = global.testUtils.generateTestData.user({
            username: `pool_test_${i}`,
            email: `pool${i}@test.com`,
          });

          const result = await dbPool.query(
            'INSERT INTO users (username, email, wallet_address, status, role) VALUES ($1, $2, $3, $4, $5) RETURNING id',
            [
              userData.username,
              userData.email,
              userData.walletAddress,
              userData.status,
              userData.role,
            ]
          );

          return result.rows[0].id;
        }
      );

      const userIds = await Promise.all(operations);
      expect(userIds).toHaveLength(concurrentOperations);

      // Verify all users were created
      const verification = await dbPool.query(
        'SELECT COUNT(*) as count FROM users WHERE username LIKE $1',
        ['pool_test_%']
      );

      expect(parseInt(verification.rows[0].count)).toBe(concurrentOperations);
    });

    test('should handle database constraint violations gracefully', async () => {
      const userData = global.testUtils.generateTestData.user();

      // Insert user successfully
      await dbPool.query(
        'INSERT INTO users (username, email, wallet_address, status, role) VALUES ($1, $2, $3, $4, $5)',
        [
          userData.username,
          userData.email,
          userData.walletAddress,
          userData.status,
          userData.role,
        ]
      );

      // Try to insert duplicate username - should fail
      await expect(
        dbPool.query(
          'INSERT INTO users (username, email, wallet_address, status, role) VALUES ($1, $2, $3, $4, $5)',
          [
            userData.username,
            'different@email.com',
            '0x1234567890123456789012345678901234567890',
            userData.status,
            userData.role,
          ]
        )
      ).rejects.toThrow(/duplicate key value violates unique constraint/);

      // Try to insert duplicate email - should fail
      await expect(
        dbPool.query(
          'INSERT INTO users (username, email, wallet_address, status, role) VALUES ($1, $2, $3, $4, $5)',
          [
            'different_username',
            userData.email,
            '0x1234567890123456789012345678901234567890',
            userData.status,
            userData.role,
          ]
        )
      ).rejects.toThrow(/duplicate key value violates unique constraint/);
    });
  });

  describe('Redis Cache Service Interactions', () => {
    test('should cache and retrieve weather data efficiently', async () => {
      const stationId = 'cache-test-station';
      const weatherData =
        global.testUtils.generateTestData.weatherData(stationId);
      const cacheKey = `weather:${stationId}:latest`;

      // Store in cache
      await redisClient.setEx(cacheKey, 3600, JSON.stringify(weatherData));

      // Retrieve from cache
      const cachedData = await redisClient.get(cacheKey);
      const parsedData = JSON.parse(cachedData);

      expect(parsedData.station_id).toBe(stationId);
      expect(parsedData.measurements).toEqual(weatherData.measurements);

      // Check TTL
      const ttl = await redisClient.ttl(cacheKey);
      expect(ttl).toBeGreaterThan(3500);
      expect(ttl).toBeLessThanOrEqual(3600);
    });

    test('should handle cache invalidation patterns', async () => {
      const stationId = 'invalidation-test-station';
      const cacheKeys = [
        `weather:${stationId}:latest`,
        `weather:${stationId}:hourly`,
        `weather:${stationId}:daily`,
      ];

      // Set multiple cache entries
      for (const key of cacheKeys) {
        await redisClient.set(
          key,
          JSON.stringify({ station: stationId, timestamp: Date.now() })
        );
      }

      // Verify all keys exist
      for (const key of cacheKeys) {
        const exists = await redisClient.exists(key);
        expect(exists).toBe(1);
      }

      // Invalidate pattern
      const pattern = `weather:${stationId}:*`;
      const keys = await redisClient.keys(pattern);

      if (keys.length > 0) {
        await redisClient.del(keys);
      }

      // Verify all keys are deleted
      for (const key of cacheKeys) {
        const exists = await redisClient.exists(key);
        expect(exists).toBe(0);
      }
    });

    test('should handle session management with Redis', async () => {
      const userId = 123;
      const sessionToken = 'test-session-token-' + Date.now();
      const sessionData = {
        userId: userId,
        username: 'test_user',
        createdAt: new Date().toISOString(),
        expiresAt: new Date(Date.now() + 3600000).toISOString(), // 1 hour
      };

      // Store session
      const sessionKey = `session:${sessionToken}`;
      await redisClient.setEx(sessionKey, 3600, JSON.stringify(sessionData));

      // Retrieve session
      const retrievedSession = await redisClient.get(sessionKey);
      const parsedSession = JSON.parse(retrievedSession);

      expect(parsedSession.userId).toBe(userId);
      expect(parsedSession.username).toBe('test_user');

      // Update last activity
      parsedSession.lastActivity = new Date().toISOString();
      await redisClient.setEx(sessionKey, 3600, JSON.stringify(parsedSession));

      // Verify update
      const updatedSession = JSON.parse(await redisClient.get(sessionKey));
      expect(updatedSession.lastActivity).toBeDefined();

      // Simulate session expiry
      await redisClient.expire(sessionKey, 1);
      await new Promise(resolve => setTimeout(resolve, 1100));

      const expiredSession = await redisClient.get(sessionKey);
      expect(expiredSession).toBeNull();
    });
  });

  describe('Blockchain RPC Service Interactions', () => {
    test('should communicate with blockchain RPC mock service', async () => {
      const rpcUrl = global.testConfig.services.blockchain;

      // Test RPC health
      const healthResponse = await fetch(`${rpcUrl}/health`);
      expect(healthResponse.ok).toBe(true);

      const healthData = await healthResponse.json();
      expect(healthData).toMatchObject({
        service: expect.stringContaining('blockchain'),
        status: 'healthy',
        timestamp: expect.any(String),
      });

      // Test RPC call
      const rpcPayload = {
        jsonrpc: '2.0',
        method: 'eth_blockNumber',
        params: [],
        id: 1,
      };

      const rpcResponse = await fetch(rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(rpcPayload),
      });

      expect(rpcResponse.ok).toBe(true);

      const rpcData = await rpcResponse.json();
      expect(rpcData).toMatchObject({
        jsonrpc: '2.0',
        result: expect.any(String),
        id: 1,
      });

      // Verify block number is hex format
      expect(rpcData.result).toMatch(/^0x[0-9a-fA-F]+$/);
    });

    test('should handle blockchain transaction submission flow', async () => {
      const rpcUrl = global.testConfig.services.blockchain;

      // Create transaction
      const txData = {
        jsonrpc: '2.0',
        method: 'eth_sendTransaction',
        params: [
          {
            from: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
            to: '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
            value: '0x16345785d8a0000', // 0.1 ETH in hex
            gas: '0x5208', // 21000 in hex
            gasPrice: '0x4a817c800', // 20 Gwei in hex
          },
        ],
        id: 2,
      };

      const txResponse = await fetch(rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(txData),
      });

      expect(txResponse.ok).toBe(true);

      const txResult = await txResponse.json();
      expect(txResult).toMatchObject({
        jsonrpc: '2.0',
        result: expect.stringMatching(/^0x[a-fA-F0-9]{64}$/), // Transaction hash
        id: 2,
      });

      const txHash = txResult.result;

      // Check transaction receipt
      const receiptData = {
        jsonrpc: '2.0',
        method: 'eth_getTransactionReceipt',
        params: [txHash],
        id: 3,
      };

      const receiptResponse = await fetch(rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(receiptData),
      });

      const receiptResult = await receiptResponse.json();
      expect(receiptResult.jsonrpc).toBe('2.0');
      expect(receiptResult.id).toBe(3);
    });
  });

  describe('Weather Oracle Service Interactions', () => {
    test('should retrieve weather data from oracle mock service', async () => {
      const weatherUrl = global.testConfig.services.weather;

      // Test health endpoint
      const healthResponse = await fetch(`${weatherUrl}/health`);
      expect(healthResponse.ok).toBe(true);

      // Get station data
      const stationId = 'station-integration-001';
      const dataResponse = await fetch(
        `${weatherUrl}/api/station/${stationId}/data`
      );

      expect(dataResponse.ok).toBe(true);

      const weatherData = await dataResponse.json();
      expect(weatherData).toMatchObject({
        success: true,
        data: {
          stationId: stationId,
          measurements: expect.objectContaining({
            temperature: expect.any(Number),
            humidity: expect.any(Number),
            pressure: expect.any(Number),
          }),
          timestamp: expect.any(String),
        },
      });

      // Validate measurements are within realistic ranges
      const { measurements } = weatherData.data;
      expect(measurements.temperature).toBeGreaterThan(-50);
      expect(measurements.temperature).toBeLessThan(60);
      expect(measurements.humidity).toBeGreaterThanOrEqual(0);
      expect(measurements.humidity).toBeLessThanOrEqual(100);
      expect(measurements.pressure).toBeGreaterThan(900);
      expect(measurements.pressure).toBeLessThan(1100);
    });

    test('should handle batch weather data requests', async () => {
      const weatherUrl = global.testConfig.services.weather;
      const stationIds = ['station-integration-001', 'station-integration-002'];

      const batchResponse = await fetch(`${weatherUrl}/api/stations/batch`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ stations: stationIds }),
      });

      expect(batchResponse.ok).toBe(true);

      const batchData = await batchResponse.json();
      expect(batchData.success).toBe(true);
      expect(batchData.data).toHaveLength(stationIds.length);

      for (const stationData of batchData.data) {
        expect(stationIds).toContain(stationData.stationId);
        expect(stationData.measurements).toBeDefined();
        expect(stationData.timestamp).toBeDefined();
      }
    });

    test('should handle weather alerts and notifications', async () => {
      const weatherUrl = global.testConfig.services.weather;

      // Subscribe to alerts
      const alertResponse = await fetch(`${weatherUrl}/api/alerts/subscribe`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          stationId: 'station-integration-001',
          thresholds: {
            temperature: { min: -10, max: 40 },
            humidity: { min: 10, max: 95 },
            windSpeed: { max: 25 },
          },
        }),
      });

      expect(alertResponse.ok).toBe(true);

      // Get current alerts
      const alertsListResponse = await fetch(`${weatherUrl}/api/alerts/active`);
      expect(alertsListResponse.ok).toBe(true);

      const alertsData = await alertsListResponse.json();
      expect(alertsData).toMatchObject({
        success: true,
        data: expect.any(Array),
      });
    });
  });

  describe('Carbon Oracle Service Interactions', () => {
    test('should calculate carbon credits from weather data', async () => {
      const carbonUrl = global.testConfig.services.carbon;

      // Submit weather data for carbon calculation
      const weatherData = global.testUtils.generateTestData.weatherData(
        'station-integration-001'
      );

      const carbonResponse = await fetch(`${carbonUrl}/api/calculate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          stationId: weatherData.station_id,
          measurements: weatherData.measurements,
          timestamp: weatherData.timestamp,
          calculationMethod: 'weather_monitoring_ipcc_2021',
        }),
      });

      expect(carbonResponse.ok).toBe(true);

      const carbonData = await carbonResponse.json();
      expect(carbonData).toMatchObject({
        success: true,
        data: {
          co2Equivalent: expect.any(Number),
          carbonCreditsEarned: expect.any(Number),
          calculationMethod: 'weather_monitoring_ipcc_2021',
          metadata: expect.any(Object),
          timestamp: expect.any(String),
        },
      });

      // Validate carbon metrics are positive
      expect(carbonData.data.co2Equivalent).toBeGreaterThan(0);
      expect(carbonData.data.carbonCreditsEarned).toBeGreaterThan(0);
    });

    test('should track carbon credit portfolio', async () => {
      const carbonUrl = global.testConfig.services.carbon;
      const userId = 1; // Test user from schema

      // Get user's carbon portfolio
      const portfolioResponse = await fetch(
        `${carbonUrl}/api/portfolio/${userId}`
      );

      expect(portfolioResponse.ok).toBe(true);

      const portfolioData = await portfolioResponse.json();
      expect(portfolioData).toMatchObject({
        success: true,
        data: {
          userId: userId,
          totalCredits: expect.any(Number),
          availableCredits: expect.any(Number),
          retiredCredits: expect.any(Number),
          portfolio: expect.any(Array),
        },
      });

      // Validate portfolio structure
      if (portfolioData.data.portfolio.length > 0) {
        const credit = portfolioData.data.portfolio[0];
        expect(credit).toMatchObject({
          id: expect.any(String),
          amount: expect.any(Number),
          source: expect.any(String),
          status: expect.stringMatching(/verified|issued|retired/),
          issuedAt: expect.any(String),
        });
      }
    });
  });

  describe('MockServer Advanced Interactions', () => {
    test('should handle dynamic mock expectations', async () => {
      const mockServerUrl = global.testConfig.services.mockserver;

      // Create dynamic expectation
      const expectation = {
        httpRequest: {
          method: 'GET',
          path: '/api/dynamic/test',
        },
        httpResponse: {
          statusCode: 200,
          headers: {
            'Content-Type': ['application/json'],
          },
          body: JSON.stringify({
            message: 'Dynamic mock response',
            timestamp: new Date().toISOString(),
          }),
        },
        times: {
          remainingTimes: 3,
        },
      };

      const expectationResponse = await fetch(
        `${mockServerUrl}/mockserver/expectation`,
        {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(expectation),
        }
      );

      expect(expectationResponse.ok).toBe(true);

      // Test the dynamic mock
      const testResponse = await fetch(`${mockServerUrl}/api/dynamic/test`);
      expect(testResponse.ok).toBe(true);

      const testData = await testResponse.json();
      expect(testData.message).toBe('Dynamic mock response');
    });

    test('should verify mock interactions', async () => {
      const mockServerUrl = global.testConfig.services.mockserver;

      // Make a request to a mocked endpoint
      await fetch(`${mockServerUrl}/api/test-verification`);

      // Verify the interaction was recorded
      const verificationResponse = await fetch(
        `${mockServerUrl}/mockserver/verify`,
        {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            httpRequest: {
              method: 'GET',
              path: '/api/test-verification',
            },
            times: {
              atLeast: 1,
            },
          }),
        }
      );

      expect([200, 202]).toContain(verificationResponse.status); // 202 = verification successful
    });
  });

  describe('Cross-Service Integration Flows', () => {
    test('should complete full weather-to-blockchain flow', async () => {
      // Step 1: Store weather data in database
      const weatherData =
        global.testUtils.generateTestData.weatherData('flow-test-station');

      const weatherResult = await dbPool.query(
        `INSERT INTO weather_data (station_id, measurements, quality_score, timestamp)
         VALUES ($1, $2, $3, $4) RETURNING id`,
        [
          weatherData.station_id,
          JSON.stringify(weatherData.measurements),
          weatherData.quality_score,
          weatherData.timestamp,
        ]
      );

      const weatherId = weatherResult.rows[0].id;

      // Step 2: Calculate carbon credits via carbon oracle
      const carbonUrl = global.testConfig.services.carbon;
      const carbonResponse = await fetch(`${carbonUrl}/api/calculate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          stationId: weatherData.station_id,
          measurements: weatherData.measurements,
          timestamp: weatherData.timestamp,
          calculationMethod: 'weather_monitoring_ipcc_2021',
        }),
      });

      const carbonData = await carbonResponse.json();

      // Step 3: Store carbon credits in database
      const carbonResult = await dbPool.query(
        `INSERT INTO carbon_credits (user_id, amount, source, metadata, status)
         VALUES ($1, $2, $3, $4, $5) RETURNING id`,
        [
          1,
          carbonData.data.carbonCreditsEarned,
          'weather_monitoring',
          JSON.stringify(carbonData.data.metadata),
          'verified',
        ]
      );

      const carbonCreditId = carbonResult.rows[0].id;

      // Step 4: Create blockchain transaction via RPC
      const rpcUrl = global.testConfig.services.blockchain;
      const txData = {
        jsonrpc: '2.0',
        method: 'eth_sendTransaction',
        params: [
          {
            from: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
            to: '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
            value: '0x0', // No ETH transfer, just data
            gas: '0x5208',
            data:
              '0x' +
              Buffer.from(`carbon_credit:${carbonCreditId}`).toString('hex'),
          },
        ],
        id: Date.now(),
      };

      const txResponse = await fetch(rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(txData),
      });

      const txResult = await txResponse.json();

      // Step 5: Store transaction in database
      const transactionData = global.testUtils.generateTestData.transaction({
        tx_id: txResult.result,
        type: 'carbon_credit',
        data: { carbonCreditId, weatherId },
      });

      const txDbResult = await dbPool.query(
        `INSERT INTO transactions (tx_id, from_address, to_address, amount, fee, status, type, data)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id`,
        [
          transactionData.tx_id,
          transactionData.from_address,
          transactionData.to_address,
          transactionData.amount,
          transactionData.fee,
          transactionData.status,
          transactionData.type,
          JSON.stringify(transactionData.data),
        ]
      );

      // Step 6: Cache results in Redis
      const cacheKey = `flow:${weatherData.station_id}:${Date.now()}`;
      const flowResult = {
        weatherId,
        carbonCreditId,
        transactionId: txDbResult.rows[0].id,
        txHash: txResult.result,
        completedAt: new Date().toISOString(),
      };

      await redisClient.setEx(cacheKey, 3600, JSON.stringify(flowResult));

      // Verify complete flow
      expect(weatherId).toBeDefined();
      expect(carbonCreditId).toBeDefined();
      expect(txDbResult.rows[0].id).toBeDefined();
      expect(txResult.result).toMatch(/^0x[a-fA-F0-9]{64}$/);

      // Verify cache
      const cachedFlow = JSON.parse(await redisClient.get(cacheKey));
      expect(cachedFlow.weatherId).toBe(weatherId);
      expect(cachedFlow.carbonCreditId).toBe(carbonCreditId);
    });

    test('should handle service failure scenarios gracefully', async () => {
      // Test database failure scenario
      const invalidQuery = 'SELECT * FROM nonexistent_table';

      await expect(dbPool.query(invalidQuery)).rejects.toThrow();

      // Verify other services still work
      const healthResponse = await fetch(
        `${global.testConfig.services.weather}/health`
      );
      expect(healthResponse.ok).toBe(true);

      // Test Redis connection recovery
      await redisClient.set('test:recovery', 'test-value');
      const recoveredValue = await redisClient.get('test:recovery');
      expect(recoveredValue).toBe('test-value');
    });
  });

  describe('Service Performance Monitoring', () => {
    test('should monitor service response times', async () => {
      const services = [
        {
          name: 'blockchain',
          url: global.testConfig.services.blockchain + '/health',
        },
        {
          name: 'weather',
          url: global.testConfig.services.weather + '/health',
        },
        { name: 'carbon', url: global.testConfig.services.carbon + '/health' },
      ];

      const performanceResults = [];

      for (const service of services) {
        const startTime = Date.now();
        const response = await fetch(service.url);
        const responseTime = Date.now() - startTime;

        performanceResults.push({
          service: service.name,
          responseTime,
          status: response.status,
          healthy: response.ok,
        });

        expect(response.ok).toBe(true);
        expect(responseTime).toBeLessThan(2000); // Less than 2 seconds
      }

      // All services should be responsive
      expect(performanceResults.every(r => r.healthy)).toBe(true);

      // Average response time should be reasonable
      const avgResponseTime =
        performanceResults.reduce((sum, r) => sum + r.responseTime, 0) /
        performanceResults.length;
      expect(avgResponseTime).toBeLessThan(1000); // Less than 1 second average
    });
  });
});
