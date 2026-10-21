const express = require('express');
const cors = require('cors');

const app = express();
const PORT = process.env.PORT || 3010;

app.use(cors());
app.use(express.json());

// Mock weather station data
const mockStations = {
  'station-1': {
    id: 'station-1',
    name: 'Test Weather Station 1',
    location: { lat: 37.7749, lng: -122.4194, elevation: 52 },
    isActive: true,
  },
  'station-test': {
    id: 'station-test',
    name: 'Test Weather Station',
    location: { lat: 40.7128, lng: -74.006, elevation: 10 },
    isActive: true,
  },
};

const mockWeatherData = {
  'station-1': [
    {
      id: 'weather-1',
      stationId: 'station-1',
      measurements: {
        temperature: 22.5,
        humidity: 65.0,
        pressure: 1013.25,
        windSpeed: 3.2,
        windDirection: 180,
      },
      carbonMetrics: {
        co2Equivalent: 0.05,
        carbonCreditsEarned: 0.005,
      },
      qualityScore: 0.95,
      timestamp: new Date().toISOString(),
      processedAt: new Date().toISOString(),
    },
  ],
};

// Health check endpoint
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    service: 'weather-api-mock',
    timestamp: new Date().toISOString(),
  });
});

// Get weather station by ID
app.get('/api/weather/stations/:stationId', (req, res) => {
  const { stationId } = req.params;
  const station = mockStations[stationId];

  if (!station) {
    return res.status(404).json({
      success: false,
      error: 'Station not found',
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
  const { limit = 100, offset = 0 } = req.query;

  const data = mockWeatherData[stationId] || [];
  const paginatedData = data.slice(offset, offset + parseInt(limit));

  res.json({
    success: true,
    data: paginatedData,
    count: paginatedData.length,
    total: data.length,
    timestamp: new Date().toISOString(),
  });
});

// Submit weather data
app.post('/api/weather/stations/:stationId/data', (req, res) => {
  const { stationId } = req.params;
  const { measurements, metadata } = req.body;

  // Validate required fields
  if (!measurements || typeof measurements !== 'object') {
    return res.status(400).json({
      success: false,
      error: 'Invalid measurements data',
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
      timestamp: new Date().toISOString(),
    });
  }

  // Create new weather data entry
  const newData = {
    id: `weather-${Date.now()}`,
    stationId,
    measurements: {
      temperature: measurements.temperature,
      humidity: measurements.humidity,
      pressure: measurements.pressure || 1013.25,
      windSpeed: measurements.windSpeed || 0,
      windDirection: measurements.windDirection || 0,
    },
    carbonMetrics: {
      co2Equivalent:
        Math.round(
          (measurements.temperature * 0.001 + measurements.humidity * 0.0005) *
            1000
        ) / 1000,
      carbonCreditsEarned: Math.round(Math.random() * 0.01 * 1000) / 1000,
    },
    qualityScore: Math.random() * 0.2 + 0.8, // 0.8 to 1.0
    timestamp: new Date().toISOString(),
    processedAt: new Date().toISOString(),
  };

  // Store in mock data
  if (!mockWeatherData[stationId]) {
    mockWeatherData[stationId] = [];
  }
  mockWeatherData[stationId].unshift(newData);

  // Keep only last 1000 entries per station
  if (mockWeatherData[stationId].length > 1000) {
    mockWeatherData[stationId] = mockWeatherData[stationId].slice(0, 1000);
  }

  res.status(201).json({
    success: true,
    data: newData,
    timestamp: new Date().toISOString(),
  });
});

// Get weather analytics
app.get('/api/weather/analytics/submissions', (req, res) => {
  const { timeframe = '24h' } = req.query;

  res.json({
    success: true,
    data: {
      timeframe,
      totalSubmissions: 1547,
      validSubmissions: 1523,
      invalidSubmissions: 24,
      averageQuality: 94.8,
      rewardsDistributed: '193.875 WRF',
      topContributors: [
        { address: '0x123...abc', submissions: 45, quality: 98.2 },
        { address: '0x456...def', submissions: 38, quality: 96.7 },
      ],
    },
    timestamp: new Date().toISOString(),
  });
});

// List all weather stations
app.get('/api/weather/stations', (req, res) => {
  const stations = Object.values(mockStations);

  res.json({
    success: true,
    data: stations,
    count: stations.length,
    timestamp: new Date().toISOString(),
  });
});

// Error handling
app.use((req, res) => {
  res.status(404).json({
    success: false,
    error: `Endpoint ${req.path} not found`,
    timestamp: new Date().toISOString(),
  });
});

app.use((error, req, res, next) => {
  console.error('Weather API Mock Error:', error);
  res.status(500).json({
    success: false,
    error: 'Internal server error',
    timestamp: new Date().toISOString(),
  });
});

app.listen(PORT, '0.0.0.0', () => {
  console.log(`Weather API Mock running on port ${PORT}`);
  console.log(`Health check: http://localhost:${PORT}/health`);
});
