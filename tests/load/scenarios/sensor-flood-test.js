/**
 * K6 Sensor Flood Test - 2,000 Concurrent Sensor Posts Per Minute
 * Primary load test for WRFCoin IoT Sensor Networks (Port 3002)
 *
 * This test validates the system's ability to handle high-volume
 * weather sensor data ingestion as required by Task 43.
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Counter, Rate, Trend, Gauge } from 'k6/metrics';
import {
  randomIntBetween,
  randomItem,
  randomString,
} from 'https://jslib.k6.io/k6-utils/1.2.0/index.js';
import {
  SERVICE_REGISTRY,
  PERFORMANCE_THRESHOLDS,
  TEST_DATA,
} from '../k6.config.js';

// Custom Metrics for Sensor Data Testing
const sensorPostsPerMinute = new Rate('sensor_posts_per_minute');
const weatherDataProcessingTime = new Trend('weather_data_processing_time');
const sensorDataQualityRate = new Rate('sensor_data_quality_rate');
const databaseInsertions = new Counter('database_insertions_total');
const redisUpdates = new Counter('redis_updates_total');
const iotServiceUptime = new Gauge('iot_service_uptime');

// Test Options specifically for 2k concurrent sensor posts
export let options = {
  scenarios: {
    sensor_flood: {
      executor: 'constant-arrival-rate',
      rate: 2000, // 2,000 requests per minute
      timeUnit: '1m', // Per minute
      duration: '15m', // Sustained for 15 minutes
      preAllocatedVUs: 100, // Pre-allocate VUs for stability
      maxVUs: 200, // Maximum VUs if needed
      tags: { test_type: 'sensor_flood', service: 'iot_sensors' },
    },

    // Progressive ramp-up before main flood test
    ramp_up: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '1m', target: 10 }, // Warm-up
        { duration: '2m', target: 50 }, // Initial ramp
        { duration: '2m', target: 100 }, // Build to flood level
      ],
      tags: { test_type: 'ramp_up', service: 'iot_sensors' },
    },
  },

  thresholds: {
    // Sensor-specific thresholds
    'http_req_duration{service:iot_sensors}': ['p(95)<100'], // <100ms for IoT
    'http_req_failed{service:iot_sensors}': ['rate<0.005'], // <0.5% error rate
    sensor_posts_per_minute: ['rate>=2000'], // Meet 2k requirement
    weather_data_processing_time: ['p(95)<50'], // Processing time
    sensor_data_quality_rate: ['rate>=0.99'], // Data quality
    database_insertions_total: ['count>=29000'], // 15min * 2000/min - 1000 buffer
    iot_service_uptime: ['value>=0.99'], // 99% uptime
  },

  // HTTP configuration optimized for sensor data
  http: {
    timeout: '30s',
    userAgent: 'K6-WRFCoin-SensorFlood/1.0',
  },
};

// Weather Station Configuration
const WEATHER_STATIONS = Array.from({ length: 100 }, (_, i) => ({
  id: `WS_${String(i + 1).padStart(3, '0')}`,
  name: `Weather Station ${i + 1}`,
  location: TEST_DATA.locations[i % TEST_DATA.locations.length],
  sensorTypes: [
    'temperature',
    'humidity',
    'pressure',
    'windSpeed',
    'windDirection',
    'rainfall',
  ],
  firmware: `v${randomIntBetween(1, 3)}.${randomIntBetween(0, 9)}.${randomIntBetween(0, 9)}`,
  lastMaintenanceDate: new Date(
    Date.now() - randomIntBetween(1, 90) * 24 * 60 * 60 * 1000
  ).toISOString(),
}));

// Generate realistic sensor data payload
function generateSensorReading(station) {
  const timestamp = new Date().toISOString();
  const readingId = `${station.id}_${Date.now()}_${randomString(8)}`;

  // Add realistic variations and correlations
  const baseTemp = 20 + Math.sin((Date.now() / 86400000) * 2 * Math.PI) * 15; // Daily cycle
  const temperature = baseTemp + randomIntBetween(-5, 5);
  const humidity = Math.max(
    0,
    Math.min(100, 60 + (30 - temperature) * 2 + randomIntBetween(-20, 20))
  );
  const pressure = 1013 + randomIntBetween(-50, 50);

  return {
    readingId,
    stationId: station.id,
    timestamp,
    location: {
      latitude: station.location.lat + (Math.random() - 0.5) * 0.01, // Small GPS variance
      longitude: station.location.lon + (Math.random() - 0.5) * 0.01,
      altitude: randomIntBetween(0, 1000),
    },
    sensors: {
      temperature: {
        value: Math.round(temperature * 10) / 10,
        unit: 'celsius',
        quality: randomItem(['excellent', 'good', 'fair']),
        calibrationDate: station.lastMaintenanceDate,
      },
      humidity: {
        value: Math.round(humidity * 10) / 10,
        unit: 'percent',
        quality: randomItem(['excellent', 'good', 'fair']),
      },
      pressure: {
        value: Math.round(pressure * 10) / 10,
        unit: 'hPa',
        quality: randomItem(['excellent', 'good']),
      },
      windSpeed: {
        value: Math.round(randomIntBetween(0, 50) * 10) / 10,
        unit: 'km/h',
        quality: 'good',
      },
      windDirection: {
        value: randomIntBetween(0, 360),
        unit: 'degrees',
        quality: 'good',
      },
      rainfall: {
        value:
          Math.random() > 0.8 ? Math.round(Math.random() * 10 * 10) / 10 : 0,
        unit: 'mm',
        quality: 'excellent',
      },
    },
    metadata: {
      firmware: station.firmware,
      batteryLevel: randomIntBetween(20, 100),
      signalStrength: randomIntBetween(-90, -30),
      dataIntegrity: 'verified',
      processingFlags: [],
    },
  };
}

// Validate sensor data quality
function validateSensorData(data, response) {
  const isValid =
    data.sensors.temperature.value >= -40 &&
    data.sensors.temperature.value <= 60 &&
    data.sensors.humidity.value >= 0 &&
    data.sensors.humidity.value <= 100 &&
    data.sensors.pressure.value >= 950 &&
    data.sensors.pressure.value <= 1050;

  sensorDataQualityRate.add(isValid ? 1 : 0);

  if (response.status === 201 || response.status === 200) {
    databaseInsertions.add(1);

    // Simulate Redis cache update
    if (Math.random() > 0.1) {
      // 90% cache hit rate
      redisUpdates.add(1);
    }
  }

  return isValid;
}

// Setup function - runs once per VU
export function setup() {
  console.log(
    '🚀 Starting WRFCoin Sensor Flood Test - 2,000 posts/minute target'
  );
  console.log(`📊 Testing ${WEATHER_STATIONS.length} weather stations`);

  // Health check on IoT service
  const healthResponse = http.get(
    `${SERVICE_REGISTRY.iot.host}:${SERVICE_REGISTRY.iot.port}/health`
  );
  const isHealthy = check(healthResponse, {
    'IoT service is healthy': r => r.status === 200,
  });

  iotServiceUptime.add(isHealthy ? 1 : 0);

  if (!isHealthy) {
    console.error('❌ IoT service health check failed');
  }

  return {
    stations: WEATHER_STATIONS,
    startTime: Date.now(),
  };
}

// Main test function - runs for each iteration
export default function (data) {
  const station = randomItem(data.stations);
  const sensorReading = generateSensorReading(station);

  group('IoT Sensor Data Submission', function () {
    const startTime = Date.now();

    // Submit sensor reading to IoT Networks service
    const response = http.post(
      `${SERVICE_REGISTRY.iot.host}:${SERVICE_REGISTRY.iot.port}/api/sensor/readings`,
      JSON.stringify(sensorReading),
      {
        headers: {
          'Content-Type': 'application/json',
          Accept: 'application/json',
          'X-Station-ID': station.id,
          'X-Data-Format': 'wrfcoin-v1',
          Authorization: `Bearer ${__ENV.API_TOKEN || 'test-token-001'}`,
        },
        tags: {
          service: 'iot_sensors',
          station: station.id,
          endpoint: 'sensor_readings',
        },
      }
    );

    const processingTime = Date.now() - startTime;
    weatherDataProcessingTime.add(processingTime);

    // Comprehensive checks for sensor data submission
    const checks = check(response, {
      'Sensor post successful': r => r.status === 200 || r.status === 201,
      'Response time < 100ms': r => r.timings.duration < 100,
      'Response has reading ID': r => {
        try {
          const body = JSON.parse(r.body);
          return body.readingId !== undefined;
        } catch (e) {
          return false;
        }
      },
      'Data processing confirmed': r => {
        try {
          const body = JSON.parse(r.body);
          return body.status === 'processed' || body.status === 'accepted';
        } catch (e) {
          return false;
        }
      },
    });

    // Track successful sensor posts
    sensorPostsPerMinute.add(
      response.status === 200 || response.status === 201 ? 1 : 0
    );

    // Validate data quality
    validateSensorData(sensorReading, response);

    // Log errors for debugging
    if (response.status >= 400) {
      console.error(
        `❌ Sensor post failed: ${response.status} - ${response.body}`
      );
    }
  });

  // Optional: Test real-time data retrieval
  if (Math.random() > 0.95) {
    // 5% chance to test data retrieval
    group('Real-time Data Retrieval', function () {
      const retrievalResponse = http.get(
        `${SERVICE_REGISTRY.iot.host}:${SERVICE_REGISTRY.iot.port}/api/sensor/stations/${station.id}/latest`,
        {
          headers: {
            Accept: 'application/json',
            Authorization: `Bearer ${__ENV.API_TOKEN || 'test-token-001'}`,
          },
          tags: {
            service: 'iot_sensors',
            station: station.id,
            endpoint: 'latest_reading',
          },
        }
      );

      check(retrievalResponse, {
        'Data retrieval successful': r => r.status === 200,
        'Latest data available': r => {
          try {
            const body = JSON.parse(r.body);
            return body.timestamp !== undefined;
          } catch (e) {
            return false;
          }
        },
      });
    });
  }

  // Small sleep to prevent overwhelming (adjust based on system capacity)
  sleep(0.01); // 10ms sleep
}

// Teardown function - runs after all iterations
export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`✅ Sensor flood test completed after ${duration} seconds`);
  console.log(
    `📈 Targeted 2,000 sensor posts per minute for ${duration / 60} minutes`
  );

  // Final health check
  const finalHealthResponse = http.get(
    `${SERVICE_REGISTRY.iot.host}:${SERVICE_REGISTRY.iot.port}/health`
  );
  const finalHealth = check(finalHealthResponse, {
    'IoT service healthy after test': r => r.status === 200,
  });

  console.log(
    finalHealth ? '✅ IoT service remained healthy' : '⚠️ IoT service degraded'
  );
}

// Export custom metrics for monitoring integration
export {
  sensorPostsPerMinute,
  weatherDataProcessingTime,
  sensorDataQualityRate,
  databaseInsertions,
  redisUpdates,
  iotServiceUptime,
};
