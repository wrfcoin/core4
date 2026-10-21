const express = require('express');
const cors = require('cors');
const winston = require('winston');
const fs = require('fs');
const path = require('path');

/**
 * Enhanced Weather API Mock with Scenario Support and Failure Simulation
 *
 * This enhanced version supports:
 * - Multiple test scenarios (default, high-load, failure, degraded)
 * - Configurable failure rates and latency simulation
 * - Chaos engineering support
 * - Comprehensive logging and metrics
 * - Dynamic data generation with realistic patterns
 */

const app = express();
const PORT = process.env.PORT || 3010;

// Configuration from environment variables
const config = {
  scenario: process.env.MOCK_SCENARIO || 'default',
  failureRate: parseFloat(process.env.FAILURE_RATE || 0),
  latencyMs: parseInt(process.env.LATENCY_MS || 0),
  chaosEnabled: process.env.CHAOS_ENABLED === 'true',
  updateInterval: parseInt(process.env.UPDATE_INTERVAL || 30000),
  maxStations: parseInt(process.env.MAX_STATIONS || 100),
};

// Setup logging
const logger = winston.createLogger({
  level: 'info',
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.json()
  ),
  transports: [
    new winston.transports.Console(),
    new winston.transports.File({ filename: '/app/logs/weather-mock.log' }),
  ],
});

// Load scenario configurations
let scenarios = {};
try {
  if (fs.existsSync('./scenarios/weather-scenarios.json')) {
    scenarios = JSON.parse(
      fs.readFileSync('./scenarios/weather-scenarios.json', 'utf8')
    );
  }
} catch (error) {
  logger.warn('Failed to load weather scenarios:', error.message);
}

// Middleware
app.use(cors());
app.use(express.json());
app.use((req, res, next) => {
  logger.info(`${req.method} ${req.path}`, {
    ip: req.ip,
    userAgent: req.get('User-Agent'),
    scenario: config.scenario,
  });
  next();
});

// Failure simulation middleware
app.use((req, res, next) => {
  // Simulate random failures
  if (config.failureRate > 0 && Math.random() < config.failureRate) {
    const errorTypes = [
      { status: 500, message: 'Internal server error' },
      { status: 503, message: 'Service temporarily unavailable' },
      { status: 429, message: 'Too many requests' },
      { status: 408, message: 'Request timeout' },
    ];
    const error = errorTypes[Math.floor(Math.random() * errorTypes.length)];
    logger.warn('Simulating failure', { error, path: req.path });
    return res.status(error.status).json({
      success: false,
      error: error.message,
      code: 'SIMULATED_FAILURE',
      timestamp: new Date().toISOString(),
    });
  }

  // Simulate latency
  if (config.latencyMs > 0) {
    setTimeout(next, config.latencyMs);
  } else {
    next();
  }
});

// Enhanced mock data with realistic patterns
const generateRealisticWeatherData = (stationId, baseTime = new Date()) => {
  const hour = baseTime.getHours();
  const season = Math.floor((baseTime.getMonth() + 1) / 3); // 0=winter, 1=spring, 2=summer, 3=fall

  // Seasonal and diurnal temperature patterns
  const baseTemp = [5, 15, 25, 15][season]; // Seasonal base temperatures
  const diurnalVariation = 10 * Math.sin(((hour - 6) * Math.PI) / 12); // Peak at 2 PM
  const randomVariation = (Math.random() - 0.5) * 4;
  const temperature =
    Math.round((baseTemp + diurnalVariation + randomVariation) * 10) / 10;

  // Humidity inversely related to temperature
  const baseHumidity = 70 - (temperature - 15) * 2;
  const humidity = Math.max(
    20,
    Math.min(95, Math.round(baseHumidity + (Math.random() - 0.5) * 20))
  );

  // Pressure variations
  const pressure =
    Math.round((1013.25 + (Math.random() - 0.5) * 30) * 100) / 100;

  // Wind patterns
  const windSpeed = Math.max(
    0,
    Math.round(
      (Math.random() * 15 + Math.sin((hour * Math.PI) / 12) * 3) * 10
    ) / 10
  );
  const windDirection = Math.floor(Math.random() * 360);

  // Precipitation (seasonal and random)
  const precipitationChance = [0.3, 0.25, 0.15, 0.35][season];
  const precipitation =
    Math.random() < precipitationChance
      ? Math.round(Math.random() * 10 * 100) / 100
      : 0;

  // Quality score based on completeness and reasonableness
  let qualityScore = 0.85;
  if (temperature >= -40 && temperature <= 50) qualityScore += 0.05;
  if (humidity >= 0 && humidity <= 100) qualityScore += 0.05;
  if (pressure >= 980 && pressure <= 1050) qualityScore += 0.05;

  return {
    id: `weather-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
    stationId,
    measurements: {
      temperature,
      humidity,
      pressure,
      windSpeed,
      windDirection,
      precipitation,
      solarRadiation: Math.max(0, 1000 * Math.sin(((hour - 6) * Math.PI) / 12)),
      uvIndex: Math.max(0, Math.min(11, Math.floor(Math.random() * 12))),
    },
    carbonMetrics: {
      co2Equivalent: Math.round((0.1 + Math.random() * 0.05) * 1000) / 1000,
      carbonCreditsEarned: Math.round(Math.random() * 0.02 * 1000) / 1000,
    },
    qualityScore: Math.round(qualityScore * 1000) / 1000,
    timestamp: baseTime.toISOString(),
    processedAt: new Date().toISOString(),
  };
};

// Dynamic mock data storage
const mockStations = new Map();
const mockWeatherData = new Map();
const metrics = {
  requests: 0,
  errors: 0,
  responseTime: [],
  startTime: Date.now(),
};

// Initialize mock stations based on scenario
const initializeMockData = () => {
  const stationCount = scenarios[config.scenario]?.stationCount || 5;

  for (let i = 1; i <= Math.min(stationCount, config.maxStations); i++) {
    const stationId = `station-${i.toString().padStart(3, '0')}`;
    mockStations.set(stationId, {
      id: stationId,
      name: `Test Weather Station ${i}`,
      location: {
        lat: 37.7749 + (Math.random() - 0.5) * 10,
        lng: -122.4194 + (Math.random() - 0.5) * 20,
        elevation: Math.floor(Math.random() * 2000),
      },
      isActive: Math.random() > 0.05, // 95% active
      lastUpdate: new Date().toISOString(),
    });

    // Generate historical data
    const dataPoints = scenarios[config.scenario]?.historyCount || 24;
    const weatherHistory = [];
    for (let h = dataPoints; h >= 0; h--) {
      const timestamp = new Date(Date.now() - h * 3600000); // hourly data
      weatherHistory.push(generateRealisticWeatherData(stationId, timestamp));
    }
    mockWeatherData.set(stationId, weatherHistory);
  }

  logger.info('Mock data initialized', {
    scenario: config.scenario,
    stationCount: mockStations.size,
    totalDataPoints: Array.from(mockWeatherData.values()).reduce(
      (sum, arr) => sum + arr.length,
      0
    ),
  });
};

// Middleware to track metrics
app.use((req, res, next) => {
  const start = Date.now();
  metrics.requests++;

  res.on('finish', () => {
    const duration = Date.now() - start;
    metrics.responseTime.push(duration);

    // Keep only last 1000 response times
    if (metrics.responseTime.length > 1000) {
      metrics.responseTime = metrics.responseTime.slice(-1000);
    }

    if (res.statusCode >= 400) {
      metrics.errors++;
    }
  });

  next();
});

// Health check endpoint with detailed status
app.get('/health', (req, res) => {
  const uptime = Date.now() - metrics.startTime;
  const avgResponseTime =
    metrics.responseTime.length > 0
      ? metrics.responseTime.reduce((a, b) => a + b, 0) /
        metrics.responseTime.length
      : 0;

  const health = {
    status: 'healthy',
    service: 'weather-api-mock',
    version: '2.0.0',
    scenario: config.scenario,
    uptime: Math.floor(uptime / 1000),
    metrics: {
      totalRequests: metrics.requests,
      totalErrors: metrics.errors,
      errorRate:
        metrics.requests > 0
          ? ((metrics.errors / metrics.requests) * 100).toFixed(2) + '%'
          : '0%',
      averageResponseTime: Math.round(avgResponseTime) + 'ms',
    },
    configuration: {
      failureRate: config.failureRate,
      latencyMs: config.latencyMs,
      chaosEnabled: config.chaosEnabled,
      activeStations: mockStations.size,
    },
    timestamp: new Date().toISOString(),
  };

  res.json(health);
});

// Enhanced metrics endpoint
app.get('/metrics', (req, res) => {
  const uptime = Date.now() - metrics.startTime;
  const avgResponseTime =
    metrics.responseTime.length > 0
      ? metrics.responseTime.reduce((a, b) => a + b, 0) /
        metrics.responseTime.length
      : 0;

  const prometheusMetrics = `
# HELP weather_mock_requests_total Total number of requests
# TYPE weather_mock_requests_total counter
weather_mock_requests_total ${metrics.requests}

# HELP weather_mock_errors_total Total number of errors
# TYPE weather_mock_errors_total counter
weather_mock_errors_total ${metrics.errors}

# HELP weather_mock_response_time_ms Average response time in milliseconds
# TYPE weather_mock_response_time_ms gauge
weather_mock_response_time_ms ${Math.round(avgResponseTime)}

# HELP weather_mock_uptime_seconds Service uptime in seconds
# TYPE weather_mock_uptime_seconds counter
weather_mock_uptime_seconds ${Math.floor(uptime / 1000)}

# HELP weather_mock_stations_total Total number of weather stations
# TYPE weather_mock_stations_total gauge
weather_mock_stations_total ${mockStations.size}
`;

  res.set('Content-Type', 'text/plain');
  res.send(prometheusMetrics.trim());
});

// Get all weather stations
app.get('/api/weather/stations', (req, res) => {
  const { limit = 100, offset = 0, active_only = 'false' } = req.query;

  let stations = Array.from(mockStations.values());

  if (active_only === 'true') {
    stations = stations.filter(s => s.isActive);
  }

  const paginatedStations = stations.slice(
    parseInt(offset),
    parseInt(offset) + parseInt(limit)
  );

  res.json({
    success: true,
    data: paginatedStations,
    count: paginatedStations.length,
    total: stations.length,
    timestamp: new Date().toISOString(),
  });
});

// Get weather station by ID
app.get('/api/weather/stations/:stationId', (req, res) => {
  const { stationId } = req.params;
  const station = mockStations.get(stationId);

  if (!station) {
    return res.status(404).json({
      success: false,
      error: 'Station not found',
      code: 'STATION_NOT_FOUND',
      timestamp: new Date().toISOString(),
    });
  }

  res.json({
    success: true,
    data: station,
    timestamp: new Date().toISOString(),
  });
});

// Get weather data for station
app.get('/api/weather/stations/:stationId/data', (req, res) => {
  const { stationId } = req.params;
  const { limit = 100, offset = 0, from_time, to_time } = req.query;

  if (!mockStations.has(stationId)) {
    return res.status(404).json({
      success: false,
      error: 'Station not found',
      code: 'STATION_NOT_FOUND',
      timestamp: new Date().toISOString(),
    });
  }

  let data = mockWeatherData.get(stationId) || [];

  // Filter by time range
  if (from_time) {
    const fromDate = new Date(from_time);
    data = data.filter(d => new Date(d.timestamp) >= fromDate);
  }
  if (to_time) {
    const toDate = new Date(to_time);
    data = data.filter(d => new Date(d.timestamp) <= toDate);
  }

  // Sort by timestamp (newest first)
  data.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp));

  const paginatedData = data.slice(
    parseInt(offset),
    parseInt(offset) + parseInt(limit)
  );

  res.json({
    success: true,
    data: paginatedData,
    count: paginatedData.length,
    total: data.length,
    timestamp: new Date().toISOString(),
  });
});

// Submit weather data with enhanced validation
app.post('/api/weather/stations/:stationId/data', (req, res) => {
  const { stationId } = req.params;
  const { measurements, metadata = {} } = req.body;

  if (!mockStations.has(stationId)) {
    return res.status(404).json({
      success: false,
      error: 'Station not found',
      code: 'STATION_NOT_FOUND',
      timestamp: new Date().toISOString(),
    });
  }

  // Enhanced validation
  if (!measurements || typeof measurements !== 'object') {
    return res.status(400).json({
      success: false,
      error: 'Invalid measurements data',
      code: 'INVALID_MEASUREMENTS',
      timestamp: new Date().toISOString(),
    });
  }

  const requiredFields = ['temperature', 'humidity'];
  const missingFields = requiredFields.filter(
    field => !(field in measurements)
  );

  if (missingFields.length > 0) {
    return res.status(400).json({
      success: false,
      error: `Missing required fields: ${missingFields.join(', ')}`,
      code: 'MISSING_REQUIRED_FIELDS',
      timestamp: new Date().toISOString(),
    });
  }

  // Range validation
  if (measurements.temperature < -50 || measurements.temperature > 60) {
    return res.status(400).json({
      success: false,
      error: 'Temperature out of valid range (-50 to 60°C)',
      code: 'INVALID_TEMPERATURE_RANGE',
      timestamp: new Date().toISOString(),
    });
  }

  if (measurements.humidity < 0 || measurements.humidity > 100) {
    return res.status(400).json({
      success: false,
      error: 'Humidity out of valid range (0 to 100%)',
      code: 'INVALID_HUMIDITY_RANGE',
      timestamp: new Date().toISOString(),
    });
  }

  // Create enhanced weather data entry
  const newData = generateRealisticWeatherData(stationId);

  // Override with submitted measurements
  Object.assign(newData.measurements, measurements);
  newData.metadata = metadata;

  // Store in mock data
  if (!mockWeatherData.has(stationId)) {
    mockWeatherData.set(stationId, []);
  }

  const stationData = mockWeatherData.get(stationId);
  stationData.unshift(newData);

  // Keep only last 10000 entries per station
  if (stationData.length > 10000) {
    stationData.splice(10000);
  }

  // Update station last update time
  const station = mockStations.get(stationId);
  station.lastUpdate = new Date().toISOString();

  res.status(201).json({
    success: true,
    data: newData,
    timestamp: new Date().toISOString(),
  });
});

// Enhanced analytics endpoint
app.get('/api/weather/analytics/submissions', (req, res) => {
  const { timeframe = '24h', station_id } = req.query;

  // Calculate analytics based on current data
  let allData = [];
  if (station_id) {
    allData = mockWeatherData.get(station_id) || [];
  } else {
    for (const data of mockWeatherData.values()) {
      allData.push(...data);
    }
  }

  // Filter by timeframe
  const timeframHours =
    timeframe === '1h'
      ? 1
      : timeframe === '24h'
        ? 24
        : timeframe === '7d'
          ? 168
          : 24;
  const cutoffTime = new Date(Date.now() - timeframHours * 3600000);
  const recentData = allData.filter(d => new Date(d.timestamp) > cutoffTime);

  const totalSubmissions = recentData.length;
  const validSubmissions = recentData.filter(d => d.qualityScore >= 0.8).length;
  const averageQuality =
    recentData.length > 0
      ? recentData.reduce((sum, d) => sum + d.qualityScore, 0) /
        recentData.length
      : 0;

  res.json({
    success: true,
    data: {
      timeframe,
      totalSubmissions,
      validSubmissions,
      invalidSubmissions: totalSubmissions - validSubmissions,
      averageQuality: Math.round(averageQuality * 1000) / 10, // percentage with 1 decimal
      rewardsDistributed: (totalSubmissions * 0.125).toFixed(3) + ' WRF',
      topContributors: Array.from(mockStations.keys())
        .slice(0, 5)
        .map(stationId => ({
          stationId,
          submissions: Math.floor(Math.random() * 50),
          quality: Math.round((0.8 + Math.random() * 0.2) * 1000) / 10,
        })),
    },
    timestamp: new Date().toISOString(),
  });
});

// Scenario management endpoints
app.get('/api/mock/scenario', (req, res) => {
  res.json({
    success: true,
    data: {
      currentScenario: config.scenario,
      availableScenarios: Object.keys(scenarios),
      configuration: config,
    },
    timestamp: new Date().toISOString(),
  });
});

app.post('/api/mock/scenario/:scenarioName', (req, res) => {
  const { scenarioName } = req.params;

  if (!scenarios[scenarioName]) {
    return res.status(400).json({
      success: false,
      error: `Unknown scenario: ${scenarioName}`,
      availableScenarios: Object.keys(scenarios),
      timestamp: new Date().toISOString(),
    });
  }

  config.scenario = scenarioName;

  // Reinitialize data for new scenario
  mockStations.clear();
  mockWeatherData.clear();
  initializeMockData();

  logger.info('Scenario changed', { newScenario: scenarioName });

  res.json({
    success: true,
    message: `Scenario changed to: ${scenarioName}`,
    data: {
      scenario: config.scenario,
      stations: mockStations.size,
    },
    timestamp: new Date().toISOString(),
  });
});

// Chaos engineering endpoint
app.post('/api/mock/chaos', (req, res) => {
  const { action, duration = 30 } = req.body;

  switch (action) {
    case 'enable':
      config.chaosEnabled = true;
      config.failureRate = Math.max(config.failureRate, 0.1);
      setTimeout(() => {
        config.chaosEnabled = false;
        config.failureRate = 0;
      }, duration * 1000);
      break;

    case 'high_latency':
      config.latencyMs = 5000;
      setTimeout(() => {
        config.latencyMs = 0;
      }, duration * 1000);
      break;

    case 'intermittent_failures':
      config.failureRate = 0.5;
      setTimeout(() => {
        config.failureRate = 0;
      }, duration * 1000);
      break;

    default:
      return res.status(400).json({
        success: false,
        error: 'Invalid chaos action',
        supportedActions: ['enable', 'high_latency', 'intermittent_failures'],
        timestamp: new Date().toISOString(),
      });
  }

  logger.warn('Chaos engineering activated', { action, duration });

  res.json({
    success: true,
    message: `Chaos action '${action}' activated for ${duration} seconds`,
    timestamp: new Date().toISOString(),
  });
});

// Error handling
app.use((req, res) => {
  res.status(404).json({
    success: false,
    error: `Endpoint ${req.method} ${req.path} not found`,
    code: 'ENDPOINT_NOT_FOUND',
    timestamp: new Date().toISOString(),
  });
});

app.use((error, req, res, next) => {
  logger.error('Weather API Mock Error:', error);
  res.status(500).json({
    success: false,
    error: 'Internal server error',
    code: 'INTERNAL_SERVER_ERROR',
    timestamp: new Date().toISOString(),
  });
});

// Data update simulation
if (config.updateInterval > 0) {
  setInterval(() => {
    // Add new data points for all active stations
    for (const stationId of mockStations.keys()) {
      if (mockStations.get(stationId).isActive) {
        const newData = generateRealisticWeatherData(stationId);
        const stationData = mockWeatherData.get(stationId);
        stationData.unshift(newData);

        // Keep reasonable history
        if (stationData.length > 1000) {
          stationData.splice(1000);
        }
      }
    }
  }, config.updateInterval);
}

// Initialize and start server
initializeMockData();

app.listen(PORT, '0.0.0.0', () => {
  logger.info('Enhanced Weather API Mock started', {
    port: PORT,
    scenario: config.scenario,
    stations: mockStations.size,
    config,
  });
});

module.exports = app;
