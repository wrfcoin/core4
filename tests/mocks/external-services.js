/**
 * External Services Mocks
 * Mock implementations for weather APIs, database connections, and other external services
 */

// Weather API mocks
const weatherAPIMocks = {
  acuRite: {
    getDeviceData: jest.fn().mockResolvedValue({
      deviceId: 'acurite-001',
      readings: [
        {
          temperature: 22.5,
          humidity: 65,
          pressure: 1013.25,
          timestamp: Date.now(),
        },
      ],
    }),
  },
  openWeatherMap: {
    getCurrentWeather: jest.fn().mockResolvedValue({
      main: {
        temp: 22.5,
        humidity: 65,
        pressure: 1013,
      },
      wind: { speed: 5.2, deg: 180 },
    }),
  },
  weatherFlow: {
    getStationData: jest.fn().mockResolvedValue({
      stationId: 'wf-station-001',
      observations: [
        {
          timestamp: Date.now(),
          airTemperature: 22.5,
          humidity: 65,
          pressure: 1013.25,
          windSpeed: 5.2,
        },
      ],
    }),
  },
};

// Database mocks
const databaseMocks = {
  query: jest.fn().mockResolvedValue({ rows: [], rowCount: 0 }),
  connect: jest.fn().mockResolvedValue({ connected: true }),
  disconnect: jest.fn().mockResolvedValue(true),
};

// Reset all mocks helper
const resetAllMocks = () => {
  Object.values(weatherAPIMocks).forEach(api => {
    Object.values(api).forEach(fn => {
      if (typeof fn === 'function' && fn.mockClear) {
        fn.mockClear();
      }
    });
  });
  Object.values(databaseMocks).forEach(fn => {
    if (typeof fn === 'function' && fn.mockClear) {
      fn.mockClear();
    }
  });
};

module.exports = {
  weatherAPIMocks,
  databaseMocks,
  resetAllMocks,
};
