/**
 * Weather Service for Wrfcoin SDK
 *
 * Handles weather station management, data submission,
 * rewards tracking, and quality validation.
 */

import { GraphQLClient } from '../client/GraphQLClient.js';
import {
  WeatherStation,
  WeatherData,
  WeatherDataInput,
  WeatherDataSubmissionResult,
  WeatherStationFilter,
  QueryOptions,
  QueryResponse,
  Address,
  DateTime,
} from '../types/index.js';
import {
  WeatherError,
  ValidationError,
  BlockchainError,
} from '../errors/index.js';

// GraphQL queries
const GET_WEATHER_STATION_QUERY = `
  query GetWeatherStation($id: ID!) {
    weatherStation(id: $id) {
      id
      address
      stationId
      location
      isActive
      lastRewardTime
      totalRewards
      qualityScore
      latestWeatherData {
        id
        timestamp
        temperature
        humidity
        pressure
        windSpeed
        rainfall
        confidence
      }
    }
  }
`;

const GET_WEATHER_STATIONS_QUERY = `
  query GetWeatherStations(
    $first: Int,
    $skip: Int,
    $orderBy: String,
    $orderDirection: String,
    $where: WeatherStationFilter
  ) {
    weatherStations(
      first: $first,
      skip: $skip,
      orderBy: $orderBy,
      orderDirection: $orderDirection,
      where: $where
    ) {
      id
      address
      stationId
      location
      isActive
      lastRewardTime
      totalRewards
      qualityScore
      latestWeatherData {
        id
        timestamp
        temperature
        humidity
        pressure
        windSpeed
        rainfall
        confidence
      }
    }
  }
`;

const GET_WEATHER_DATA_QUERY = `
  query GetWeatherData(
    $stationId: String!,
    $from: DateTime,
    $to: DateTime,
    $first: Int
  ) {
    weatherData(
      stationId: $stationId,
      from: $from,
      to: $to,
      first: $first
    ) {
      id
      stationId
      timestamp
      temperature
      humidity
      pressure
      windSpeed
      rainfall
      confidence
      station {
        id
        stationId
        location
        qualityScore
      }
    }
  }
`;

const SUBMIT_WEATHER_DATA_MUTATION = `
  mutation SubmitWeatherData($input: WeatherDataInput!) {
    submitWeatherData(input: $input) {
      success
      weatherData {
        id
        timestamp
        temperature
        humidity
        pressure
        windSpeed
        rainfall
        confidence
      }
      rewards
      error
    }
  }
`;

// Subscriptions for real-time data
const WEATHER_DATA_UPDATED_SUBSCRIPTION = `
  subscription WeatherDataUpdated($stationId: String) {
    weatherDataUpdated(stationId: $stationId) {
      id
      stationId
      timestamp
      temperature
      humidity
      pressure
      windSpeed
      rainfall
      confidence
    }
  }
`;

export class WeatherService {
  constructor(private graphql: GraphQLClient) {}

  /**
   * Get weather station by ID
   */
  async getStation(id: string): Promise<WeatherStation | null> {
    try {
      if (!id) {
        throw new ValidationError('Station ID is required');
      }

      const response = await this.graphql.query(GET_WEATHER_STATION_QUERY, {
        id,
      });
      return response.weatherStation || null;
    } catch (error) {
      if (error instanceof ValidationError) {
        throw error;
      }
      throw new WeatherError(
        `Failed to get weather station: ${error instanceof Error ? error.message : 'Unknown error'}`,
        id
      );
    }
  }

  /**
   * Get weather stations with filtering and pagination
   */
  async getStations(
    filter: WeatherStationFilter = {},
    options: QueryOptions = {}
  ): Promise<QueryResponse<WeatherStation>> {
    try {
      const {
        first = 20,
        skip = 0,
        orderBy = 'qualityScore',
        orderDirection = 'desc',
      } = options;

      const response = await this.graphql.query(GET_WEATHER_STATIONS_QUERY, {
        first,
        skip,
        orderBy,
        orderDirection,
        where: filter,
      });

      return {
        data: response.weatherStations || [],
        totalCount: response.weatherStations?.length || 0,
      };
    } catch (error) {
      throw new WeatherError(
        `Failed to get weather stations: ${error instanceof Error ? error.message : 'Unknown error'}`
      );
    }
  }

  /**
   * Get active weather stations only
   */
  async getActiveStations(
    options: QueryOptions = {}
  ): Promise<QueryResponse<WeatherStation>> {
    return this.getStations({ isActive: true }, options);
  }

  /**
   * Get high quality stations (quality score >= threshold)
   */
  async getHighQualityStations(
    qualityThreshold: number = 80,
    options: QueryOptions = {}
  ): Promise<QueryResponse<WeatherStation>> {
    return this.getStations({ qualityScore_gte: qualityThreshold }, options);
  }

  /**
   * Search stations by location
   */
  async searchStationsByLocation(
    location: string,
    options: QueryOptions = {}
  ): Promise<QueryResponse<WeatherStation>> {
    if (!location.trim()) {
      throw new ValidationError('Location search term cannot be empty');
    }

    return this.getStations({ location_contains: location }, options);
  }

  /**
   * Get weather data for a station
   */
  async getWeatherData(
    stationId: string,
    fromDate?: DateTime,
    toDate?: DateTime,
    limit: number = 100
  ): Promise<WeatherData[]> {
    try {
      if (!stationId) {
        throw new ValidationError('Station ID is required');
      }

      if (fromDate && toDate && new Date(fromDate) >= new Date(toDate)) {
        throw new ValidationError('From date must be before to date');
      }

      const response = await this.graphql.query(GET_WEATHER_DATA_QUERY, {
        stationId,
        from: fromDate,
        to: toDate,
        first: limit,
      });

      return response.weatherData || [];
    } catch (error) {
      if (error instanceof ValidationError) {
        throw error;
      }
      throw new WeatherError(
        `Failed to get weather data: ${error instanceof Error ? error.message : 'Unknown error'}`,
        stationId
      );
    }
  }

  /**
   * Get latest weather data for a station
   */
  async getLatestWeatherData(stationId: string): Promise<WeatherData | null> {
    try {
      const station = await this.getStation(stationId);
      return station?.latestWeatherData || null;
    } catch (error) {
      throw new WeatherError(
        `Failed to get latest weather data: ${error instanceof Error ? error.message : 'Unknown error'}`,
        stationId
      );
    }
  }

  /**
   * Submit weather data and earn rewards
   */
  async submitWeatherData(
    input: WeatherDataInput
  ): Promise<WeatherDataSubmissionResult> {
    try {
      // Validate input data
      this.validateWeatherDataInput(input);

      const response = await this.graphql.mutate(SUBMIT_WEATHER_DATA_MUTATION, {
        input,
      });

      const result = response.submitWeatherData;

      if (!result.success) {
        throw new WeatherError(
          result.error || 'Failed to submit weather data',
          input.stationId
        );
      }

      return result;
    } catch (error) {
      if (error instanceof ValidationError || error instanceof WeatherError) {
        throw error;
      }
      throw new WeatherError(
        `Failed to submit weather data: ${error instanceof Error ? error.message : 'Unknown error'}`,
        input.stationId
      );
    }
  }

  /**
   * Subscribe to weather data updates for a station
   */
  subscribeToWeatherData(
    stationId: string | undefined,
    onData: (data: WeatherData) => void,
    onError?: (error: Error) => void
  ): () => void {
    return this.graphql.subscribe(
      WEATHER_DATA_UPDATED_SUBSCRIPTION,
      { stationId },
      {
        onData,
        onError,
      }
    );
  }

  /**
   * Calculate potential rewards for weather data submission
   */
  calculatePotentialRewards(
    qualityScore: number,
    dataFrequency: number = 1
  ): string {
    // Base reward rate (in wei) - this would come from contract
    const baseReward = BigInt('1000000000000000000'); // 1 WRF

    // Quality bonus (0-100% based on quality score)
    const qualityMultiplier = Math.max(0.1, qualityScore / 100);

    // Frequency bonus for consistent data submission
    const frequencyMultiplier = Math.min(2.0, 1 + (dataFrequency - 1) * 0.1);

    const totalReward =
      (baseReward *
        BigInt(Math.floor(qualityMultiplier * frequencyMultiplier * 100))) /
      BigInt(100);

    return totalReward.toString();
  }

  /**
   * Validate weather data quality
   */
  validateDataQuality(data: WeatherData): {
    isValid: boolean;
    qualityScore: number;
    issues: string[];
  } {
    const issues: string[] = [];
    let qualityScore = 100;

    // Temperature validation (-100°C to 100°C)
    if (data.temperature < -10000 || data.temperature > 10000) {
      issues.push('Temperature out of valid range');
      qualityScore -= 20;
    }

    // Humidity validation (0-100%)
    if (data.humidity < 0 || data.humidity > 100) {
      issues.push('Humidity out of valid range');
      qualityScore -= 15;
    }

    // Pressure validation (800-1200 hPa)
    if (data.pressure < 80000 || data.pressure > 120000) {
      issues.push('Pressure out of valid range');
      qualityScore -= 15;
    }

    // Wind speed validation (0-200 m/s)
    if (data.windSpeed < 0 || data.windSpeed > 20000) {
      issues.push('Wind speed out of valid range');
      qualityScore -= 10;
    }

    // Rainfall validation (0-1000 mm)
    if (data.rainfall < 0 || data.rainfall > 100000) {
      issues.push('Rainfall out of valid range');
      qualityScore -= 10;
    }

    // Confidence validation (0-100%)
    if (data.confidence < 0 || data.confidence > 100) {
      issues.push('Confidence score out of valid range');
      qualityScore -= 10;
    }

    // Apply confidence penalty to overall quality
    qualityScore = Math.floor(qualityScore * (data.confidence / 100));

    return {
      isValid: issues.length === 0,
      qualityScore: Math.max(0, qualityScore),
      issues,
    };
  }

  /**
   * Convert raw sensor values to display values
   */
  formatWeatherData(data: WeatherData): {
    temperature: number;
    humidity: number;
    pressure: number;
    windSpeed: number;
    rainfall: number;
    confidence: number;
  } {
    return {
      temperature: data.temperature / 100, // Convert from centidegrees
      humidity: data.humidity,
      pressure: data.pressure / 100, // Convert from centihPa
      windSpeed: data.windSpeed / 100, // Convert from cm/s
      rainfall: data.rainfall / 100, // Convert from centimm
      confidence: data.confidence,
    };
  }

  /**
   * Get weather statistics for a station over a time period
   */
  async getWeatherStatistics(
    stationId: string,
    fromDate: DateTime,
    toDate: DateTime
  ): Promise<{
    averageTemperature: number;
    averageHumidity: number;
    averagePressure: number;
    totalRainfall: number;
    dataPoints: number;
    qualityScore: number;
  }> {
    try {
      const weatherData = await this.getWeatherData(
        stationId,
        fromDate,
        toDate,
        1000
      );

      if (weatherData.length === 0) {
        throw new WeatherError(
          'No weather data found for the specified period',
          stationId
        );
      }

      const totals = weatherData.reduce(
        (acc, data) => ({
          temperature: acc.temperature + data.temperature,
          humidity: acc.humidity + data.humidity,
          pressure: acc.pressure + data.pressure,
          rainfall: acc.rainfall + data.rainfall,
          confidence: acc.confidence + data.confidence,
        }),
        { temperature: 0, humidity: 0, pressure: 0, rainfall: 0, confidence: 0 }
      );

      const count = weatherData.length;

      return {
        averageTemperature: totals.temperature / count / 100, // Convert to actual degrees
        averageHumidity: totals.humidity / count,
        averagePressure: totals.pressure / count / 100, // Convert to actual hPa
        totalRainfall: totals.rainfall / 100, // Convert to actual mm
        dataPoints: count,
        qualityScore: totals.confidence / count,
      };
    } catch (error) {
      if (error instanceof WeatherError) {
        throw error;
      }
      throw new WeatherError(
        `Failed to calculate weather statistics: ${error instanceof Error ? error.message : 'Unknown error'}`,
        stationId
      );
    }
  }

  /**
   * Validate weather data input
   */
  private validateWeatherDataInput(input: WeatherDataInput): void {
    if (!input.stationId) {
      throw new ValidationError('Station ID is required');
    }

    if (!input.signature) {
      throw new ValidationError('Signature is required');
    }

    // Validate temperature (-100°C to 100°C)
    if (input.temperature < -100 || input.temperature > 100) {
      throw new ValidationError(
        'Temperature must be between -100 and 100 degrees Celsius'
      );
    }

    // Validate humidity (0-100%)
    if (input.humidity < 0 || input.humidity > 100) {
      throw new ValidationError('Humidity must be between 0 and 100 percent');
    }

    // Validate pressure (800-1200 hPa)
    if (input.pressure < 800 || input.pressure > 1200) {
      throw new ValidationError('Pressure must be between 800 and 1200 hPa');
    }

    // Validate wind speed (0-200 m/s)
    if (input.windSpeed < 0 || input.windSpeed > 200) {
      throw new ValidationError('Wind speed must be between 0 and 200 m/s');
    }

    // Validate rainfall (0-1000 mm)
    if (input.rainfall < 0 || input.rainfall > 1000) {
      throw new ValidationError('Rainfall must be between 0 and 1000 mm');
    }

    // Validate confidence (0-100%)
    if (input.confidence < 0 || input.confidence > 100) {
      throw new ValidationError('Confidence must be between 0 and 100 percent');
    }
  }
}
