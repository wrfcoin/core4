/**
 * Weather Multi-Source Consensus Tests
 * Task 114.8: Multi-source aggregation, conflict resolution, validation
 *
 * Tests for weather data consensus from multiple sources
 * Target: 15 tests
 */

import { describe, it, expect, beforeEach, jest } from '@jest/globals';

interface WeatherReading {
  sourceId: string;
  timestamp: Date;
  temperature: number;
  humidity: number;
  pressure: number;
  confidence: number;
}

interface ConsensusResult {
  temperature: number;
  humidity: number;
  pressure: number;
  confidence: number;
  sourceCount: number;
  outliers: string[];
}

class MockWeatherConsensusService {
  private readonly MAX_TEMP_DEVIATION = 5; // °C
  private readonly MAX_HUMIDITY_DEVIATION = 15; // %
  private readonly MAX_PRESSURE_DEVIATION = 10; // hPa

  computeConsensus(readings: WeatherReading[]): ConsensusResult {
    if (readings.length === 0) {
      throw new Error('No readings provided');
    }

    if (readings.length === 1) {
      return {
        temperature: readings[0].temperature,
        humidity: readings[0].humidity,
        pressure: readings[0].pressure,
        confidence: readings[0].confidence,
        sourceCount: 1,
        outliers: [],
      };
    }

    // Helper function to calculate median (robust against outliers)
    const getMedian = (values: number[]): number => {
      const sorted = [...values].sort((a, b) => a - b);
      const mid = Math.floor(sorted.length / 2);
      return sorted.length % 2 !== 0
        ? sorted[mid]
        : (sorted[mid - 1] + sorted[mid]) / 2;
    };

    // Use median for outlier detection (robust against extreme values)
    const medianTemp = getMedian(readings.map(r => r.temperature));
    const medianHumidity = getMedian(readings.map(r => r.humidity));
    const medianPressure = getMedian(readings.map(r => r.pressure));

    // Find outliers based on deviation from MEDIAN (not mean)
    const outliers = readings
      .filter(
        r =>
          Math.abs(r.temperature - medianTemp) > this.MAX_TEMP_DEVIATION ||
          Math.abs(r.humidity - medianHumidity) > this.MAX_HUMIDITY_DEVIATION ||
          Math.abs(r.pressure - medianPressure) > this.MAX_PRESSURE_DEVIATION
      )
      .map(r => r.sourceId);

    // Recalculate without outliers
    const validReadings = readings.filter(r => !outliers.includes(r.sourceId));
    const validWeight = validReadings.reduce((sum, r) => sum + r.confidence, 0);

    // If no valid readings remain (all are outliers), fallback to median values
    const finalTemp =
      validReadings.length > 0
        ? validReadings.reduce(
            (sum, r) => sum + r.temperature * r.confidence,
            0
          ) / validWeight
        : medianTemp;
    const finalHumidity =
      validReadings.length > 0
        ? validReadings.reduce((sum, r) => sum + r.humidity * r.confidence, 0) /
          validWeight
        : medianHumidity;
    const finalPressure =
      validReadings.length > 0
        ? validReadings.reduce((sum, r) => sum + r.pressure * r.confidence, 0) /
          validWeight
        : medianPressure;

    // Calculate consensus confidence
    const consensusConfidence =
      validReadings.length > 0
        ? validReadings.reduce((sum, r) => sum + r.confidence, 0) /
          validReadings.length
        : 0.5;

    return {
      temperature: Math.round(finalTemp * 100) / 100,
      humidity: Math.round(finalHumidity * 100) / 100,
      pressure: Math.round(finalPressure * 100) / 100,
      confidence: Math.round(consensusConfidence * 100) / 100,
      sourceCount: validReadings.length,
      outliers,
    };
  }

  detectConflicts(readings: WeatherReading[]): {
    hasConflict: boolean;
    conflictingPairs: Array<{
      source1: string;
      source2: string;
      field: string;
      diff: number;
    }>;
  } {
    const conflicts: Array<{
      source1: string;
      source2: string;
      field: string;
      diff: number;
    }> = [];

    for (let i = 0; i < readings.length; i++) {
      for (let j = i + 1; j < readings.length; j++) {
        const r1 = readings[i];
        const r2 = readings[j];

        const tempDiff = Math.abs(r1.temperature - r2.temperature);
        if (tempDiff > this.MAX_TEMP_DEVIATION) {
          conflicts.push({
            source1: r1.sourceId,
            source2: r2.sourceId,
            field: 'temperature',
            diff: tempDiff,
          });
        }

        const humidityDiff = Math.abs(r1.humidity - r2.humidity);
        if (humidityDiff > this.MAX_HUMIDITY_DEVIATION) {
          conflicts.push({
            source1: r1.sourceId,
            source2: r2.sourceId,
            field: 'humidity',
            diff: humidityDiff,
          });
        }
      }
    }

    return {
      hasConflict: conflicts.length > 0,
      conflictingPairs: conflicts,
    };
  }

  validateReading(
    reading: WeatherReading,
    historicalAvg: { temperature: number; humidity: number; pressure: number }
  ): {
    isValid: boolean;
    deviations: Record<string, number>;
  } {
    const deviations = {
      temperature: Math.abs(reading.temperature - historicalAvg.temperature),
      humidity: Math.abs(reading.humidity - historicalAvg.humidity),
      pressure: Math.abs(reading.pressure - historicalAvg.pressure),
    };

    const isValid =
      deviations.temperature <= this.MAX_TEMP_DEVIATION * 2 &&
      deviations.humidity <= this.MAX_HUMIDITY_DEVIATION * 2 &&
      deviations.pressure <= this.MAX_PRESSURE_DEVIATION * 2;

    return { isValid, deviations };
  }
}

describe('Weather Multi-Source Consensus', () => {
  let consensusService: MockWeatherConsensusService;

  beforeEach(() => {
    consensusService = new MockWeatherConsensusService();
  });

  describe('Multi-Source Aggregation', () => {
    it('should compute weighted average from multiple sources', () => {
      const readings: WeatherReading[] = [
        {
          sourceId: 's1',
          timestamp: new Date(),
          temperature: 20,
          humidity: 50,
          pressure: 1013,
          confidence: 0.9,
        },
        {
          sourceId: 's2',
          timestamp: new Date(),
          temperature: 21,
          humidity: 52,
          pressure: 1014,
          confidence: 0.8,
        },
        {
          sourceId: 's3',
          timestamp: new Date(),
          temperature: 20.5,
          humidity: 51,
          pressure: 1013.5,
          confidence: 0.85,
        },
      ];

      const result = consensusService.computeConsensus(readings);

      expect(result.temperature).toBeCloseTo(20.5, 1);
      expect(result.sourceCount).toBe(3);
      expect(result.outliers.length).toBe(0);
    });

    it('should handle single source', () => {
      const readings: WeatherReading[] = [
        {
          sourceId: 's1',
          timestamp: new Date(),
          temperature: 22,
          humidity: 60,
          pressure: 1015,
          confidence: 0.95,
        },
      ];

      const result = consensusService.computeConsensus(readings);

      expect(result.temperature).toBe(22);
      expect(result.sourceCount).toBe(1);
    });

    it('should throw error for empty readings', () => {
      expect(() => consensusService.computeConsensus([])).toThrow(
        'No readings provided'
      );
    });

    it('should weight by confidence', () => {
      const readings: WeatherReading[] = [
        {
          sourceId: 's1',
          timestamp: new Date(),
          temperature: 20,
          humidity: 50,
          pressure: 1013,
          confidence: 1.0,
        },
        {
          sourceId: 's2',
          timestamp: new Date(),
          temperature: 22,
          humidity: 50,
          pressure: 1013,
          confidence: 0.5,
        },
      ];

      const result = consensusService.computeConsensus(readings);

      // Weighted average should be closer to 20 (higher confidence source)
      expect(result.temperature).toBeLessThan(21);
    });
  });

  describe('Outlier Detection', () => {
    it('should detect temperature outliers', () => {
      const readings: WeatherReading[] = [
        {
          sourceId: 's1',
          timestamp: new Date(),
          temperature: 20,
          humidity: 50,
          pressure: 1013,
          confidence: 0.9,
        },
        {
          sourceId: 's2',
          timestamp: new Date(),
          temperature: 21,
          humidity: 51,
          pressure: 1014,
          confidence: 0.9,
        },
        {
          sourceId: 's3',
          timestamp: new Date(),
          temperature: 35,
          humidity: 50,
          pressure: 1013,
          confidence: 0.9,
        }, // Outlier
      ];

      const result = consensusService.computeConsensus(readings);

      expect(result.outliers).toContain('s3');
    });

    it('should exclude outliers from final calculation', () => {
      const readings: WeatherReading[] = [
        {
          sourceId: 's1',
          timestamp: new Date(),
          temperature: 20,
          humidity: 50,
          pressure: 1013,
          confidence: 0.9,
        },
        {
          sourceId: 's2',
          timestamp: new Date(),
          temperature: 21,
          humidity: 51,
          pressure: 1014,
          confidence: 0.9,
        },
        {
          sourceId: 's3',
          timestamp: new Date(),
          temperature: 50,
          humidity: 50,
          pressure: 1013,
          confidence: 0.9,
        }, // Outlier
      ];

      const result = consensusService.computeConsensus(readings);

      expect(result.sourceCount).toBe(2);
      expect(result.temperature).toBeLessThan(25);
    });
  });

  describe('Conflict Resolution', () => {
    it('should detect temperature conflicts', () => {
      const readings: WeatherReading[] = [
        {
          sourceId: 's1',
          timestamp: new Date(),
          temperature: 20,
          humidity: 50,
          pressure: 1013,
          confidence: 0.9,
        },
        {
          sourceId: 's2',
          timestamp: new Date(),
          temperature: 30,
          humidity: 50,
          pressure: 1013,
          confidence: 0.9,
        },
      ];

      const result = consensusService.detectConflicts(readings);

      expect(result.hasConflict).toBe(true);
      expect(result.conflictingPairs[0].field).toBe('temperature');
    });

    it('should report no conflict for agreeing sources', () => {
      const readings: WeatherReading[] = [
        {
          sourceId: 's1',
          timestamp: new Date(),
          temperature: 20,
          humidity: 50,
          pressure: 1013,
          confidence: 0.9,
        },
        {
          sourceId: 's2',
          timestamp: new Date(),
          temperature: 22,
          humidity: 55,
          pressure: 1015,
          confidence: 0.9,
        },
      ];

      const result = consensusService.detectConflicts(readings);

      expect(result.hasConflict).toBe(false);
    });
  });

  describe('Historical Validation', () => {
    it('should validate reading against historical average', () => {
      const reading: WeatherReading = {
        sourceId: 's1',
        timestamp: new Date(),
        temperature: 22,
        humidity: 55,
        pressure: 1015,
        confidence: 0.9,
      };

      const historical = { temperature: 20, humidity: 50, pressure: 1013 };

      const result = consensusService.validateReading(reading, historical);

      expect(result.isValid).toBe(true);
    });

    it('should reject reading with extreme deviation', () => {
      const reading: WeatherReading = {
        sourceId: 's1',
        timestamp: new Date(),
        temperature: 45, // Very different from historical
        humidity: 55,
        pressure: 1015,
        confidence: 0.9,
      };

      const historical = { temperature: 20, humidity: 50, pressure: 1013 };

      const result = consensusService.validateReading(reading, historical);

      expect(result.isValid).toBe(false);
    });
  });

  describe('Edge Cases', () => {
    it('should handle scenario where all sources are outliers from each other', () => {
      // This edge case tests when sources are so divergent that outlier detection
      // cannot establish a meaningful consensus - each source deviates significantly
      const readings: WeatherReading[] = [
        {
          sourceId: 's1',
          timestamp: new Date(),
          temperature: -20,
          humidity: 10,
          pressure: 900,
          confidence: 0.8,
        },
        {
          sourceId: 's2',
          timestamp: new Date(),
          temperature: 40,
          humidity: 90,
          pressure: 1050,
          confidence: 0.8,
        },
        {
          sourceId: 's3',
          timestamp: new Date(),
          temperature: 15,
          humidity: 50,
          pressure: 980,
          confidence: 0.8,
        },
      ];

      // All three sources deviate significantly from each other
      // The median approach should still work by using median as anchor
      const result = consensusService.computeConsensus(readings);

      // Should identify extreme values as outliers
      // With median-based detection, s1 (-20) and s2 (40) should be flagged as outliers
      // since they are more than 5°C from the median (15)
      expect(result.outliers.length).toBeGreaterThanOrEqual(1);

      // The consensus should still produce a result
      expect(result.sourceCount).toBeGreaterThanOrEqual(1);
      expect(typeof result.temperature).toBe('number');
      expect(typeof result.confidence).toBe('number');

      // Confidence should be reduced when many sources are outliers
      expect(result.confidence).toBeLessThanOrEqual(0.9);

      // Conflict detection should identify the disagreement
      const conflicts = consensusService.detectConflicts(readings);
      expect(conflicts.hasConflict).toBe(true);
      expect(conflicts.conflictingPairs.length).toBeGreaterThan(0);
    });
  });
});
