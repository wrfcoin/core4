/**
 * K6 Database Stress Test
 * Tests PostgreSQL and Redis performance under high load
 *
 * Validates:
 * - Database connection pool management
 * - Query performance under concurrent load
 * - Redis cache effectiveness
 * - Transaction integrity
 * - Connection limits and recovery
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Counter, Rate, Trend, Gauge } from 'k6/metrics';
import {
  randomIntBetween,
  randomItem,
  randomString,
} from 'https://jslib.k6.io/k6-utils/1.2.0/index.js';
import { SERVICE_REGISTRY } from '../k6.config.js';

// Custom Database Metrics
const dbQueryTime = new Trend('database_query_time');
const dbConnectionCount = new Gauge('database_connection_count');
const dbErrorRate = new Rate('database_error_rate');
const redisHitRate = new Rate('redis_cache_hit_rate');
const redisQueryTime = new Trend('redis_query_time');
const dbTransactionRate = new Rate('database_transaction_success_rate');
const connectionPoolUtilization = new Gauge('connection_pool_utilization');

export let options = {
  scenarios: {
    // Database connection pool stress test
    connection_pool_stress: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '1m', target: 50 }, // Ramp up to pool limit
        { duration: '3m', target: 50 }, // Maintain at limit
        { duration: '1m', target: 100 }, // Exceed pool limit
        { duration: '3m', target: 100 }, // Test pool overflow
        { duration: '1m', target: 150 }, // Stress test
        { duration: '2m', target: 150 }, // Sustained stress
        { duration: '2m', target: 0 }, // Cool down
      ],
      tags: { test_type: 'db_connection_stress' },
    },

    // High-frequency read operations
    read_heavy_load: {
      executor: 'constant-vus',
      vus: 80,
      duration: '10m',
      tags: { test_type: 'db_read_heavy' },
    },

    // Write-intensive operations
    write_intensive: {
      executor: 'ramping-arrival-rate',
      startRate: 10,
      stages: [
        { duration: '2m', target: 50 }, // Ramp writes
        { duration: '5m', target: 100 }, // High write load
        { duration: '2m', target: 200 }, // Peak write load
        { duration: '1m', target: 0 }, // Stop writes
      ],
      timeUnit: '1s',
      preAllocatedVUs: 50,
      maxVUs: 100,
      tags: { test_type: 'db_write_intensive' },
    },
  },

  thresholds: {
    database_query_time: ['p(95)<500', 'p(99)<1000'], // Query performance
    database_error_rate: ['rate<0.01'], // <1% DB errors
    redis_query_time: ['p(95)<50', 'p(99)<100'], // Redis performance
    redis_cache_hit_rate: ['rate>=0.80'], // 80% cache hit rate
    database_transaction_success_rate: ['rate>=0.99'], // 99% tx success
    connection_pool_utilization: ['value<=0.90'], // Pool not saturated
    database_connection_count: ['value<=100'], // Connection limit
    'http_req_duration{operation:db_read}': ['p(95)<300'],
    'http_req_duration{operation:db_write}': ['p(95)<800'],
  },
};

// Database test operations
const DB_OPERATIONS = {
  // Weather data queries (read-heavy)
  WEATHER_QUERIES: [
    {
      name: 'recent_readings',
      query:
        "SELECT * FROM weather_readings WHERE timestamp > NOW() - INTERVAL '1 hour' LIMIT 100",
      type: 'read',
      cacheable: true,
    },
    {
      name: 'station_summary',
      query:
        "SELECT station_id, AVG(temperature), COUNT(*) FROM weather_readings WHERE timestamp > NOW() - INTERVAL '24 hours' GROUP BY station_id",
      type: 'read',
      cacheable: true,
    },
    {
      name: 'weather_trends',
      query:
        "SELECT DATE_TRUNC('hour', timestamp) as hour, AVG(temperature), AVG(humidity) FROM weather_readings WHERE timestamp > NOW() - INTERVAL '7 days' GROUP BY hour ORDER BY hour",
      type: 'read',
      cacheable: false,
    },
  ],

  // Transaction operations (write-heavy)
  TRANSACTION_OPERATIONS: [
    {
      name: 'insert_weather_reading',
      type: 'write',
      table: 'weather_readings',
    },
    {
      name: 'update_user_balance',
      type: 'write',
      table: 'user_wallets',
    },
    {
      name: 'insert_transaction',
      type: 'write',
      table: 'transactions',
    },
    {
      name: 'update_station_status',
      type: 'write',
      table: 'weather_stations',
    },
  ],

  // Complex analytical queries
  ANALYTICAL_QUERIES: [
    {
      name: 'reward_calculations',
      query: `
        WITH station_metrics AS (
          SELECT station_id, 
                 COUNT(*) as reading_count,
                 AVG(CASE WHEN quality = 'excellent' THEN 3 WHEN quality = 'good' THEN 2 ELSE 1 END) as quality_score
          FROM weather_readings 
          WHERE timestamp > NOW() - INTERVAL '24 hours'
          GROUP BY station_id
        )
        SELECT s.*, m.reading_count, m.quality_score,
               (m.reading_count * m.quality_score) as reward_points
        FROM weather_stations s
        JOIN station_metrics m ON s.id = m.station_id
        ORDER BY reward_points DESC
      `,
      type: 'read',
      cacheable: false,
    },
  ],
};

export default function () {
  const scenario = randomItem([
    'read_heavy',
    'write_intensive',
    'mixed_operations',
  ]);

  group('Database Stress Testing', function () {
    if (scenario === 'read_heavy' || scenario === 'mixed_operations') {
      group('Read Operations', function () {
        testReadOperations();
      });
    }

    if (scenario === 'write_intensive' || scenario === 'mixed_operations') {
      group('Write Operations', function () {
        testWriteOperations();
      });
    }

    // Always test cache performance
    group('Cache Operations', function () {
      testCacheOperations();
    });

    // Periodically test analytical queries
    if (Math.random() > 0.9) {
      group('Analytical Queries', function () {
        testAnalyticalQueries();
      });
    }
  });

  sleep(randomIntBetween(1, 3));
}

function testReadOperations() {
  const operation = randomItem(DB_OPERATIONS.WEATHER_QUERIES);
  const startTime = Date.now();

  // Check cache first if cacheable
  if (operation.cacheable) {
    const cacheKey = `${operation.name}:${Date.now() % 300000}`; // 5-min cache buckets

    const cacheResponse = http.get(
      `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/cache/${cacheKey}`,
      {
        tags: {
          operation: 'cache_read',
          cache_key: operation.name,
        },
      }
    );

    const cacheHit = cacheResponse.status === 200;
    redisHitRate.add(cacheHit ? 1 : 0);

    if (cacheHit) {
      const cacheTime = Date.now() - startTime;
      redisQueryTime.add(cacheTime);
      return; // Cache hit, no need for DB query
    }
  }

  // Execute database query
  const dbResponse = http.post(
    `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/database/query`,
    JSON.stringify({
      operation: operation.name,
      query: operation.query,
      type: 'read',
      timeout: 10000,
    }),
    {
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${__ENV.API_TOKEN || 'test-token-001'}`,
      },
      tags: {
        operation: 'db_read',
        query_type: operation.name,
      },
    }
  );

  const queryTime = Date.now() - startTime;
  dbQueryTime.add(queryTime);

  const success = check(dbResponse, {
    'DB read successful': r => r.status === 200,
    'Response contains data': r => {
      try {
        const body = JSON.parse(r.body);
        return Array.isArray(body.data) && body.data.length >= 0;
      } catch (e) {
        return false;
      }
    },
    'Query completed within timeout': r => queryTime < 10000,
  });

  dbErrorRate.add(success ? 0 : 1);

  // Update connection metrics
  try {
    const responseBody = JSON.parse(dbResponse.body);
    if (responseBody.connectionInfo) {
      dbConnectionCount.add(responseBody.connectionInfo.activeConnections || 0);
      connectionPoolUtilization.add(
        (responseBody.connectionInfo.activeConnections || 0) /
          (responseBody.connectionInfo.maxConnections || 100)
      );
    }
  } catch (e) {
    // Ignore parsing errors
  }
}

function testWriteOperations() {
  const operation = randomItem(DB_OPERATIONS.TRANSACTION_OPERATIONS);
  const startTime = Date.now();

  let payload;

  switch (operation.name) {
    case 'insert_weather_reading':
      payload = {
        station_id: `WS_${randomIntBetween(1, 100)}`,
        timestamp: new Date().toISOString(),
        temperature: randomIntBetween(-20, 50),
        humidity: randomIntBetween(0, 100),
        pressure: randomIntBetween(980, 1040),
        quality: randomItem(['excellent', 'good', 'fair']),
      };
      break;

    case 'update_user_balance':
      payload = {
        user_id: `user-${randomIntBetween(1, 1000)}`,
        balance_change: randomIntBetween(-100, 500),
        transaction_type: 'reward_payout',
      };
      break;

    case 'insert_transaction':
      payload = {
        from_address: `0x${randomString(40)}`,
        to_address: `0x${randomString(40)}`,
        amount: randomIntBetween(1, 1000).toString(),
        token: 'WRF',
        status: 'pending',
      };
      break;

    case 'update_station_status':
      payload = {
        station_id: `WS_${randomIntBetween(1, 100)}`,
        status: randomItem(['active', 'maintenance', 'offline']),
        last_seen: new Date().toISOString(),
      };
      break;
  }

  const dbResponse = http.post(
    `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/database/execute`,
    JSON.stringify({
      operation: operation.name,
      table: operation.table,
      data: payload,
      type: 'write',
    }),
    {
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${__ENV.API_TOKEN || 'test-token-001'}`,
      },
      tags: {
        operation: 'db_write',
        table: operation.table,
      },
    }
  );

  const writeTime = Date.now() - startTime;
  dbQueryTime.add(writeTime);

  const success = check(dbResponse, {
    'DB write successful': r => r.status === 200 || r.status === 201,
    'Write confirmed': r => {
      try {
        const body = JSON.parse(r.body);
        return body.success === true || body.affected_rows > 0;
      } catch (e) {
        return false;
      }
    },
    'Write completed within timeout': r => writeTime < 5000,
  });

  dbErrorRate.add(success ? 0 : 1);
  dbTransactionRate.add(success ? 1 : 0);
}

function testCacheOperations() {
  const operations = ['get', 'set', 'delete'];
  const operation = randomItem(operations);
  const cacheKey = `test:${randomString(10)}`;
  const startTime = Date.now();

  let response;

  switch (operation) {
    case 'get':
      response = http.get(
        `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/cache/${cacheKey}`,
        { tags: { operation: 'redis_get' } }
      );
      break;

    case 'set':
      response = http.post(
        `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/cache/${cacheKey}`,
        JSON.stringify({
          value: { data: randomString(100), timestamp: Date.now() },
          ttl: 300, // 5 minutes
        }),
        {
          headers: { 'Content-Type': 'application/json' },
          tags: { operation: 'redis_set' },
        }
      );
      break;

    case 'delete':
      response = http.del(
        `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/cache/${cacheKey}`,
        null,
        { tags: { operation: 'redis_delete' } }
      );
      break;
  }

  const cacheTime = Date.now() - startTime;
  redisQueryTime.add(cacheTime);

  const success = check(response, {
    'Cache operation successful': r => r.status >= 200 && r.status < 300,
    'Cache response time acceptable': r => cacheTime < 100,
  });

  if (operation === 'get') {
    redisHitRate.add(response.status === 200 ? 1 : 0);
  }
}

function testAnalyticalQueries() {
  const operation = randomItem(DB_OPERATIONS.ANALYTICAL_QUERIES);
  const startTime = Date.now();

  const analyticsResponse = http.post(
    `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/analytics/execute`,
    JSON.stringify({
      operation: operation.name,
      query: operation.query,
      timeout: 30000, // 30 second timeout for complex queries
    }),
    {
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${__ENV.API_TOKEN || 'test-token-001'}`,
      },
      tags: {
        operation: 'db_analytics',
        query_type: operation.name,
      },
    }
  );

  const analyticsTime = Date.now() - startTime;
  dbQueryTime.add(analyticsTime);

  const success = check(analyticsResponse, {
    'Analytics query successful': r => r.status === 200,
    'Analytics completed in reasonable time': r => analyticsTime < 30000,
    'Results returned': r => {
      try {
        const body = JSON.parse(r.body);
        return body.results !== undefined;
      } catch (e) {
        return false;
      }
    },
  });

  dbErrorRate.add(success ? 0 : 1);
}

export function setup() {
  console.log('🗄️ Starting Database Stress Test');
  console.log('📊 Testing PostgreSQL connection pools and query performance');
  console.log('⚡ Testing Redis cache effectiveness');

  return { startTime: Date.now() };
}

export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`✅ Database stress test completed after ${duration} seconds`);
}

export {
  dbQueryTime,
  dbConnectionCount,
  dbErrorRate,
  redisHitRate,
  redisQueryTime,
  dbTransactionRate,
  connectionPoolUtilization,
};
