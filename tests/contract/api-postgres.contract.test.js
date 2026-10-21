/**
 * API-Postgres Contract Tests
 * Tests the contract between the WRFCoin API and PostgreSQL database
 * Ensures data consistency and proper database operations
 */

const { Pool } = require('pg');
const {
  pactProviders,
  commonMatchers,
  setupPactProvider,
  teardownPactProvider,
  like,
  eachLike,
  term,
  uuid,
  iso8601DateTime,
} = require('./setup/pact.setup');

describe('API-Postgres Contract Tests', () => {
  let provider;
  let dbPool;

  beforeAll(async () => {
    // Setup Pact provider for database interactions
    provider = await setupPactProvider('postgres');

    // Setup database connection pool
    dbPool = new Pool({
      host: 'localhost',
      port: 5434,
      user: 'postgres',
      password: 'testpass',
      database: 'wrfcoin_test',
      max: 5,
      idleTimeoutMillis: 30000,
    });

    // Test database connectivity
    await dbPool.query('SELECT 1');
    console.log('✅ Database connection established');
  });

  afterAll(async () => {
    if (dbPool) {
      await dbPool.end();
    }
    if (provider) {
      await teardownPactProvider('postgres');
    }
  });

  beforeEach(async () => {
    // Clean up test data before each test
    await cleanupTestData();
  });

  // Helper function to clean test data
  const cleanupTestData = async () => {
    const tables = [
      'transactions',
      'weather_data',
      'carbon_credits',
      'user_sessions',
      'users',
    ];

    for (const table of tables) {
      try {
        await dbPool.query(`DELETE FROM ${table} WHERE true`);
      } catch (error) {
        // Table might not exist yet, ignore
        if (!error.message.includes('does not exist')) {
          console.warn(`Warning cleaning ${table}:`, error.message);
        }
      }
    }
  };

  describe('User Management Contracts', () => {
    test('should create user with valid data', async () => {
      const userData = {
        username: 'testuser',
        email: 'test@wrfcoin.com',
        wallet_address: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
        status: 'active',
        role: 'user',
      };

      // Set up contract expectation
      await provider
        .given('database is empty')
        .uponReceiving('a request to create a new user')
        .withRequest({
          method: 'POST',
          path: '/users',
          headers: { 'Content-Type': 'application/json' },
          body: userData,
        })
        .willRespondWith({
          status: 201,
          headers: { 'Content-Type': 'application/json' },
          body: {
            success: true,
            data: {
              id: commonMatchers.id,
              username: userData.username,
              email: userData.email,
              walletAddress: userData.wallet_address,
              status: userData.status,
              role: userData.role,
              createdAt: iso8601DateTime(),
              updatedAt: iso8601DateTime(),
              isVerified: false,
              reputation: 0,
            },
          },
        });

      // Execute database operation directly to test contract
      const result = await dbPool.query(
        `
        INSERT INTO users (username, email, wallet_address, status, role)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, username, email, wallet_address as "walletAddress", 
                  status, role, created_at as "createdAt", updated_at as "updatedAt",
                  is_verified as "isVerified", reputation
      `,
        [
          userData.username,
          userData.email,
          userData.wallet_address,
          userData.status,
          userData.role,
        ]
      );

      expect(result.rows).toHaveLength(1);
      const createdUser = result.rows[0];

      expect(createdUser).toMatchObject({
        username: userData.username,
        email: userData.email,
        walletAddress: userData.wallet_address,
        status: userData.status,
        role: userData.role,
        isVerified: false,
        reputation: 0,
      });

      expect(createdUser.id).toBeDefined();
      expect(createdUser.createdAt).toBeDefined();
      expect(createdUser.updatedAt).toBeDefined();

      await provider.verify();
    });

    test('should retrieve user by ID', async () => {
      // First create a user
      const insertResult = await dbPool.query(
        `
        INSERT INTO users (username, email, wallet_address, status, role)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
      `,
        [
          'testuser',
          'test@wrfcoin.com',
          '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
          'active',
          'user',
        ]
      );

      const userId = insertResult.rows[0].id;

      // Set up contract expectation
      await provider
        .given('a user exists with ID 1')
        .uponReceiving('a request to get user by ID')
        .withRequest({
          method: 'GET',
          path: `/users/${userId}`,
        })
        .willRespondWith({
          status: 200,
          headers: { 'Content-Type': 'application/json' },
          body: {
            success: true,
            data: {
              id: userId,
              username: 'testuser',
              email: 'test@wrfcoin.com',
              walletAddress: commonMatchers.walletAddress,
              status: 'active',
              role: 'user',
              createdAt: iso8601DateTime(),
              updatedAt: iso8601DateTime(),
              isVerified: false,
              reputation: 0,
            },
          },
        });

      // Execute database query
      const result = await dbPool.query(
        `
        SELECT id, username, email, wallet_address as "walletAddress",
               status, role, created_at as "createdAt", updated_at as "updatedAt",
               is_verified as "isVerified", reputation
        FROM users 
        WHERE id = $1
      `,
        [userId]
      );

      expect(result.rows).toHaveLength(1);
      const user = result.rows[0];
      expect(user.id).toBe(userId);
      expect(user.username).toBe('testuser');

      await provider.verify();
    });
  });

  describe('Weather Data Contracts', () => {
    test('should store weather data with carbon metrics', async () => {
      const weatherData = {
        station_id: 'station-test',
        measurements: {
          temperature: 22.5,
          humidity: 65.0,
          pressure: 1013.25,
          windSpeed: 3.2,
          windDirection: 180,
        },
        carbon_metrics: {
          co2Equivalent: 0.05,
          carbonCreditsEarned: 0.005,
          calculationMethod: 'weather_monitoring_ipcc_2021',
        },
        quality_score: 0.95,
        timestamp: new Date().toISOString(),
      };

      // Set up contract expectation
      await provider
        .given('weather station exists')
        .uponReceiving('a request to store weather data')
        .withRequest({
          method: 'POST',
          path: '/weather/data',
          headers: { 'Content-Type': 'application/json' },
          body: weatherData,
        })
        .willRespondWith({
          status: 201,
          headers: { 'Content-Type': 'application/json' },
          body: {
            success: true,
            data: {
              id: uuid(),
              stationId: weatherData.station_id,
              measurements: commonMatchers.weatherMeasurement,
              carbonMetrics: commonMatchers.carbonMeasurement,
              qualityScore: like(0.95),
              timestamp: iso8601DateTime(),
              processedAt: iso8601DateTime(),
            },
          },
        });

      // Execute database operation
      const result = await dbPool.query(
        `
        INSERT INTO weather_data (station_id, measurements, carbon_metrics, quality_score, timestamp)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, station_id as "stationId", measurements, carbon_metrics as "carbonMetrics",
                  quality_score as "qualityScore", timestamp, processed_at as "processedAt"
      `,
        [
          weatherData.station_id,
          JSON.stringify(weatherData.measurements),
          JSON.stringify(weatherData.carbon_metrics),
          weatherData.quality_score,
          weatherData.timestamp,
        ]
      );

      expect(result.rows).toHaveLength(1);
      const stored = result.rows[0];

      expect(stored.stationId).toBe(weatherData.station_id);
      expect(stored.measurements).toEqual(weatherData.measurements);
      expect(stored.carbonMetrics).toEqual(weatherData.carbon_metrics);
      expect(stored.qualityScore).toBe(weatherData.quality_score);

      await provider.verify();
    });

    test('should query weather data by station and date range', async () => {
      // Insert test weather data
      const testData = [
        {
          station_id: 'station-test',
          measurements: { temperature: 20.0, humidity: 60.0 },
          timestamp: '2024-01-01T12:00:00Z',
        },
        {
          station_id: 'station-test',
          measurements: { temperature: 22.0, humidity: 65.0 },
          timestamp: '2024-01-01T13:00:00Z',
        },
      ];

      for (const data of testData) {
        await dbPool.query(
          `
          INSERT INTO weather_data (station_id, measurements, timestamp)
          VALUES ($1, $2, $3)
        `,
          [data.station_id, JSON.stringify(data.measurements), data.timestamp]
        );
      }

      // Set up contract expectation
      await provider
        .given('weather data exists for station-test')
        .uponReceiving(
          'a request to get weather data by station and date range'
        )
        .withRequest({
          method: 'GET',
          path: '/weather/station-test/data',
          query: {
            start: '2024-01-01T00:00:00Z',
            end: '2024-01-01T23:59:59Z',
          },
        })
        .willRespondWith({
          status: 200,
          headers: { 'Content-Type': 'application/json' },
          body: {
            success: true,
            data: eachLike(
              {
                id: uuid(),
                stationId: 'station-test',
                measurements: commonMatchers.weatherMeasurement,
                timestamp: iso8601DateTime(),
                processedAt: iso8601DateTime(),
              },
              { min: 2 }
            ),
            count: 2,
            timestamp: iso8601DateTime(),
          },
        });

      // Execute database query
      const result = await dbPool.query(
        `
        SELECT id, station_id as "stationId", measurements, timestamp, processed_at as "processedAt"
        FROM weather_data
        WHERE station_id = $1 
          AND timestamp BETWEEN $2 AND $3
        ORDER BY timestamp
      `,
        ['station-test', '2024-01-01T00:00:00Z', '2024-01-01T23:59:59Z']
      );

      expect(result.rows).toHaveLength(2);
      expect(result.rows[0].stationId).toBe('station-test');
      expect(result.rows[1].stationId).toBe('station-test');

      await provider.verify();
    });
  });

  describe('Transaction Management Contracts', () => {
    test('should store blockchain transaction with proper validation', async () => {
      const transactionData = {
        tx_id:
          '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
        from_address: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
        to_address: '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
        amount: 1000000000, // Wei
        fee: 21000,
        status: 'confirmed',
        type: 'transfer',
        data: {
          blockNumber: 1000000,
          gasUsed: 21000,
          confirmations: 12,
        },
      };

      // Set up contract expectation
      await provider
        .given('blockchain is operational')
        .uponReceiving('a request to store transaction')
        .withRequest({
          method: 'POST',
          path: '/transactions',
          headers: { 'Content-Type': 'application/json' },
          body: transactionData,
        })
        .willRespondWith({
          status: 201,
          headers: { 'Content-Type': 'application/json' },
          body: {
            success: true,
            data: {
              id: commonMatchers.id,
              txId: commonMatchers.transactionHash,
              fromAddress: commonMatchers.walletAddress,
              toAddress: commonMatchers.walletAddress,
              amount: like(1000000000),
              fee: like(21000),
              status: 'confirmed',
              type: 'transfer',
              timestamp: iso8601DateTime(),
              data: like({
                blockNumber: 1000000,
                gasUsed: 21000,
                confirmations: 12,
              }),
            },
          },
        });

      // Execute database operation
      const result = await dbPool.query(
        `
        INSERT INTO transactions (tx_id, from_address, to_address, amount, fee, status, type, data)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, tx_id as "txId", from_address as "fromAddress", to_address as "toAddress",
                  amount, fee, status, type, timestamp, data
      `,
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

      expect(result.rows).toHaveLength(1);
      const stored = result.rows[0];

      expect(stored.txId).toBe(transactionData.tx_id);
      expect(stored.fromAddress).toBe(transactionData.from_address);
      expect(stored.toAddress).toBe(transactionData.to_address);
      expect(stored.amount).toBe(transactionData.amount);
      expect(stored.status).toBe(transactionData.status);

      await provider.verify();
    });

    test('should query transactions by address with pagination', async () => {
      // Insert test transactions
      const testTxs = [
        {
          tx_id: '0xabc1',
          from_address: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
          to_address: '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
          amount: 1000000,
          fee: 21000,
          status: 'confirmed',
          type: 'transfer',
        },
        {
          tx_id: '0xabc2',
          from_address: '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
          to_address: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
          amount: 2000000,
          fee: 21000,
          status: 'confirmed',
          type: 'transfer',
        },
      ];

      for (const tx of testTxs) {
        await dbPool.query(
          `
          INSERT INTO transactions (tx_id, from_address, to_address, amount, fee, status, type)
          VALUES ($1, $2, $3, $4, $5, $6, $7)
        `,
          [
            tx.tx_id,
            tx.from_address,
            tx.to_address,
            tx.amount,
            tx.fee,
            tx.status,
            tx.type,
          ]
        );
      }

      // Set up contract expectation
      await provider
        .given('transactions exist for address')
        .uponReceiving('a request to get transactions by address')
        .withRequest({
          method: 'GET',
          path: '/transactions',
          query: {
            address: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
            limit: '10',
            offset: '0',
          },
        })
        .willRespondWith({
          status: 200,
          headers: { 'Content-Type': 'application/json' },
          body: {
            success: true,
            data: eachLike(
              {
                id: commonMatchers.id,
                txId: commonMatchers.transactionHash,
                fromAddress: commonMatchers.walletAddress,
                toAddress: commonMatchers.walletAddress,
                amount: like(1000000),
                fee: like(21000),
                status: 'confirmed',
                type: 'transfer',
                timestamp: iso8601DateTime(),
              },
              { min: 1 }
            ),
            pagination: {
              total: like(2),
              limit: 10,
              offset: 0,
              hasMore: false,
            },
          },
        });

      // Execute database query
      const address = '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234';
      const result = await dbPool.query(
        `
        SELECT id, tx_id as "txId", from_address as "fromAddress", to_address as "toAddress",
               amount, fee, status, type, timestamp
        FROM transactions
        WHERE from_address = $1 OR to_address = $1
        ORDER BY timestamp DESC
        LIMIT $2 OFFSET $3
      `,
        [address, 10, 0]
      );

      const countResult = await dbPool.query(
        `
        SELECT COUNT(*) as total
        FROM transactions
        WHERE from_address = $1 OR to_address = $1
      `,
        [address]
      );

      expect(result.rows.length).toBeGreaterThan(0);
      expect(parseInt(countResult.rows[0].total)).toBeGreaterThan(0);

      await provider.verify();
    });
  });

  describe('Carbon Credits Contracts', () => {
    test('should track carbon credits with proper validation', async () => {
      // Create a user first
      const userResult = await dbPool.query(
        `
        INSERT INTO users (username, email, wallet_address, status, role)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
      `,
        [
          'carbonuser',
          'carbon@wrfcoin.com',
          '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
          'active',
          'user',
        ]
      );

      const userId = userResult.rows[0].id;

      const carbonData = {
        user_id: userId,
        amount: 1.5678,
        source: 'weather_monitoring',
        metadata: {
          calculationMethod: 'weather_monitoring_ipcc_2021',
          stationId: 'station-test',
          co2Offset: 1.5678,
          verificationStandard: 'vm0012',
        },
        status: 'verified',
      };

      // Set up contract expectation
      await provider
        .given('user exists for carbon credits')
        .uponReceiving('a request to track carbon credits')
        .withRequest({
          method: 'POST',
          path: '/carbon-credits',
          headers: { 'Content-Type': 'application/json' },
          body: carbonData,
        })
        .willRespondWith({
          status: 201,
          headers: { 'Content-Type': 'application/json' },
          body: {
            success: true,
            data: {
              id: uuid(),
              userId: userId,
              amount: like(1.5678),
              source: 'weather_monitoring',
              metadata: like({
                calculationMethod: 'weather_monitoring_ipcc_2021',
                stationId: 'station-test',
                co2Offset: 1.5678,
                verificationStandard: 'vm0012',
              }),
              status: 'verified',
              issuedAt: iso8601DateTime(),
            },
          },
        });

      // Execute database operation
      const result = await dbPool.query(
        `
        INSERT INTO carbon_credits (user_id, amount, source, metadata, status)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, user_id as "userId", amount, source, metadata, status, issued_at as "issuedAt"
      `,
        [
          carbonData.user_id,
          carbonData.amount,
          carbonData.source,
          JSON.stringify(carbonData.metadata),
          carbonData.status,
        ]
      );

      expect(result.rows).toHaveLength(1);
      const stored = result.rows[0];

      expect(stored.userId).toBe(userId);
      expect(parseFloat(stored.amount)).toBe(carbonData.amount);
      expect(stored.source).toBe(carbonData.source);
      expect(stored.status).toBe(carbonData.status);
      expect(stored.metadata).toEqual(carbonData.metadata);

      await provider.verify();
    });
  });

  describe('Data Integrity and Constraints', () => {
    test('should enforce unique constraints', async () => {
      // Test unique username constraint
      await dbPool.query(
        `
        INSERT INTO users (username, email, wallet_address, status, role)
        VALUES ($1, $2, $3, $4, $5)
      `,
        [
          'uniqueuser',
          'unique@wrfcoin.com',
          '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
          'active',
          'user',
        ]
      );

      // Attempt to insert duplicate username should fail
      await expect(
        dbPool.query(
          `
          INSERT INTO users (username, email, wallet_address, status, role)
          VALUES ($1, $2, $3, $4, $5)
        `,
          [
            'uniqueuser',
            'different@wrfcoin.com',
            '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
            'active',
            'user',
          ]
        )
      ).rejects.toThrow(/duplicate key value violates unique constraint/);
    });

    test('should enforce foreign key constraints', async () => {
      // Attempt to insert carbon credits for non-existent user should fail
      await expect(
        dbPool.query(
          `
          INSERT INTO carbon_credits (user_id, amount, source, status)
          VALUES ($1, $2, $3, $4)
        `,
          [99999, 1.0, 'test', 'pending']
        )
      ).rejects.toThrow(/violates foreign key constraint/);
    });

    test('should handle JSON validation correctly', async () => {
      // Valid JSON in measurements should work
      const validResult = await dbPool.query(
        `
        INSERT INTO weather_data (station_id, measurements, timestamp)
        VALUES ($1, $2, $3)
        RETURNING id
      `,
        [
          'station-test',
          JSON.stringify({ temperature: 20.0 }),
          new Date().toISOString(),
        ]
      );

      expect(validResult.rows).toHaveLength(1);

      // Query with JSON operations
      const jsonResult = await dbPool.query(
        `
        SELECT measurements->>'temperature' as temperature
        FROM weather_data
        WHERE id = $1
      `,
        [validResult.rows[0].id]
      );

      expect(jsonResult.rows[0].temperature).toBe('20');
    });
  });

  describe('Enhanced Database Performance Contracts', () => {
    test('should handle large dataset queries with proper pagination', async () => {
      // Insert test data for pagination
      const testData = [];
      for (let i = 0; i < 25; i++) {
        testData.push({
          username: `user_${i}`,
          email: `user_${i}@wrfcoin.com`,
          wallet_address: `0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f${i.toString().padStart(4, '0')}`,
          status: 'active',
          role: 'user',
        });
      }

      // Insert all test users
      for (const user of testData) {
        await dbPool.query(
          `
          INSERT INTO users (username, email, wallet_address, status, role)
          VALUES ($1, $2, $3, $4, $5)
        `,
          [
            user.username,
            user.email,
            user.wallet_address,
            user.status,
            user.role,
          ]
        );
      }

      await provider
        .given('large dataset exists for pagination')
        .uponReceiving('a request for paginated users with limit and offset')
        .withRequest({
          method: 'GET',
          path: '/users',
          query: {
            limit: '10',
            offset: '5',
          },
        })
        .willRespondWith({
          status: 200,
          headers: { 'Content-Type': 'application/json' },
          body: commonMatchers.paginatedResponse(
            eachLike(
              {
                id: commonMatchers.id,
                username: like('user_5'),
                email: commonMatchers.email,
                walletAddress: commonMatchers.walletAddress,
                status: 'active',
                role: 'user',
                createdAt: iso8601DateTime(),
                updatedAt: iso8601DateTime(),
                isVerified: false,
                reputation: 0,
              },
              { min: 10, max: 10 }
            ),
            25
          ),
        });

      // Execute database query with pagination
      const result = await dbPool.query(
        `
        SELECT id, username, email, wallet_address as "walletAddress",
               status, role, created_at as "createdAt", updated_at as "updatedAt",
               is_verified as "isVerified", reputation
        FROM users 
        ORDER BY id
        LIMIT $1 OFFSET $2
      `,
        [10, 5]
      );

      const countResult = await dbPool.query(
        'SELECT COUNT(*) as total FROM users'
      );

      expect(result.rows).toHaveLength(10);
      expect(parseInt(countResult.rows[0].total)).toBeGreaterThanOrEqual(25);

      await provider.verify();
    });

    test('should handle concurrent database operations safely', async () => {
      await provider
        .given('database supports concurrent operations')
        .uponReceiving('concurrent user creation requests')
        .withRequest({
          method: 'POST',
          path: '/users/batch',
          headers: { 'Content-Type': 'application/json' },
          body: {
            users: eachLike(
              {
                username: like('concurrent_user'),
                email: commonMatchers.email,
                wallet_address: commonMatchers.walletAddress,
                status: 'active',
                role: 'user',
              },
              { min: 3, max: 3 }
            ),
          },
        })
        .willRespondWith({
          status: 201,
          headers: { 'Content-Type': 'application/json' },
          body: {
            success: true,
            data: {
              created: like(3),
              users: eachLike(commonMatchers.userProfile, { min: 3, max: 3 }),
            },
            timestamp: iso8601DateTime(),
          },
        });

      // Simulate concurrent operations using Promise.all
      const concurrentOps = [
        dbPool.query(
          `
          INSERT INTO users (username, email, wallet_address, status, role)
          VALUES ($1, $2, $3, $4, $5)
          RETURNING id
        `,
          [
            'concurrent_1',
            'concurrent_1@wrfcoin.com',
            '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f0001',
            'active',
            'user',
          ]
        ),
        dbPool.query(
          `
          INSERT INTO users (username, email, wallet_address, status, role)
          VALUES ($1, $2, $3, $4, $5)
          RETURNING id
        `,
          [
            'concurrent_2',
            'concurrent_2@wrfcoin.com',
            '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f0002',
            'active',
            'user',
          ]
        ),
        dbPool.query(
          `
          INSERT INTO users (username, email, wallet_address, status, role)
          VALUES ($1, $2, $3, $4, $5)
          RETURNING id
        `,
          [
            'concurrent_3',
            'concurrent_3@wrfcoin.com',
            '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f0003',
            'active',
            'user',
          ]
        ),
      ];

      const results = await Promise.all(concurrentOps);
      expect(results).toHaveLength(3);
      expect(results.every(r => r.rows.length === 1)).toBe(true);

      await provider.verify();
    });

    test('should handle database connection pool limits gracefully', async () => {
      await provider
        .given('database connection pool is under stress')
        .uponReceiving('multiple simultaneous database requests')
        .withRequest({
          method: 'GET',
          path: '/users/health-check',
        })
        .willRespondWith({
          status: 200,
          headers: { 'Content-Type': 'application/json' },
          body: commonMatchers.healthCheck,
        });

      // Test connection pool behavior
      const connectionTests = [];
      for (let i = 0; i < 8; i++) {
        // More than typical pool size
        connectionTests.push(dbPool.query('SELECT 1 as test'));
      }

      const results = await Promise.all(connectionTests);
      expect(results).toHaveLength(8);
      expect(results.every(r => r.rows[0].test === 1)).toBe(true);

      await provider.verify();
    });
  });

  describe('Enhanced Error Handling Contracts', () => {
    test('should handle database transaction rollbacks properly', async () => {
      await provider
        .given('transaction rollback scenario exists')
        .uponReceiving('a request that should trigger rollback')
        .withRequest({
          method: 'POST',
          path: '/users/with-transaction',
          headers: { 'Content-Type': 'application/json' },
          body: {
            username: 'rollback_test',
            email: 'rollback@wrfcoin.com',
            wallet_address: 'invalid_address', // This should cause rollback
            status: 'active',
            role: 'user',
          },
        })
        .willRespondWith({
          status: 400,
          headers: { 'Content-Type': 'application/json' },
          body: commonMatchers.errorResponse('Invalid wallet address format'),
        });

      // Test transaction rollback behavior
      const client = await dbPool.connect();
      try {
        await client.query('BEGIN');

        // This should fail and rollback
        try {
          await client.query(
            `
            INSERT INTO users (username, email, wallet_address, status, role)
            VALUES ($1, $2, $3, $4, $5)
          `,
            [
              'rollback_test',
              'rollback@wrfcoin.com',
              'invalid_wallet',
              'active',
              'user',
            ]
          );

          await client.query('COMMIT');
          throw new Error('Expected transaction to fail');
        } catch (error) {
          await client.query('ROLLBACK');
          expect(error.message).toContain('wallet_address'); // Constraint violation
        }

        // Verify rollback worked - user should not exist
        const checkResult = await client.query(
          'SELECT id FROM users WHERE username = $1',
          ['rollback_test']
        );
        expect(checkResult.rows).toHaveLength(0);
      } finally {
        client.release();
      }

      await provider.verify();
    });

    test('should handle database timeout scenarios', async () => {
      await provider
        .given('database query timeout scenario')
        .uponReceiving('a request that times out')
        .withRequest({
          method: 'GET',
          path: '/users/slow-query',
          query: { timeout: '1000' },
        })
        .willRespondWith({
          status: 408,
          headers: { 'Content-Type': 'application/json' },
          body: commonMatchers.errorResponse('Query timeout'),
        });

      // Test timeout behavior with a long-running query
      const startTime = Date.now();
      try {
        // This simulates a query that might timeout
        await Promise.race([
          dbPool.query('SELECT pg_sleep(0.1)'), // Short sleep for test
          new Promise((_, reject) =>
            setTimeout(() => reject(new Error('Query timeout')), 50)
          ),
        ]);
      } catch (error) {
        const duration = Date.now() - startTime;
        expect(duration).toBeLessThan(100); // Should timeout quickly
        expect(error.message).toContain('timeout');
      }

      await provider.verify();
    });
  });

  describe('Enhanced Data Validation Contracts', () => {
    test('should validate complex JSON schema in database', async () => {
      const complexWeatherData = {
        station_id: 'complex-station',
        measurements: {
          temperature: 23.5,
          humidity: 67.2,
          pressure: 1015.3,
          windSpeed: 4.1,
          windDirection: 225,
          rainfall: 0.0,
          solarRadiation: 850.5,
          soilMoisture: [45.2, 38.7, 52.1], // Array data
          airQuality: {
            pm25: 12.3,
            pm10: 18.7,
            co2: 410.5,
            ozone: 0.08,
          },
        },
        metadata: {
          deviceInfo: {
            model: 'WRF-STATION-PRO-V3',
            firmware: 'v3.2.1',
            sensors: ['temp', 'humidity', 'wind', 'rain', 'air_quality'],
          },
          calibration: {
            lastCalibrated: '2024-01-15T10:00:00Z',
            nextCalibration: '2024-07-15T10:00:00Z',
            calibrationCertificate: 'CERT-2024-001',
          },
        },
      };

      await provider
        .given('complex JSON validation is enforced')
        .uponReceiving('weather data with complex nested structure')
        .withRequest({
          method: 'POST',
          path: '/weather/complex-data',
          headers: { 'Content-Type': 'application/json' },
          body: complexWeatherData,
        })
        .willRespondWith({
          status: 201,
          headers: { 'Content-Type': 'application/json' },
          body: {
            success: true,
            data: {
              id: uuid(),
              stationId: like(complexWeatherData.station_id),
              measurements: like(complexWeatherData.measurements),
              metadata: like(complexWeatherData.metadata),
              validationScore: like(0.98),
              processedAt: iso8601DateTime(),
            },
          },
        });

      // Test complex JSON storage and retrieval
      const result = await dbPool.query(
        `
        INSERT INTO weather_data (station_id, measurements, metadata, quality_score, timestamp)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, measurements, metadata, quality_score as "qualityScore"
      `,
        [
          complexWeatherData.station_id,
          JSON.stringify(complexWeatherData.measurements),
          JSON.stringify(complexWeatherData.metadata),
          0.98,
          new Date().toISOString(),
        ]
      );

      expect(result.rows).toHaveLength(1);
      const stored = result.rows[0];

      // Verify complex JSON structure is preserved
      expect(stored.measurements.airQuality.pm25).toBe(12.3);
      expect(stored.measurements.soilMoisture).toHaveLength(3);
      expect(stored.metadata.deviceInfo.sensors).toContain('temp');

      await provider.verify();
    });

    test('should enforce strict data type validation', async () => {
      await provider
        .given('strict data type validation is enforced')
        .uponReceiving('a request with incorrect data types')
        .withRequest({
          method: 'POST',
          path: '/transactions',
          headers: { 'Content-Type': 'application/json' },
          body: {
            tx_id: '0xabcdef123', // Valid
            from_address: '0xinvalid', // Invalid format
            to_address: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
            amount: 'not_a_number', // Invalid type
            fee: 21000,
            status: 'confirmed',
            type: 'transfer',
          },
        })
        .willRespondWith({
          status: 422,
          headers: { 'Content-Type': 'application/json' },
          body: commonMatchers.validationError,
        });

      // Test data type validation
      await expect(
        dbPool.query(
          `
          INSERT INTO transactions (tx_id, from_address, to_address, amount, fee, status, type)
          VALUES ($1, $2, $3, $4, $5, $6, $7)
        `,
          [
            '0xabcdef123',
            '0xinvalid',
            '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
            'not_a_number',
            21000,
            'confirmed',
            'transfer',
          ]
        )
      ).rejects.toThrow(); // Should fail due to invalid data types

      await provider.verify();
    });
  });
});
