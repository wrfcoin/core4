#!/usr/bin/env node

/**
 * Enhanced Weather Oracle Mock Server
 * Provides comprehensive weather data API with real-time updates, WebSocket support,
 * and advanced features for testing various weather scenarios and edge cases
 */

const express = require('express');
const cors = require('cors');
const WebSocket = require('ws');
const { v4: uuidv4 } = require('uuid');
const moment = require('moment');

const app = express();
const PORT = process.env.PORT || 3010;
const WS_PORT = process.env.WS_PORT || 3011;
const GRPC_PORT = process.env.GRPC_PORT || 3012;

// Configuration
const config = {
  stationCount: parseInt(process.env.STATION_COUNT) || 50,
  updateIntervalSec: parseInt(process.env.UPDATE_INTERVAL_SEC) || 30,
  historicalDays: parseInt(process.env.HISTORICAL_DAYS) || 365,
  enableExtremeWeather: process.env.ENABLE_EXTREME_WEATHER === 'true',
  enableRealTimeUpdates: process.env.ENABLE_REAL_TIME_UPDATES === 'true',
  simulateSensorFailures: process.env.SIMULATE_SENSOR_FAILURES === 'true',
};

// Weather station types and their characteristics
const stationTypes = [
  {
    type: 'AWS',
    name: 'Automatic Weather Station',
    accuracy: 0.95,
    sensors: ['temp', 'humidity', 'pressure', 'wind', 'rain', 'solar'],
  },
  {
    type: 'PWS',
    name: 'Personal Weather Station',
    accuracy: 0.85,
    sensors: ['temp', 'humidity', 'pressure', 'wind', 'rain'],
  },
  {
    type: 'RAWS',
    name: 'Remote Automatic Weather Station',
    accuracy: 0.9,
    sensors: ['temp', 'humidity', 'wind', 'solar'],
  },
  {
    type: 'AWOS',
    name: 'Automated Weather Observing System',
    accuracy: 0.98,
    sensors: ['temp', 'humidity', 'pressure', 'wind', 'visibility', 'cloud'],
  },
  {
    type: 'ASOS',
    name: 'Automated Surface Observing System',
    accuracy: 0.98,
    sensors: [
      'temp',
      'humidity',
      'pressure',
      'wind',
      'rain',
      'visibility',
      'cloud',
    ],
  },
  {
    type: 'METAR',
    name: 'Aviation Weather Station',
    accuracy: 0.99,
    sensors: [
      'temp',
      'humidity',
      'pressure',
      'wind',
      'visibility',
      'cloud',
      'precipitation',
    ],
  },
];

// Global weather patterns and climate zones
const climateZones = [
  {
    zone: 'tropical',
    baseTemp: 26,
    tempVar: 8,
    humidityBase: 80,
    humidityVar: 15,
  },
  {
    zone: 'subtropical',
    baseTemp: 22,
    tempVar: 12,
    humidityBase: 65,
    humidityVar: 20,
  },
  {
    zone: 'temperate',
    baseTemp: 15,
    tempVar: 20,
    humidityBase: 60,
    humidityVar: 25,
  },
  {
    zone: 'continental',
    baseTemp: 8,
    tempVar: 30,
    humidityBase: 55,
    humidityVar: 30,
  },
  {
    zone: 'polar',
    baseTemp: -5,
    tempVar: 15,
    humidityBase: 75,
    humidityVar: 15,
  },
  {
    zone: 'desert',
    baseTemp: 28,
    tempVar: 25,
    humidityBase: 25,
    humidityVar: 20,
  },
  {
    zone: 'mediterranean',
    baseTemp: 18,
    tempVar: 15,
    humidityBase: 58,
    humidityVar: 22,
  },
  {
    zone: 'oceanic',
    baseTemp: 12,
    tempVar: 12,
    humidityBase: 78,
    humidityVar: 15,
  },
];

// Extreme weather event types
const extremeWeatherEvents = [
  { type: 'heat_wave', tempIncrease: 15, duration: 5, probability: 0.02 },
  { type: 'cold_snap', tempDecrease: 20, duration: 3, probability: 0.02 },
  { type: 'drought', rainDecrease: 0.9, duration: 30, probability: 0.01 },
  { type: 'flood', rainIncrease: 10, duration: 2, probability: 0.01 },
  {
    type: 'hurricane',
    windIncrease: 50,
    rainIncrease: 5,
    duration: 1,
    probability: 0.005,
  },
  {
    type: 'blizzard',
    tempDecrease: 15,
    windIncrease: 30,
    duration: 2,
    probability: 0.01,
  },
];

// Mock weather stations
const mockStations = [];
const stationData = new Map(); // Current readings
const historicalData = new Map(); // Historical readings
const activeEvents = new Map(); // Active extreme weather events

// WebSocket connections
const wsConnections = new Set();

// Initialize mock weather stations
function initializeStations() {
  for (let i = 0; i < config.stationCount; i++) {
    const stationType =
      stationTypes[Math.floor(Math.random() * stationTypes.length)];
    const climateZone =
      climateZones[Math.floor(Math.random() * climateZones.length)];

    const station = {
      id: `WRF-${String(i + 1).padStart(4, '0')}`,
      name: `Weather Station ${i + 1}`,
      type: stationType.type,
      typeName: stationType.name,
      location: {
        latitude: (Math.random() - 0.5) * 180,
        longitude: (Math.random() - 0.5) * 360,
        altitude: Math.floor(Math.random() * 3000),
        country: ['US', 'CA', 'UK', 'DE', 'JP', 'AU', 'BR', 'IN', 'CN', 'RU'][
          Math.floor(Math.random() * 10)
        ],
        region: `Region-${Math.floor(Math.random() * 20) + 1}`,
        city: `City-${Math.floor(Math.random() * 100) + 1}`,
      },
      climate: climateZone.zone,
      sensors: stationType.sensors,
      accuracy: stationType.accuracy,
      status: 'online',
      lastUpdate: new Date().toISOString(),
      installDate: moment()
        .subtract(Math.floor(Math.random() * 365 * 5), 'days')
        .toISOString(),
      owner: ['gov', 'private', 'research', 'community'][
        Math.floor(Math.random() * 4)
      ],
      dataQuality: Math.random() * 0.3 + 0.7, // 0.7 to 1.0
      calibrationDate: moment()
        .subtract(Math.floor(Math.random() * 365), 'days')
        .toISOString(),
    };

    mockStations.push(station);

    // Initialize current data
    stationData.set(station.id, generateWeatherData(station, climateZone));

    // Initialize historical data
    const historical = [];
    for (let days = config.historicalDays; days > 0; days--) {
      const timestamp = moment().subtract(days, 'days');
      historical.push(generateWeatherData(station, climateZone, timestamp));
    }
    historicalData.set(station.id, historical);
  }

  console.log(`Initialized ${config.stationCount} mock weather stations`);
}

// Generate realistic weather data
function generateWeatherData(station, climateZone, timestamp = null) {
  const time = timestamp || moment();
  const seasonFactor = Math.sin((time.dayOfYear() / 365) * 2 * Math.PI);
  const dailyFactor = Math.sin((time.hour() / 24) * 2 * Math.PI);

  // Base temperature with seasonal and daily variations
  let temperature =
    climateZone.baseTemp +
    seasonFactor * climateZone.tempVar * 0.5 +
    dailyFactor * climateZone.tempVar * 0.3 +
    (Math.random() - 0.5) * climateZone.tempVar * 0.4;

  // Base humidity
  let humidity = Math.max(
    10,
    Math.min(
      100,
      climateZone.humidityBase + (Math.random() - 0.5) * climateZone.humidityVar
    )
  );

  // Atmospheric pressure (varies with altitude and weather)
  let pressure =
    1013.25 - station.location.altitude * 0.12 + (Math.random() - 0.5) * 30;

  // Wind speed and direction
  const windSpeed = Math.max(
    0,
    Math.random() * 25 + (Math.random() - 0.5) * 10
  );
  const windDirection = Math.floor(Math.random() * 360);
  const windGust = windSpeed + Math.random() * 10;

  // Precipitation
  let precipitation = 0;
  if (Math.random() < 0.3) {
    // 30% chance of precipitation
    precipitation = Math.random() * 10;
  }

  // Solar radiation (affected by time of day and weather)
  let solarRadiation = 0;
  if (time.hour() >= 6 && time.hour() <= 18) {
    const solarFactor = Math.sin(((time.hour() - 6) / 12) * Math.PI);
    solarRadiation = solarFactor * 1000 * (1 - precipitation * 0.1);
  }

  // UV Index
  const uvIndex = Math.max(0, Math.min(11, solarRadiation / 100));

  // Visibility (km)
  let visibility = 10 - precipitation * 2 - humidity / 20;
  visibility = Math.max(0.1, visibility);

  // Cloud cover percentage
  const cloudCover = Math.min(100, 100 - visibility * 8 + Math.random() * 30);

  // Apply extreme weather events if enabled
  const activeEvent = activeEvents.get(station.id);
  if (config.enableExtremeWeather && activeEvent) {
    temperature += activeEvent.tempModifier || 0;
    humidity += activeEvent.humidityModifier || 0;
    windSpeed += activeEvent.windModifier || 0;
    precipitation += activeEvent.rainModifier || 0;
  }

  // Simulate sensor accuracy
  const accuracy = station.accuracy;
  const addNoise = (value, range) =>
    value + (Math.random() - 0.5) * range * (1 - accuracy);

  const data = {
    stationId: station.id,
    timestamp: time.toISOString(),
    temperature: addNoise(temperature, 2),
    humidity: Math.max(0, Math.min(100, addNoise(humidity, 5))),
    pressure: addNoise(pressure, 5),
    windSpeed: Math.max(0, addNoise(windSpeed, 2)),
    windDirection: Math.max(
      0,
      Math.min(359, Math.floor(addNoise(windDirection, 20)))
    ),
    windGust: Math.max(0, addNoise(windGust, 3)),
    precipitation: Math.max(0, addNoise(precipitation, 0.5)),
    solarRadiation: Math.max(0, addNoise(solarRadiation, 50)),
    uvIndex: Math.max(0, Math.min(11, addNoise(uvIndex, 0.5))),
    visibility: Math.max(0.1, addNoise(visibility, 1)),
    cloudCover: Math.max(0, Math.min(100, addNoise(cloudCover, 10))),
    dewPoint: temperature - (100 - humidity) / 5,
    heatIndex: calculateHeatIndex(temperature, humidity),
    windChill: calculateWindChill(temperature, windSpeed),
    dataQuality: station.dataQuality + (Math.random() - 0.5) * 0.1,
    sensorStatus: generateSensorStatus(station.sensors),
    metadata: {
      source: 'WRFCoin Weather Oracle',
      version: '2.1.0',
      processingTime: Math.random() * 100,
      validationFlags: generateValidationFlags(),
    },
  };

  // Simulate sensor failures if enabled
  if (config.simulateSensorFailures && Math.random() < 0.05) {
    const failedSensor =
      station.sensors[Math.floor(Math.random() * station.sensors.length)];
    data.sensorStatus[failedSensor] = 'failed';

    // Set failed sensor data to null
    switch (failedSensor) {
      case 'temp':
        data.temperature = null;
        break;
      case 'humidity':
        data.humidity = null;
        break;
      case 'pressure':
        data.pressure = null;
        break;
      case 'wind':
        data.windSpeed = data.windDirection = data.windGust = null;
        break;
      case 'rain':
        data.precipitation = null;
        break;
      case 'solar':
        data.solarRadiation = data.uvIndex = null;
        break;
    }
  }

  return data;
}

// Utility functions
function calculateHeatIndex(temp, humidity) {
  if (temp < 27) return temp;

  const c1 = -8.78469475556;
  const c2 = 1.61139411;
  const c3 = 2.33854883889;
  const c4 = -0.14611605;
  const c5 = -0.012308094;
  const c6 = -0.0164248277778;
  const c7 = 0.002211732;
  const c8 = 0.00072546;
  const c9 = -0.000003582;

  return (
    c1 +
    c2 * temp +
    c3 * humidity +
    c4 * temp * humidity +
    c5 * temp * temp +
    c6 * humidity * humidity +
    c7 * temp * temp * humidity +
    c8 * temp * humidity * humidity +
    c9 * temp * temp * humidity * humidity
  );
}

function calculateWindChill(temp, windSpeed) {
  if (temp > 10 || windSpeed < 4.8) return temp;

  return (
    13.12 +
    0.6215 * temp -
    11.37 * Math.pow(windSpeed, 0.16) +
    0.3965 * temp * Math.pow(windSpeed, 0.16)
  );
}

function generateSensorStatus(sensors) {
  const status = {};
  sensors.forEach(sensor => {
    const reliability = Math.random();
    if (reliability > 0.95) status[sensor] = 'excellent';
    else if (reliability > 0.85) status[sensor] = 'good';
    else if (reliability > 0.7) status[sensor] = 'fair';
    else if (reliability > 0.5) status[sensor] = 'poor';
    else status[sensor] = 'failed';
  });
  return status;
}

function generateValidationFlags() {
  const flags = [];
  if (Math.random() > 0.9) flags.push('quality_check_passed');
  if (Math.random() > 0.95) flags.push('calibration_verified');
  if (Math.random() > 0.8) flags.push('data_consistency_ok');
  if (Math.random() < 0.1) flags.push('anomaly_detected');
  if (Math.random() < 0.05) flags.push('maintenance_required');
  return flags;
}

// Middleware
app.use(
  cors({
    origin: '*',
    methods: ['GET', 'POST', 'PUT', 'DELETE', 'OPTIONS'],
    allowedHeaders: ['Content-Type', 'Authorization', 'X-API-Key'],
  })
);
app.use(express.json());

// Health check endpoint
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    service: process.env.SERVICE_NAME || 'weather-oracle-enhanced',
    timestamp: new Date().toISOString(),
    config: {
      stations: config.stationCount,
      updateInterval: config.updateIntervalSec + 's',
      extremeWeather: config.enableExtremeWeather,
      realTimeUpdates: config.enableRealTimeUpdates,
    },
    stats: {
      totalStations: mockStations.length,
      onlineStations: mockStations.filter(s => s.status === 'online').length,
      activeEvents: activeEvents.size,
      wsConnections: wsConnections.size,
    },
  });
});

// List all weather stations
app.get('/api/weather/stations', (req, res) => {
  const { type, climate, country, status, limit = 50, offset = 0 } = req.query;

  let stations = [...mockStations];

  // Apply filters
  if (type) stations = stations.filter(s => s.type === type);
  if (climate) stations = stations.filter(s => s.climate === climate);
  if (country) stations = stations.filter(s => s.location.country === country);
  if (status) stations = stations.filter(s => s.status === status);

  // Pagination
  const total = stations.length;
  stations = stations.slice(offset, offset + parseInt(limit));

  res.json({
    stations,
    pagination: {
      total,
      limit: parseInt(limit),
      offset: parseInt(offset),
      hasMore: offset + stations.length < total,
    },
    filters: {
      availableTypes: [...new Set(mockStations.map(s => s.type))],
      availableClimates: [...new Set(mockStations.map(s => s.climate))],
      availableCountries: [
        ...new Set(mockStations.map(s => s.location.country)),
      ],
    },
  });
});

// Get specific weather station
app.get('/api/weather/stations/:stationId', (req, res) => {
  const { stationId } = req.params;
  const station = mockStations.find(s => s.id === stationId);

  if (!station) {
    return res.status(404).json({
      error: 'Station not found',
      stationId,
    });
  }

  const currentData = stationData.get(stationId);
  const activeEvent = activeEvents.get(stationId);

  res.json({
    ...station,
    currentData,
    activeEvent,
    nearbyStations: mockStations
      .filter(s => s.id !== stationId)
      .map(s => ({
        id: s.id,
        name: s.name,
        distance: calculateDistance(station.location, s.location),
      }))
      .sort((a, b) => a.distance - b.distance)
      .slice(0, 5),
  });
});

// Get weather data for a station
app.get('/api/weather/stations/:stationId/data', (req, res) => {
  const { stationId } = req.params;
  const {
    start,
    end,
    interval = 'hourly',
    sensors,
    format = 'json',
    includeMetadata = false,
  } = req.query;

  const station = mockStations.find(s => s.id === stationId);
  if (!station) {
    return res.status(404).json({
      error: 'Station not found',
      stationId,
    });
  }

  let data;

  if (start || end) {
    // Historical data request
    const historical = historicalData.get(stationId) || [];
    data = historical.filter(d => {
      const timestamp = moment(d.timestamp);
      const isAfterStart = !start || timestamp.isAfter(moment(start));
      const isBeforeEnd = !end || timestamp.isBefore(moment(end));
      return isAfterStart && isBeforeEnd;
    });
  } else {
    // Current data
    data = [stationData.get(stationId)];
  }

  // Filter sensors if requested
  if (sensors) {
    const requestedSensors = sensors.split(',');
    data = data.map(d => {
      const filtered = { stationId: d.stationId, timestamp: d.timestamp };
      requestedSensors.forEach(sensor => {
        if (d[sensor] !== undefined) filtered[sensor] = d[sensor];
      });
      if (includeMetadata) filtered.metadata = d.metadata;
      return filtered;
    });
  }

  // Format response
  if (format === 'csv') {
    res.setHeader('Content-Type', 'text/csv');
    res.setHeader(
      'Content-Disposition',
      `attachment; filename=${stationId}_data.csv`
    );

    if (data.length === 0) return res.send('');

    const headers = Object.keys(data[0]).join(',');
    const rows = data.map(d => Object.values(d).join(','));
    res.send([headers, ...rows].join('\n'));
  } else {
    res.json({
      station: {
        id: station.id,
        name: station.name,
        type: station.type,
      },
      data,
      count: data.length,
      interval,
      sensors: sensors ? sensors.split(',') : station.sensors,
    });
  }
});

// Submit weather data (for testing data ingestion)
app.post('/api/weather/stations/:stationId/data', (req, res) => {
  const { stationId } = req.params;
  const data = req.body;

  const station = mockStations.find(s => s.id === stationId);
  if (!station) {
    return res.status(404).json({
      error: 'Station not found',
      stationId,
    });
  }

  // Validate data structure
  const requiredFields = ['timestamp', 'temperature', 'humidity', 'pressure'];
  const missingFields = requiredFields.filter(
    field => !data.hasOwnProperty(field)
  );

  if (missingFields.length > 0) {
    return res.status(400).json({
      error: 'Missing required fields',
      missingFields,
    });
  }

  // Store the data
  data.stationId = stationId;
  data.receivedAt = new Date().toISOString();
  stationData.set(stationId, data);

  // Add to historical data
  const historical = historicalData.get(stationId) || [];
  historical.push(data);
  // Keep only last 1000 entries
  if (historical.length > 1000) {
    historical.shift();
  }
  historicalData.set(stationId, historical);

  // Broadcast to WebSocket connections
  wsConnections.forEach(ws => {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(
        JSON.stringify({
          type: 'weather_update',
          stationId,
          data,
        })
      );
    }
  });

  res.json({
    success: true,
    stationId,
    receivedAt: data.receivedAt,
    nextExpectedUpdate: moment()
      .add(config.updateIntervalSec, 'seconds')
      .toISOString(),
  });
});

// Weather analytics endpoints
app.get('/api/weather/analytics/submissions', (req, res) => {
  const { days = 7, groupBy = 'station' } = req.query;

  const submissions = [];
  const startDate = moment().subtract(days, 'days');

  mockStations.forEach(station => {
    const stationSubmissions = Math.floor(Math.random() * days * 24); // Random submissions
    submissions.push({
      stationId: station.id,
      stationName: station.name,
      submissions: stationSubmissions,
      period: `${days} days`,
      avgPerDay: (stationSubmissions / days).toFixed(1),
      quality: station.dataQuality,
      reliability: Math.random() * 0.3 + 0.7,
    });
  });

  res.json({
    summary: {
      totalSubmissions: submissions.reduce((sum, s) => sum + s.submissions, 0),
      totalStations: submissions.length,
      period: `${days} days`,
      avgPerStation: (
        submissions.reduce((sum, s) => sum + s.submissions, 0) /
        submissions.length
      ).toFixed(1),
    },
    stations: submissions.sort((a, b) => b.submissions - a.submissions),
  });
});

// Extreme weather events
app.get('/api/weather/events', (req, res) => {
  const events = Array.from(activeEvents.entries()).map(
    ([stationId, event]) => {
      const station = mockStations.find(s => s.id === stationId);
      return {
        id: event.id,
        stationId,
        stationName: station ? station.name : 'Unknown',
        type: event.type,
        startTime: event.startTime,
        expectedEndTime: event.expectedEndTime,
        severity: event.severity,
        description: event.description,
      };
    }
  );

  res.json({
    activeEvents: events,
    count: events.length,
  });
});

// Trigger extreme weather event (for testing)
app.post('/api/weather/events', (req, res) => {
  const { stationId, eventType, duration, severity = 'moderate' } = req.body;

  if (!stationId || !eventType) {
    return res.status(400).json({
      error: 'Station ID and event type are required',
    });
  }

  const station = mockStations.find(s => s.id === stationId);
  if (!station) {
    return res.status(404).json({
      error: 'Station not found',
      stationId,
    });
  }

  const eventConfig = extremeWeatherEvents.find(e => e.type === eventType);
  if (!eventConfig) {
    return res.status(400).json({
      error: 'Invalid event type',
      availableTypes: extremeWeatherEvents.map(e => e.type),
    });
  }

  const event = {
    id: uuidv4(),
    type: eventType,
    stationId,
    severity,
    startTime: new Date().toISOString(),
    expectedEndTime: moment()
      .add(duration || eventConfig.duration, 'hours')
      .toISOString(),
    description: `${eventType.replace('_', ' ')} event at ${station.name}`,
    tempModifier: eventConfig.tempIncrease
      ? eventConfig.tempIncrease
      : eventConfig.tempDecrease
        ? -eventConfig.tempDecrease
        : 0,
    humidityModifier: 0,
    windModifier: eventConfig.windIncrease || 0,
    rainModifier: eventConfig.rainIncrease
      ? eventConfig.rainIncrease
      : eventConfig.rainDecrease
        ? -eventConfig.rainDecrease
        : 0,
  };

  // Adjust modifiers based on severity
  const severityMultiplier =
    severity === 'extreme' ? 2 : severity === 'severe' ? 1.5 : 1;
  event.tempModifier *= severityMultiplier;
  event.windModifier *= severityMultiplier;
  event.rainModifier *= severityMultiplier;

  activeEvents.set(stationId, event);

  // Auto-remove event after duration
  setTimeout(
    () => {
      activeEvents.delete(stationId);
    },
    (duration || eventConfig.duration) * 60 * 60 * 1000
  );

  res.json({
    success: true,
    event,
  });
});

// Helper function to calculate distance between two points
function calculateDistance(pos1, pos2) {
  const R = 6371; // Earth radius in km
  const dLat = ((pos2.latitude - pos1.latitude) * Math.PI) / 180;
  const dLon = ((pos2.longitude - pos1.longitude) * Math.PI) / 180;
  const a =
    Math.sin(dLat / 2) * Math.sin(dLat / 2) +
    Math.cos((pos1.latitude * Math.PI) / 180) *
      Math.cos((pos2.latitude * Math.PI) / 180) *
      Math.sin(dLon / 2) *
      Math.sin(dLon / 2);
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
  return R * c;
}

// WebSocket server for real-time updates
const wss = new WebSocket.Server({ port: WS_PORT });

wss.on('connection', ws => {
  console.log('WebSocket connection established');
  wsConnections.add(ws);

  // Send welcome message
  ws.send(
    JSON.stringify({
      type: 'welcome',
      message: 'Connected to WRFCoin Weather Oracle',
      timestamp: new Date().toISOString(),
    })
  );

  ws.on('message', data => {
    try {
      const message = JSON.parse(data);

      if (message.type === 'subscribe' && message.stationId) {
        ws.stationId = message.stationId;
        ws.send(
          JSON.stringify({
            type: 'subscribed',
            stationId: message.stationId,
            currentData: stationData.get(message.stationId),
          })
        );
      }
    } catch (error) {
      ws.send(
        JSON.stringify({
          type: 'error',
          message: 'Invalid message format',
        })
      );
    }
  });

  ws.on('close', () => {
    console.log('WebSocket connection closed');
    wsConnections.delete(ws);
  });
});

// Real-time data updates
if (config.enableRealTimeUpdates) {
  setInterval(() => {
    mockStations.forEach(station => {
      if (station.status === 'online') {
        const climateZone = climateZones.find(c => c.zone === station.climate);
        const newData = generateWeatherData(station, climateZone);
        stationData.set(station.id, newData);

        // Add to historical data
        const historical = historicalData.get(station.id) || [];
        historical.push(newData);
        if (historical.length > 1000) {
          historical.shift();
        }
        historicalData.set(station.id, historical);

        // Broadcast to subscribed WebSocket connections
        wsConnections.forEach(ws => {
          if (ws.readyState === WebSocket.OPEN && ws.stationId === station.id) {
            ws.send(
              JSON.stringify({
                type: 'weather_update',
                stationId: station.id,
                data: newData,
              })
            );
          }
        });
      }
    });
  }, config.updateIntervalSec * 1000);
}

// Random extreme weather events
if (config.enableExtremeWeather) {
  setInterval(() => {
    extremeWeatherEvents.forEach(eventType => {
      if (Math.random() < eventType.probability) {
        const availableStations = mockStations.filter(
          s => !activeEvents.has(s.id)
        );
        if (availableStations.length > 0) {
          const station =
            availableStations[
              Math.floor(Math.random() * availableStations.length)
            ];

          // Trigger event via API endpoint logic
          const event = {
            id: uuidv4(),
            type: eventType.type,
            stationId: station.id,
            severity: ['mild', 'moderate', 'severe'][
              Math.floor(Math.random() * 3)
            ],
            startTime: new Date().toISOString(),
            expectedEndTime: moment()
              .add(eventType.duration, 'hours')
              .toISOString(),
            description: `Random ${eventType.type.replace('_', ' ')} event`,
            tempModifier:
              eventType.tempIncrease || -eventType.tempDecrease || 0,
            windModifier: eventType.windIncrease || 0,
            rainModifier:
              eventType.rainIncrease || -eventType.rainDecrease || 0,
          };

          activeEvents.set(station.id, event);

          setTimeout(
            () => {
              activeEvents.delete(station.id);
            },
            eventType.duration * 60 * 60 * 1000
          );

          console.log(
            `Triggered ${eventType.type} event at station ${station.id}`
          );
        }
      }
    });
  }, 60000); // Check every minute
}

// Initialize and start server
initializeStations();

app.listen(PORT, '0.0.0.0', () => {
  console.log(`Enhanced Weather Oracle Mock Server running on port ${PORT}`);
  console.log(`WebSocket server running on port ${WS_PORT}`);
  console.log(`Stations: ${config.stationCount}`);
  console.log(`Update interval: ${config.updateIntervalSec}s`);
  console.log(`Extreme weather: ${config.enableExtremeWeather}`);
  console.log(`Real-time updates: ${config.enableRealTimeUpdates}`);
});

process.on('SIGTERM', () => {
  console.log('Shutting down weather oracle server...');
  wss.close();
  process.exit(0);
});
