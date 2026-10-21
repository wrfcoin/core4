/**
 * K6 Load Testing Configuration for WRFCoin Platform
 * Supports comprehensive testing of 15+ microservices
 */

// WRFCoin Service Registry for Load Testing
export const SERVICE_REGISTRY = {
  mainApi: { host: 'http://localhost', port: 4001, name: 'Main API Server' },
  crossChain: {
    host: 'http://localhost',
    port: 3000,
    name: 'Cross-Chain Protocol',
  },
  wallet: { host: 'http://localhost', port: 4000, name: 'Wallet Server' },
  compute: { host: 'http://localhost', port: 6000, name: 'Compute Networks' },
  aiml: { host: 'http://localhost', port: 6001, name: 'AI/ML Networks' },
  storage: { host: 'http://localhost', port: 7000, name: 'Storage Networks' },
  iot: { host: 'http://localhost', port: 3002, name: 'IoT Sensor Networks' },
  weatherEarnings: {
    host: 'http://localhost',
    port: 8080,
    name: 'Weather Earnings',
  },
  nodeDiscovery: {
    host: 'http://localhost',
    port: 9001,
    name: 'Node Discovery',
  },
  governance: {
    host: 'http://localhost',
    port: 8000,
    name: 'Governance Server',
  },
  database: { host: 'localhost', port: 5432, name: 'PostgreSQL Database' },
  redis: { host: 'localhost', port: 6379, name: 'Redis Cache' },
};

// Environment configurations
export const ENVIRONMENTS = {
  development: {
    baseUrl: 'http://localhost',
    services: SERVICE_REGISTRY,
    database: 'wrfcoin_dev',
    monitoring: true,
  },
  staging: {
    baseUrl: 'https://staging-api.wrfcoin.io',
    services: Object.fromEntries(
      Object.entries(SERVICE_REGISTRY).map(([key, service]) => [
        key,
        { ...service, host: 'https://staging-api.wrfcoin.io' },
      ])
    ),
    database: 'wrfcoin_staging',
    monitoring: true,
  },
  production: {
    baseUrl: 'https://api.wrfcoin.io',
    services: Object.fromEntries(
      Object.entries(SERVICE_REGISTRY).map(([key, service]) => [
        key,
        { ...service, host: 'https://api.wrfcoin.io' },
      ])
    ),
    database: 'wrfcoin_prod',
    monitoring: true,
  },
};

// Load Testing Scenarios Configuration
export const LOAD_SCENARIOS = {
  // Basic health check
  smoke: {
    executor: 'constant-vus',
    vus: 1,
    duration: '30s',
    tags: { test_type: 'smoke' },
  },

  // Standard load testing
  standard: {
    executor: 'ramping-vus',
    startVUs: 0,
    stages: [
      { duration: '2m', target: 20 }, // Ramp-up
      { duration: '5m', target: 20 }, // Stay at 20
      { duration: '2m', target: 50 }, // Ramp to 50
      { duration: '5m', target: 50 }, // Stay at 50
      { duration: '2m', target: 0 }, // Ramp-down
    ],
    tags: { test_type: 'standard_load' },
  },

  // 2k concurrent sensor posts per minute (PRIMARY REQUIREMENT)
  sensorFlood: {
    executor: 'constant-arrival-rate',
    rate: 2000, // 2000 requests per minute
    timeUnit: '1m',
    duration: '15m',
    preAllocatedVUs: 100,
    maxVUs: 200,
    tags: { test_type: 'sensor_flood' },
  },

  // Stress testing to find breaking point
  stress: {
    executor: 'ramping-vus',
    startVUs: 0,
    stages: [
      { duration: '2m', target: 100 }, // Ramp to 100
      { duration: '5m', target: 100 }, // Stay at 100
      { duration: '2m', target: 200 }, // Ramp to 200
      { duration: '5m', target: 200 }, // Stay at 200
      { duration: '2m', target: 300 }, // Ramp to 300
      { duration: '5m', target: 300 }, // Stay at 300
      { duration: '5m', target: 0 }, // Ramp-down
    ],
    tags: { test_type: 'stress' },
  },

  // Spike testing
  spike: {
    executor: 'ramping-vus',
    startVUs: 0,
    stages: [
      { duration: '10s', target: 100 }, // Quick ramp-up
      { duration: '1m', target: 100 }, // Stay at spike
      { duration: '10s', target: 0 }, // Quick ramp-down
    ],
    tags: { test_type: 'spike' },
  },

  // Volume testing (sustained load)
  volume: {
    executor: 'constant-vus',
    vus: 100,
    duration: '30m',
    tags: { test_type: 'volume' },
  },
};

// Performance Thresholds (SUBTASK 7)
export const PERFORMANCE_THRESHOLDS = {
  // HTTP request duration thresholds
  http_req_duration: [
    'p(50)<200', // 50% of requests under 200ms
    'p(90)<500', // 90% of requests under 500ms
    'p(95)<800', // 95% of requests under 800ms
    'p(99)<2000', // 99% of requests under 2s
    'max<5000', // No request over 5s
  ],

  // HTTP request failure rate
  http_req_failed: [
    'rate<0.01', // Less than 1% error rate
  ],

  // Service-specific thresholds
  iot_sensor_response: ['p(95)<100'], // IoT sensors: <100ms
  main_api_response: ['p(95)<200'], // Main API: <200ms
  cross_chain_response: ['p(95)<1000'], // Cross-chain: <1s
  database_query_time: ['p(95)<500'], // Database: <500ms
  redis_cache_time: ['p(95)<50'], // Redis: <50ms

  // Throughput thresholds
  sensor_posts_per_minute: ['rate>=2000'], // 2k sensor posts/min
  api_requests_per_second: ['rate>=100'], // 100 API req/s

  // System resource thresholds
  cpu_usage: ['value<80'], // <80% CPU
  memory_usage: ['value<85'], // <85% Memory
  disk_usage: ['value<90'], // <90% Disk

  // Custom business metrics
  weather_data_processing_rate: ['rate>=1800'], // Process 1800/min
  transaction_success_rate: ['rate>=0.99'], // 99% success
  carbon_credit_processing_time: ['p(95)<3000'], // <3s processing
};

// Test Data Configuration
export const TEST_DATA = {
  // Weather sensor data templates
  weatherSensors: {
    temperature: { min: -40, max: 60 }, // Celsius
    humidity: { min: 0, max: 100 }, // Percentage
    pressure: { min: 950, max: 1050 }, // hPa
    windSpeed: { min: 0, max: 200 }, // km/h
    windDirection: { min: 0, max: 360 }, // Degrees
    rainfall: { min: 0, max: 500 }, // mm
    solarRadiation: { min: 0, max: 1400 }, // W/m²
    uvIndex: { min: 0, max: 15 }, // Index
  },

  // Geographic locations for sensor distribution
  locations: [
    { lat: 40.7128, lon: -74.006, city: 'New York' },
    { lat: 34.0522, lon: -118.2437, city: 'Los Angeles' },
    { lat: 51.5074, lon: -0.1278, city: 'London' },
    { lat: 35.6762, lon: 139.6503, city: 'Tokyo' },
    { lat: -33.8688, lon: 151.2093, city: 'Sydney' },
    { lat: 55.7558, lon: 37.6176, city: 'Moscow' },
    { lat: 52.52, lon: 13.405, city: 'Berlin' },
    { lat: 19.4326, lon: -99.1332, city: 'Mexico City' },
  ],

  // Test users and wallets
  users: {
    count: 100,
    walletPrefix: '0x',
    tokenPrefix: 'test-token-',
  },

  // Carbon credit projects
  carbonProjects: [
    'SOLAR_FARM_001',
    'WIND_FARM_002',
    'FOREST_CONSERVATION_003',
    'BIOGAS_PLANT_004',
    'HYDRO_POWER_005',
    'GEOTHERMAL_006',
  ],
};

// Monitoring Integration Configuration (SUBTASK 8)
export const MONITORING_CONFIG = {
  // InfluxDB configuration for time-series data
  influxdb: {
    enabled: __ENV.K6_INFLUXDB_ENABLED || 'false',
    url: __ENV.K6_INFLUXDB_URL || 'http://localhost:8086',
    database: __ENV.K6_INFLUXDB_DATABASE || 'k6_metrics',
    username: __ENV.K6_INFLUXDB_USERNAME || 'k6',
    password: __ENV.K6_INFLUXDB_PASSWORD || 'k6password',
    tags: {
      environment: __ENV.ENVIRONMENT || 'development',
      version: __ENV.VERSION || 'unknown',
    },
  },

  // Prometheus pushgateway configuration
  prometheus: {
    enabled: __ENV.K6_PROMETHEUS_ENABLED || 'false',
    pushgateway_url: __ENV.K6_PROMETHEUS_PUSHGATEWAY || 'http://localhost:9091',
  },

  // Custom metrics collection
  customMetrics: {
    enabled: true,
    endpoint: __ENV.MONITORING_ENDPOINT || 'http://localhost:4001/metrics',
    interval: '10s',
  },
};

// Environment Variables
export const ENV_VARS = {
  // Test configuration
  ENVIRONMENT: __ENV.ENVIRONMENT || 'development',
  BASE_URL: __ENV.BASE_URL || 'http://localhost:4001',
  SCENARIO: __ENV.K6_SCENARIO || 'standard',

  // Authentication
  API_TOKEN: __ENV.API_TOKEN || 'test-token-001',
  JWT_SECRET: __ENV.JWT_SECRET || 'your-jwt-secret',

  // Database connections
  DATABASE_URL:
    __ENV.DATABASE_URL || 'postgresql://localhost:5432/wrfcoin_test',
  REDIS_URL: __ENV.REDIS_URL || 'redis://localhost:6379',

  // Service endpoints
  IOT_ENDPOINT: __ENV.IOT_ENDPOINT || 'http://localhost:3002',
  WALLET_ENDPOINT: __ENV.WALLET_ENDPOINT || 'http://localhost:4000',
  BRIDGE_ENDPOINT: __ENV.BRIDGE_ENDPOINT || 'http://localhost:3000',

  // Monitoring
  MONITORING_ENABLED: __ENV.MONITORING_ENABLED || 'true',
  INFLUXDB_ENABLED: __ENV.K6_INFLUXDB_ENABLED || 'false',
  PROMETHEUS_ENABLED: __ENV.K6_PROMETHEUS_ENABLED || 'false',
};

// Global options for K6 tests
export const OPTIONS = {
  // Default scenario
  scenarios: {
    default: LOAD_SCENARIOS[ENV_VARS.SCENARIO] || LOAD_SCENARIOS.standard,
  },

  // Performance thresholds
  thresholds: PERFORMANCE_THRESHOLDS,

  // External outputs for monitoring integration
  ext: {
    loadimpact: {
      name: `WRFCoin Load Test - ${ENV_VARS.SCENARIO}`,
      note: `Testing ${ENV_VARS.ENVIRONMENT} environment`,
    },
  },

  // HTTP configuration
  http: {
    timeout: '60s',
    userAgent: 'K6 WRFCoin Load Test',
  },

  // Test tags
  tags: {
    environment: ENV_VARS.ENVIRONMENT,
    scenario: ENV_VARS.SCENARIO,
    version: __ENV.VERSION || 'unknown',
  },

  // Batch requests configuration
  batch: 10,
  batchPerHost: 5,

  // DNS and TLS configuration
  dns: {
    ttl: '30s',
    select: 'roundRobin',
  },

  tlsAuth: [],
  tlsCipherSuites: [],
  tlsVersion: {
    min: 'TLSv1.2',
    max: 'TLSv1.3',
  },
};

export default {
  SERVICE_REGISTRY,
  ENVIRONMENTS,
  LOAD_SCENARIOS,
  PERFORMANCE_THRESHOLDS,
  TEST_DATA,
  MONITORING_CONFIG,
  ENV_VARS,
  OPTIONS,
};
