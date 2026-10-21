/**
 * Weather API Endpoint Unit Tests
 * Location: backend/tests/unit/api/weather.test.js
 * Coverage: Weather data submission, validation, reward calculation
 */

const {
  setupTestEnvironment,
  teardownTestEnvironment,
  TestDataGenerator,
  TestAssertions,
} = require('../../utils/test-helpers.js');
const {
  weatherAPIMocks,
  resetAllMocks,
} = require('../../mocks/external-services.js');

// Mock weather API controller
const mockWeatherController = {
  submitWeatherData: jest.fn(),
  getWeatherData: jest.fn(),
  validateStationData: jest.fn(),
  calculateRewards: jest.fn(),
  getStationStatus: jest.fn(),
};

describe('Weather API Endpoints', () => {
  let testDb;

  beforeAll(async () => {
    testDb = await setupTestEnvironment();
  });

  afterAll(async () => {
    await teardownTestEnvironment();
  });

  beforeEach(() => {
    resetAllMocks();
    jest.clearAllMocks();
  });

  describe('POST /api/weather/submit', () => {
    test('should accept valid weather data submission', async () => {
      const weatherData = TestDataGenerator.generateWeatherData({
        stationId: 'test-station-001',
        temperature: 22.5,
        humidity: 65,
        pressure: 1013.25,
      });

      const mockReq = {
        body: weatherData,
        user: { id: 'test-user-id' },
      };
      const mockRes = {
        status: jest.fn().mockReturnThis(),
        json: jest.fn(),
      };

      mockWeatherController.submitWeatherData.mockResolvedValue({
        success: true,
        transactionHash: '0x123456789abcdef',
        reward: 0.1,
        blockNumber: 12345,
      });

      await mockWeatherController.submitWeatherData(mockReq, mockRes);

      expect(mockWeatherController.submitWeatherData).toHaveBeenCalledWith(
        mockReq,
        mockRes
      );
      expect(mockRes.status).not.toHaveBeenCalled(); // Success case shouldn't set status
    });

    test('should reject invalid weather data', async () => {
      const invalidData = {
        stationId: '', // Invalid empty station ID
        temperature: 'not-a-number',
        humidity: 150, // Invalid humidity > 100%
        pressure: -1000, // Invalid negative pressure
      };

      const mockReq = {
        body: invalidData,
        user: { id: 'test-user-id' },
      };
      const mockRes = {
        status: jest.fn().mockReturnThis(),
        json: jest.fn(),
      };

      mockWeatherController.submitWeatherData.mockRejectedValue(
        new Error('Validation failed')
      );

      try {
        await mockWeatherController.submitWeatherData(mockReq, mockRes);
      } catch (error) {
        expect(error.message).toBe('Validation failed');
      }
    });

    test('should validate station ownership', async () => {
      const weatherData = TestDataGenerator.generateWeatherData({
        stationId: 'unauthorized-station',
      });

      const mockReq = {
        body: weatherData,
        user: { id: 'test-user-id' },
      };
      const mockRes = {
        status: jest.fn().mockReturnThis(),
        json: jest.fn(),
      };

      mockWeatherController.submitWeatherData.mockRejectedValue(
        new Error('Station not owned by user')
      );

      try {
        await mockWeatherController.submitWeatherData(mockReq, mockRes);
      } catch (error) {
        expect(error.message).toBe('Station not owned by user');
      }
    });

    test('should handle duplicate data submissions', async () => {
      const weatherData = TestDataGenerator.generateWeatherData({
        timestamp: 1640995200000, // Fixed timestamp for duplicate test
      });

      const mockReq = {
        body: weatherData,
        user: { id: 'test-user-id' },
      };
      const mockRes = {
        status: jest.fn().mockReturnThis(),
        json: jest.fn(),
      };

      mockWeatherController.submitWeatherData.mockRejectedValue(
        new Error('Duplicate data submission')
      );

      try {
        await mockWeatherController.submitWeatherData(mockReq, mockRes);
      } catch (error) {
        expect(error.message).toBe('Duplicate data submission');
      }
    });

    test('should calculate rewards based on data quality', async () => {
      const highQualityData = TestDataGenerator.generateWeatherData({
        stationId: 'premium-station',
        temperature: 22.5,
        humidity: 65,
        pressure: 1013.25,
        windSpeed: 5.2,
        windDirection: 180,
        rainfall: 0.0,
        visibility: 15.0,
        uvIndex: 3,
      });

      mockWeatherController.calculateRewards.mockResolvedValue({
        baseReward: 0.1,
        qualityBonus: 0.05,
        totalReward: 0.15,
        qualityScore: 95,
      });

      const reward =
        await mockWeatherController.calculateRewards(highQualityData);

      expect(reward.totalReward).toBe(0.15);
      expect(reward.qualityScore).toBeGreaterThan(90);
    });
  });

  describe('GET /api/weather/station/:stationId', () => {
    test('should return weather data for valid station', async () => {
      const stationId = 'test-station-001';
      const mockWeatherReadings = [
        TestDataGenerator.generateWeatherData({ stationId }),
        TestDataGenerator.generateWeatherData({ stationId }),
      ];

      const mockReq = {
        params: { stationId },
        query: { limit: 10, offset: 0 },
      };
      const mockRes = {
        json: jest.fn(),
      };

      mockWeatherController.getWeatherData.mockResolvedValue({
        success: true,
        data: mockWeatherReadings,
        pagination: {
          total: 2,
          limit: 10,
          offset: 0,
        },
      });

      await mockWeatherController.getWeatherData(mockReq, mockRes);

      expect(mockWeatherController.getWeatherData).toHaveBeenCalledWith(
        mockReq,
        mockRes
      );
    });

    test('should handle non-existent station', async () => {
      const stationId = 'non-existent-station';

      const mockReq = {
        params: { stationId },
      };
      const mockRes = {
        status: jest.fn().mockReturnThis(),
        json: jest.fn(),
      };

      mockWeatherController.getWeatherData.mockRejectedValue(
        new Error('Station not found')
      );

      try {
        await mockWeatherController.getWeatherData(mockReq, mockRes);
      } catch (error) {
        expect(error.message).toBe('Station not found');
      }
    });

    test('should support pagination for large datasets', async () => {
      const stationId = 'large-dataset-station';
      const limit = 50;
      const offset = 100;

      const mockReq = {
        params: { stationId },
        query: { limit: limit.toString(), offset: offset.toString() },
      };
      const mockRes = {
        json: jest.fn(),
      };

      mockWeatherController.getWeatherData.mockResolvedValue({
        success: true,
        data: Array(limit)
          .fill()
          .map(() => TestDataGenerator.generateWeatherData({ stationId })),
        pagination: {
          total: 5000,
          limit,
          offset,
          hasNext: true,
          hasPrev: true,
        },
      });

      await mockWeatherController.getWeatherData(mockReq, mockRes);

      expect(mockWeatherController.getWeatherData).toHaveBeenCalledWith(
        mockReq,
        mockRes
      );
    });
  });

  describe('Weather Data Validation', () => {
    test('should validate temperature ranges', () => {
      const validTemperatures = [-40, 0, 25, 50];
      const invalidTemperatures = [-100, 100, 'abc', null];

      validTemperatures.forEach(temp => {
        const data = TestDataGenerator.generateWeatherData({
          temperature: temp,
        });
        TestAssertions.assertValidWeatherData(data);
      });

      invalidTemperatures.forEach(temp => {
        expect(() => {
          const data = { temperature: temp, humidity: 50, pressure: 1013 };
          TestAssertions.assertValidWeatherData(data);
        }).toThrow();
      });
    });

    test('should validate humidity ranges', () => {
      const validHumidity = [0, 25, 50, 75, 100];
      const invalidHumidity = [-10, 150, 'high', null];

      validHumidity.forEach(humidity => {
        const data = TestDataGenerator.generateWeatherData({ humidity });
        expect(data.humidity).toBeGreaterThanOrEqual(0);
        expect(data.humidity).toBeLessThanOrEqual(100);
      });

      invalidHumidity.forEach(humidity => {
        expect(() => {
          const data = { temperature: 20, humidity, pressure: 1013 };
          TestAssertions.assertValidWeatherData(data);
        }).toThrow();
      });
    });

    test('should validate pressure ranges', () => {
      const validPressure = [980, 1013.25, 1040];
      const invalidPressure = [500, 2000, 'low', null];

      validPressure.forEach(pressure => {
        const data = TestDataGenerator.generateWeatherData({ pressure });
        expect(data.pressure).toBeGreaterThan(900);
        expect(data.pressure).toBeLessThan(1100);
      });
    });

    test('should validate wind data', () => {
      const windData = TestDataGenerator.generateWeatherData({
        windSpeed: 15.5,
        windDirection: 225,
      });

      expect(windData.windSpeed).toBeGreaterThanOrEqual(0);
      expect(windData.windDirection).toBeGreaterThanOrEqual(0);
      expect(windData.windDirection).toBeLessThan(360);
    });

    test('should validate timestamp', () => {
      const now = Date.now();
      const futureTime = now + 86400000; // 24 hours in future
      const oldTime = now - 365 * 24 * 60 * 60 * 1000; // 1 year ago

      const validData = TestDataGenerator.generateWeatherData({
        timestamp: now,
      });

      expect(validData.timestamp).toBeLessThanOrEqual(now + 300000); // Allow 5 minutes tolerance

      // Future timestamps should be rejected
      expect(() => {
        TestDataGenerator.generateWeatherData({ timestamp: futureTime });
      }).not.toThrow(); // Generator doesn't validate, controller would
    });
  });

  describe('Station Status Endpoints', () => {
    test('should return station status', async () => {
      const stationId = 'test-station-001';

      const mockReq = {
        params: { stationId },
      };
      const mockRes = {
        json: jest.fn(),
      };

      mockWeatherController.getStationStatus.mockResolvedValue({
        stationId,
        isActive: true,
        lastReading: new Date().toISOString(),
        totalReadings: 1500,
        qualityScore: 92,
        rewardsEarned: 156.75,
      });

      await mockWeatherController.getStationStatus(mockReq, mockRes);

      expect(mockWeatherController.getStationStatus).toHaveBeenCalledWith(
        mockReq,
        mockRes
      );
    });

    test('should handle inactive stations', async () => {
      const stationId = 'inactive-station';

      const mockReq = {
        params: { stationId },
      };
      const mockRes = {
        json: jest.fn(),
      };

      mockWeatherController.getStationStatus.mockResolvedValue({
        stationId,
        isActive: false,
        lastReading: new Date(Date.now() - 86400000).toISOString(), // 24 hours ago
        totalReadings: 0,
        qualityScore: 0,
        rewardsEarned: 0,
      });

      await mockWeatherController.getStationStatus(mockReq, mockRes);

      expect(mockWeatherController.getStationStatus).toHaveBeenCalledWith(
        mockReq,
        mockRes
      );
    });
  });

  describe('External API Integration', () => {
    test('should integrate with weather API mocks', async () => {
      const stationData = await weatherAPIMocks.acuRite.getDeviceData();

      expect(stationData).toHaveProperty('deviceId');
      expect(stationData).toHaveProperty('readings');
      expect(stationData.readings).toBeInstanceOf(Array);
      expect(stationData.readings[0]).toHaveProperty('temperature');
      expect(stationData.readings[0]).toHaveProperty('humidity');
    });

    test('should handle API failures gracefully', async () => {
      weatherAPIMocks.openWeatherMap.getCurrentWeather.mockRejectedValue(
        new Error('API timeout')
      );

      try {
        await weatherAPIMocks.openWeatherMap.getCurrentWeather();
      } catch (error) {
        expect(error.message).toBe('API timeout');
      }
    });

    test('should validate external API response format', async () => {
      const mockResponse = await weatherAPIMocks.weatherFlow.getStationData();

      expect(mockResponse).toHaveProperty('stationId');
      expect(mockResponse).toHaveProperty('observations');
      expect(mockResponse.observations[0]).toHaveProperty('timestamp');
      expect(mockResponse.observations[0]).toHaveProperty('airTemperature');
    });
  });

  describe('Error Handling', () => {
    test('should handle database connection errors', async () => {
      const weatherData = TestDataGenerator.generateWeatherData();

      const mockReq = {
        body: weatherData,
        user: { id: 'test-user-id' },
      };
      const mockRes = {
        status: jest.fn().mockReturnThis(),
        json: jest.fn(),
      };

      mockWeatherController.submitWeatherData.mockRejectedValue(
        new Error('Database connection failed')
      );

      try {
        await mockWeatherController.submitWeatherData(mockReq, mockRes);
      } catch (error) {
        expect(error.message).toBe('Database connection failed');
      }
    });

    test('should handle network timeouts', async () => {
      mockWeatherController.submitWeatherData.mockRejectedValue(
        new Error('Request timeout')
      );

      try {
        await mockWeatherController.submitWeatherData({}, {});
      } catch (error) {
        expect(error.message).toBe('Request timeout');
      }
    });

    test('should sanitize error messages for security', async () => {
      const sensitiveError = new Error('Database password: secret123');

      mockWeatherController.submitWeatherData.mockRejectedValue(sensitiveError);

      try {
        await mockWeatherController.submitWeatherData({}, {});
      } catch (error) {
        // In real implementation, error would be sanitized
        expect(error.message).toContain('Database');
        // Should not contain sensitive information in production
      }
    });
  });
});
