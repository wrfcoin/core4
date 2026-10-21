import { faker } from '@faker-js/faker';

export interface WeatherDataPoint {
  temperature: number;
  humidity: number;
  pressure: number;
  windSpeed: number;
  windDirection?: number;
  rainfall?: number;
  uvIndex?: number;
  visibility?: number;
  timestamp: string;
  location: {
    lat: number;
    lon: number;
    name: string;
    elevation?: number;
  };
  quality: {
    accuracy: number;
    completeness: number;
    timeliness: number;
  };
}

export interface WeatherDataSeries {
  deviceId: string;
  startTime: Date;
  endTime: Date;
  interval: number; // seconds
  dataPoints: WeatherDataPoint[];
  statistics: {
    averageTemperature: number;
    maxTemperature: number;
    minTemperature: number;
    averageHumidity: number;
    totalRainfall: number;
  };
}

export interface BatchWeatherData {
  headers: string[];
  rows: string[][];
  metadata: {
    recordCount: number;
    timeRange: {
      start: string;
      end: string;
    };
    location: {
      lat: number;
      lon: number;
      name: string;
    };
    dataTypes: string[];
  };
}

export class WeatherDataGenerator {
  private readonly REALISTIC_RANGES = {
    temperature: { min: -40, max: 50 }, // Celsius
    humidity: { min: 0, max: 100 }, // Percentage
    pressure: { min: 950, max: 1050 }, // hPa
    windSpeed: { min: 0, max: 200 }, // km/h
    windDirection: { min: 0, max: 360 }, // degrees
    rainfall: { min: 0, max: 100 }, // mm/hour
    uvIndex: { min: 0, max: 11 },
    visibility: { min: 0, max: 50 }, // km
  };

  private readonly SEASONAL_ADJUSTMENTS = {
    winter: { tempOffset: -10, humidityOffset: -5 },
    spring: { tempOffset: 5, humidityOffset: 10 },
    summer: { tempOffset: 15, humidityOffset: 15 },
    fall: { tempOffset: 0, humidityOffset: 5 },
  };

  private readonly REGIONAL_CHARACTERISTICS = {
    tropical: { baseTemp: 28, baseHumidity: 80, tempVariance: 5 },
    temperate: { baseTemp: 15, baseHumidity: 60, tempVariance: 15 },
    arctic: { baseTemp: -15, baseHumidity: 40, tempVariance: 20 },
    desert: { baseTemp: 25, baseHumidity: 20, tempVariance: 20 },
    coastal: { baseTemp: 18, baseHumidity: 70, tempVariance: 10 },
  };

  /**
   * Generate a single realistic weather data point
   */
  generateWeatherDataPoint(
    options: {
      location?: { lat: number; lon: number; name: string; elevation?: number };
      timestamp?: Date;
      regionalType?: keyof typeof this.REGIONAL_CHARACTERISTICS;
      season?: keyof typeof this.SEASONAL_ADJUSTMENTS;
      qualityLevel?: 'high' | 'medium' | 'low';
      includeOptionalFields?: boolean;
    } = {}
  ): WeatherDataPoint {
    const {
      location = this.generateRandomLocation(),
      timestamp = new Date(),
      regionalType = this.determineRegionalType(location.lat),
      season = this.determineSeason(timestamp),
      qualityLevel = 'high',
      includeOptionalFields = false,
    } = options;

    const regional = this.REGIONAL_CHARACTERISTICS[regionalType];
    const seasonal = this.SEASONAL_ADJUSTMENTS[season];

    // Generate base temperature with regional and seasonal adjustments
    const baseTemperature = regional.baseTemp + seasonal.tempOffset;
    const temperature = this.clampValue(
      baseTemperature +
        faker.number.float({
          min: -regional.tempVariance / 2,
          max: regional.tempVariance / 2,
        }),
      this.REALISTIC_RANGES.temperature
    );

    // Generate humidity with regional adjustments
    const baseHumidity = regional.baseHumidity + seasonal.humidityOffset;
    const humidity = this.clampValue(
      baseHumidity + faker.number.float({ min: -20, max: 20 }),
      this.REALISTIC_RANGES.humidity
    );

    // Generate pressure (influenced by elevation)
    const elevationOffset = (location.elevation || 0) * -0.12; // Pressure decreases with altitude
    const pressure = this.clampValue(
      1013.25 + elevationOffset + faker.number.float({ min: -30, max: 30 }),
      this.REALISTIC_RANGES.pressure
    );

    // Generate wind speed (higher for coastal/open areas)
    const windSpeedBase = regionalType === 'coastal' ? 15 : 8;
    const windSpeed = this.clampValue(
      windSpeedBase + faker.number.float({ min: -5, max: 20 }),
      this.REALISTIC_RANGES.windSpeed
    );

    // Generate quality metrics based on quality level
    const quality = this.generateQualityMetrics(qualityLevel);

    const dataPoint: WeatherDataPoint = {
      temperature: this.roundToDecimalPlaces(temperature, 1),
      humidity: this.roundToDecimalPlaces(humidity, 0),
      pressure: this.roundToDecimalPlaces(pressure, 2),
      windSpeed: this.roundToDecimalPlaces(windSpeed, 1),
      timestamp: timestamp.toISOString(),
      location,
      quality,
    };

    // Add optional fields if requested
    if (includeOptionalFields) {
      dataPoint.windDirection = faker.number.int(
        this.REALISTIC_RANGES.windDirection
      );
      dataPoint.rainfall = this.roundToDecimalPlaces(
        faker.number.float(this.REALISTIC_RANGES.rainfall),
        1
      );
      dataPoint.uvIndex = faker.number.int(this.REALISTIC_RANGES.uvIndex);
      dataPoint.visibility = this.roundToDecimalPlaces(
        faker.number.float(this.REALISTIC_RANGES.visibility),
        1
      );
    }

    return dataPoint;
  }

  /**
   * Generate a time series of weather data
   */
  generateWeatherDataSeries(options: {
    deviceId: string;
    duration: number; // hours
    interval: number; // seconds between readings
    location?: { lat: number; lon: number; name: string; elevation?: number };
    startTime?: Date;
    regionalType?: keyof typeof this.REGIONAL_CHARACTERISTICS;
    qualityLevel?: 'high' | 'medium' | 'low';
    includeOptionalFields?: boolean;
    addTrends?: boolean; // Add realistic trends over time
    addAnomalies?: boolean; // Add occasional anomalous readings
  }): WeatherDataSeries {
    const {
      deviceId,
      duration,
      interval,
      location = this.generateRandomLocation(),
      startTime = new Date(),
      regionalType = this.determineRegionalType(location.lat),
      qualityLevel = 'high',
      includeOptionalFields = false,
      addTrends = true,
      addAnomalies = false,
    } = options;

    const dataPoints: WeatherDataPoint[] = [];
    const totalPoints = Math.floor((duration * 3600) / interval);
    const endTime = new Date(startTime.getTime() + duration * 3600 * 1000);

    // Initialize trend factors for realistic changes over time
    let temperatureTrend = 0;
    let humidityTrend = 0;
    let pressureTrend = 0;

    for (let i = 0; i < totalPoints; i++) {
      const pointTime = new Date(startTime.getTime() + i * interval * 1000);
      const season = this.determineSeason(pointTime);

      // Apply gradual trends
      if (addTrends && i > 0) {
        temperatureTrend += faker.number.float({ min: -0.1, max: 0.1 });
        humidityTrend += faker.number.float({ min: -0.2, max: 0.2 });
        pressureTrend += faker.number.float({ min: -0.05, max: 0.05 });
      }

      const baseDataPoint = this.generateWeatherDataPoint({
        location,
        timestamp: pointTime,
        regionalType,
        season,
        qualityLevel:
          addAnomalies && Math.random() < 0.05 ? 'low' : qualityLevel, // 5% anomalies
        includeOptionalFields,
      });

      // Apply trends
      if (addTrends) {
        baseDataPoint.temperature += temperatureTrend;
        baseDataPoint.humidity = Math.max(
          0,
          Math.min(100, baseDataPoint.humidity + humidityTrend)
        );
        baseDataPoint.pressure += pressureTrend;
      }

      // Add occasional missing data points based on quality level
      const missChance =
        qualityLevel === 'high'
          ? 0.001
          : qualityLevel === 'medium'
            ? 0.01
            : 0.05;
      if (Math.random() > missChance) {
        dataPoints.push(baseDataPoint);
      }
    }

    // Calculate statistics
    const temperatures = dataPoints.map(p => p.temperature);
    const humidities = dataPoints.map(p => p.humidity);
    const rainfalls = dataPoints.map(p => p.rainfall || 0);

    const statistics = {
      averageTemperature: this.roundToDecimalPlaces(
        temperatures.reduce((a, b) => a + b, 0) / temperatures.length,
        1
      ),
      maxTemperature: Math.max(...temperatures),
      minTemperature: Math.min(...temperatures),
      averageHumidity: this.roundToDecimalPlaces(
        humidities.reduce((a, b) => a + b, 0) / humidities.length,
        1
      ),
      totalRainfall: this.roundToDecimalPlaces(
        rainfalls.reduce((a, b) => a + b, 0),
        1
      ),
    };

    return {
      deviceId,
      startTime,
      endTime,
      interval,
      dataPoints,
      statistics,
    };
  }

  /**
   * Generate batch weather data in CSV format
   */
  generateBatchWeatherData(options: {
    recordCount: number;
    timeRange: { start: Date; end: Date };
    location?: { lat: number; lon: number; name: string };
    regionalType?: keyof typeof this.REGIONAL_CHARACTERISTICS;
    qualityLevel?: 'high' | 'medium' | 'low';
    includeOptionalFields?: boolean;
    format?: 'csv' | 'json';
  }): BatchWeatherData {
    const {
      recordCount,
      timeRange,
      location = this.generateRandomLocation(),
      regionalType = this.determineRegionalType(location.lat),
      qualityLevel = 'high',
      includeOptionalFields = false,
      format = 'csv',
    } = options;

    const headers = [
      'timestamp',
      'temperature',
      'humidity',
      'pressure',
      'wind_speed',
      'latitude',
      'longitude',
      'location',
    ];

    if (includeOptionalFields) {
      headers.push('wind_direction', 'rainfall', 'uv_index', 'visibility');
    }

    const rows: string[][] = [];
    const timeDiff = timeRange.end.getTime() - timeRange.start.getTime();
    const timeStep = timeDiff / recordCount;

    for (let i = 0; i < recordCount; i++) {
      const timestamp = new Date(timeRange.start.getTime() + i * timeStep);
      const dataPoint = this.generateWeatherDataPoint({
        location,
        timestamp,
        regionalType,
        qualityLevel,
        includeOptionalFields,
      });

      const row = [
        dataPoint.timestamp,
        dataPoint.temperature.toString(),
        dataPoint.humidity.toString(),
        dataPoint.pressure.toString(),
        dataPoint.windSpeed.toString(),
        dataPoint.location.lat.toString(),
        dataPoint.location.lon.toString(),
        dataPoint.location.name,
      ];

      if (includeOptionalFields) {
        row.push(
          (dataPoint.windDirection || '').toString(),
          (dataPoint.rainfall || '').toString(),
          (dataPoint.uvIndex || '').toString(),
          (dataPoint.visibility || '').toString()
        );
      }

      rows.push(row);
    }

    return {
      headers,
      rows,
      metadata: {
        recordCount,
        timeRange: {
          start: timeRange.start.toISOString(),
          end: timeRange.end.toISOString(),
        },
        location,
        dataTypes: headers.slice(1, includeOptionalFields ? -4 : -3), // Exclude timestamp and location fields
      },
    };
  }

  /**
   * Generate realistic IoT device data submission payload
   */
  generateIoTSubmissionPayload(
    deviceId: string,
    options: {
      qualityLevel?: 'high' | 'medium' | 'low';
      location?: { lat: number; lon: number; name: string };
      includeOptionalFields?: boolean;
    } = {}
  ): any {
    const dataPoint = this.generateWeatherDataPoint(options);

    return {
      deviceId,
      timestamp: dataPoint.timestamp,
      data: {
        temperature: dataPoint.temperature,
        humidity: dataPoint.humidity,
        pressure: dataPoint.pressure,
        windSpeed: dataPoint.windSpeed,
        ...(options.includeOptionalFields && {
          windDirection: dataPoint.windDirection,
          rainfall: dataPoint.rainfall,
          uvIndex: dataPoint.uvIndex,
          visibility: dataPoint.visibility,
        }),
      },
      location: dataPoint.location,
      quality: dataPoint.quality,
      deviceMetadata: {
        firmwareVersion: '1.2.3',
        batteryLevel: faker.number.int({ min: 20, max: 100 }),
        signalStrength: faker.number.int({ min: -80, max: -30 }),
        lastMaintenance: faker.date.recent({ days: 30 }).toISOString(),
      },
    };
  }

  /**
   * Generate weather data with intentional validation errors
   */
  generateInvalidWeatherData(
    errorType: 'range' | 'format' | 'missing' | 'coordinates'
  ): Partial<WeatherDataPoint> {
    const baseData = this.generateWeatherDataPoint();

    switch (errorType) {
      case 'range':
        return {
          ...baseData,
          temperature: 200, // Unrealistic temperature
          humidity: 150, // Invalid humidity > 100%
          pressure: 500, // Unrealistic pressure
        };

      case 'format':
        return {
          ...baseData,
          temperature: 'invalid' as any,
          humidity: null as any,
          timestamp: 'not-a-date',
        };

      case 'missing':
        const { temperature, humidity, ...incomplete } = baseData;
        return incomplete;

      case 'coordinates':
        return {
          ...baseData,
          location: {
            lat: 95, // Invalid latitude > 90
            lon: 200, // Invalid longitude > 180
            name: baseData.location.name,
          },
        };

      default:
        return baseData;
    }
  }

  /**
   * Convert batch data to CSV string
   */
  batchDataToCsv(batchData: BatchWeatherData): string {
    const csvLines = [
      batchData.headers.join(','),
      ...batchData.rows.map(row => row.join(',')),
    ];
    return csvLines.join('\n');
  }

  // Private helper methods

  private generateRandomLocation(): {
    lat: number;
    lon: number;
    name: string;
    elevation?: number;
  } {
    const locations = [
      { lat: 40.7128, lon: -74.006, name: 'New York, NY', elevation: 10 },
      { lat: 34.0522, lon: -118.2437, name: 'Los Angeles, CA', elevation: 71 },
      { lat: 41.8781, lon: -87.6298, name: 'Chicago, IL', elevation: 182 },
      { lat: 29.7604, lon: -95.3698, name: 'Houston, TX', elevation: 13 },
      { lat: 39.9526, lon: -75.1652, name: 'Philadelphia, PA', elevation: 12 },
      { lat: 33.4484, lon: -112.074, name: 'Phoenix, AZ', elevation: 331 },
      {
        lat: 37.7749,
        lon: -122.4194,
        name: 'San Francisco, CA',
        elevation: 16,
      },
      { lat: 71.0489, lon: -156.7889, name: 'Utqiagvik, AK', elevation: 5 },
      { lat: 25.7617, lon: -80.1918, name: 'Miami, FL', elevation: 2 },
      { lat: 47.6062, lon: -122.3321, name: 'Seattle, WA', elevation: 56 },
    ];

    return faker.helpers.arrayElement(locations);
  }

  private determineRegionalType(
    latitude: number
  ): keyof typeof this.REGIONAL_CHARACTERISTICS {
    const absLat = Math.abs(latitude);
    if (absLat > 60) return 'arctic';
    if (absLat < 23.5) return 'tropical';
    if (absLat < 35) return 'desert';
    return 'temperate';
  }

  private determineSeason(date: Date): keyof typeof this.SEASONAL_ADJUSTMENTS {
    const month = date.getMonth() + 1; // JavaScript months are 0-indexed
    if (month >= 3 && month <= 5) return 'spring';
    if (month >= 6 && month <= 8) return 'summer';
    if (month >= 9 && month <= 11) return 'fall';
    return 'winter';
  }

  private generateQualityMetrics(level: 'high' | 'medium' | 'low'): {
    accuracy: number;
    completeness: number;
    timeliness: number;
  } {
    const ranges = {
      high: { acc: [0.95, 1.0], comp: [0.98, 1.0], time: [0.98, 1.0] },
      medium: { acc: [0.85, 0.95], comp: [0.9, 0.98], time: [0.9, 0.98] },
      low: { acc: [0.7, 0.85], comp: [0.75, 0.9], time: [0.75, 0.9] },
    };

    const range = ranges[level];
    return {
      accuracy: this.roundToDecimalPlaces(
        faker.number.float({ min: range.acc[0], max: range.acc[1] }),
        3
      ),
      completeness: this.roundToDecimalPlaces(
        faker.number.float({ min: range.comp[0], max: range.comp[1] }),
        3
      ),
      timeliness: this.roundToDecimalPlaces(
        faker.number.float({ min: range.time[0], max: range.time[1] }),
        3
      ),
    };
  }

  private clampValue(
    value: number,
    range: { min: number; max: number }
  ): number {
    return Math.max(range.min, Math.min(range.max, value));
  }

  private roundToDecimalPlaces(value: number, decimals: number): number {
    return Math.round(value * Math.pow(10, decimals)) / Math.pow(10, decimals);
  }
}
