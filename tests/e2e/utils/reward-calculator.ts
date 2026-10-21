export interface RewardComponents {
  baseReward: number;
  qualityMultiplier: number;
  frequencyBonus: number;
  geographicMultiplier: number;
  verificationBonus: number;
  consistencyBonus: number;
  totalReward: number;
}

export interface LocationData {
  lat: number;
  lon: number;
  name: string;
  dataDensity?: 'sparse' | 'medium' | 'dense';
}

export interface QualityMetrics {
  accuracy: number;
  completeness: number;
  timeliness: number;
  overall?: number;
}

export interface SubmissionFrequency {
  intervalSeconds: number;
  consistency: number; // 0-1, how consistent the intervals are
  uptime: number; // 0-1, percentage of expected submissions received
}

export interface RewardCalculationInput {
  dataQuality: QualityMetrics;
  location: LocationData;
  frequency: SubmissionFrequency;
  submissionCount: number;
  timeRangeHours: number;
  hasVerification?: boolean;
  deviceType?: string;
  dataTypes: string[];
}

export class RewardCalculator {
  // Base reward rates (WRF tokens per data submission)
  private readonly BASE_RATES = {
    temperature: 0.1,
    humidity: 0.1,
    pressure: 0.15,
    windSpeed: 0.12,
    windDirection: 0.08,
    rainfall: 0.2,
    uvIndex: 0.1,
    visibility: 0.05,
  };

  // Quality multipliers
  private readonly QUALITY_MULTIPLIERS = {
    excellent: { min: 0.95, multiplier: 1.25 }, // 95%+ quality
    high: { min: 0.85, multiplier: 1.1 }, // 85-95% quality
    good: { min: 0.75, multiplier: 1.0 }, // 75-85% quality
    fair: { min: 0.65, multiplier: 0.85 }, // 65-75% quality
    poor: { min: 0.0, multiplier: 0.6 }, // <65% quality
  };

  // Geographic multipliers based on data density
  private readonly GEOGRAPHIC_MULTIPLIERS = {
    sparse: 2.0, // Very few weather stations in area
    medium: 1.3, // Moderate coverage
    dense: 1.0, // Well-covered area
  };

  // Frequency bonus thresholds
  private readonly FREQUENCY_BONUSES = {
    realTime: { maxInterval: 60, bonus: 0.2 }, // <= 1 minute
    frequent: { maxInterval: 300, bonus: 0.15 }, // <= 5 minutes
    regular: { maxInterval: 900, bonus: 0.1 }, // <= 15 minutes
    standard: { maxInterval: 3600, bonus: 0.05 }, // <= 1 hour
    periodic: { maxInterval: 86400, bonus: 0.0 }, // <= 24 hours
  };

  // Device type multipliers
  private readonly DEVICE_MULTIPLIERS = {
    professional: 1.2, // Professional weather stations
    prosumer: 1.0, // High-end consumer devices
    consumer: 0.9, // Standard consumer devices
    iot: 0.8, // Basic IoT sensors
    mobile: 0.7, // Mobile/temporary sensors
  };

  // Known sparse regions (simplified for testing)
  private readonly SPARSE_REGIONS = [
    { lat: 71.0489, lon: -156.7889, name: 'Alaska Arctic' },
    { lat: -54.8019, lon: -68.303, name: 'Tierra del Fuego' },
    { lat: 77.875, lon: 20.9752, name: 'Svalbard' },
    { lat: -75.2509, lon: -0.0742, name: 'Antarctica Coast' },
    { lat: 23.4241, lon: 25.6628, name: 'Sahara Desert' },
  ];

  /**
   * Calculate comprehensive reward for weather data submission
   */
  calculateReward(input: RewardCalculationInput): RewardComponents {
    // Calculate base reward for all data types submitted
    const baseReward = this.calculateBaseReward(input.dataTypes);

    // Calculate quality multiplier
    const qualityMultiplier = this.calculateQualityMultiplier(
      input.dataQuality
    );

    // Calculate frequency bonus
    const frequencyBonus = this.calculateFrequencyBonus(input.frequency);

    // Calculate geographic multiplier
    const geographicMultiplier = this.calculateGeographicMultiplier(
      input.location
    );

    // Calculate verification bonus
    const verificationBonus = input.hasVerification
      ? this.calculateVerificationBonus(input.dataQuality)
      : 0;

    // Calculate consistency bonus
    const consistencyBonus = this.calculateConsistencyBonus(
      input.frequency,
      input.submissionCount,
      input.timeRangeHours
    );

    // Calculate total reward
    const adjustedBase = baseReward * qualityMultiplier * geographicMultiplier;
    const totalBonus = frequencyBonus + verificationBonus + consistencyBonus;
    const totalReward = adjustedBase + totalBonus;

    return {
      baseReward: this.roundToDecimals(baseReward, 4),
      qualityMultiplier: this.roundToDecimals(qualityMultiplier, 3),
      frequencyBonus: this.roundToDecimals(frequencyBonus, 4),
      geographicMultiplier: this.roundToDecimals(geographicMultiplier, 2),
      verificationBonus: this.roundToDecimals(verificationBonus, 4),
      consistencyBonus: this.roundToDecimals(consistencyBonus, 4),
      totalReward: this.roundToDecimals(totalReward, 4),
    };
  }

  /**
   * Calculate base reward based on data types
   */
  private calculateBaseReward(dataTypes: string[]): number {
    return dataTypes.reduce((total, dataType) => {
      const rate =
        this.BASE_RATES[dataType as keyof typeof this.BASE_RATES] || 0.05;
      return total + rate;
    }, 0);
  }

  /**
   * Calculate quality multiplier based on overall quality metrics
   */
  private calculateQualityMultiplier(quality: QualityMetrics): number {
    // Calculate overall quality if not provided
    const overallQuality =
      quality.overall ||
      (quality.accuracy + quality.completeness + quality.timeliness) / 3;

    for (const [level, config] of Object.entries(this.QUALITY_MULTIPLIERS)) {
      if (overallQuality >= config.min) {
        return config.multiplier;
      }
    }

    return this.QUALITY_MULTIPLIERS.poor.multiplier;
  }

  /**
   * Calculate frequency bonus based on submission interval and consistency
   */
  private calculateFrequencyBonus(frequency: SubmissionFrequency): number {
    let baseBonus = 0;

    // Determine frequency tier
    for (const [tier, config] of Object.entries(this.FREQUENCY_BONUSES)) {
      if (frequency.intervalSeconds <= config.maxInterval) {
        baseBonus = config.bonus;
        break;
      }
    }

    // Apply consistency modifier
    const consistencyModifier = 0.5 + frequency.consistency * 0.5; // 50% to 100% of bonus

    // Apply uptime modifier
    const uptimeModifier = Math.max(0.1, frequency.uptime); // Minimum 10% of bonus

    return baseBonus * consistencyModifier * uptimeModifier;
  }

  /**
   * Calculate geographic multiplier based on location data density
   */
  private calculateGeographicMultiplier(location: LocationData): number {
    // Use provided density or determine from coordinates
    const density = location.dataDensity || this.determineDataDensity(location);
    return this.GEOGRAPHIC_MULTIPLIERS[density];
  }

  /**
   * Calculate verification bonus for cross-validated data
   */
  private calculateVerificationBonus(quality: QualityMetrics): number {
    const overallQuality =
      quality.overall ||
      (quality.accuracy + quality.completeness + quality.timeliness) / 3;

    // Higher quality data gets bigger verification bonus
    if (overallQuality >= 0.95) return 0.15;
    if (overallQuality >= 0.9) return 0.12;
    if (overallQuality >= 0.85) return 0.1;
    if (overallQuality >= 0.8) return 0.08;
    return 0.05;
  }

  /**
   * Calculate consistency bonus for long-term reliable providers
   */
  private calculateConsistencyBonus(
    frequency: SubmissionFrequency,
    submissionCount: number,
    timeRangeHours: number
  ): number {
    // Calculate expected submissions
    const expectedSubmissions =
      (timeRangeHours * 3600) / frequency.intervalSeconds;
    const submissionRatio = submissionCount / expectedSubmissions;

    // Only award consistency bonus for providers with good track record
    if (submissionCount < 100 || timeRangeHours < 24) return 0; // Minimum thresholds

    let consistencyBonus = 0;

    // High consistency (>95% of expected submissions)
    if (submissionRatio >= 0.95 && frequency.consistency >= 0.9) {
      consistencyBonus += 0.08;
    }
    // Good consistency (>85% of expected submissions)
    else if (submissionRatio >= 0.85 && frequency.consistency >= 0.8) {
      consistencyBonus += 0.05;
    }
    // Fair consistency (>75% of expected submissions)
    else if (submissionRatio >= 0.75 && frequency.consistency >= 0.7) {
      consistencyBonus += 0.03;
    }

    // Long-term provider bonus (additional bonus for providers with >30 days)
    if (timeRangeHours > 720) {
      // 30 days
      consistencyBonus += 0.02;
    }

    // Very long-term provider bonus (additional bonus for providers with >90 days)
    if (timeRangeHours > 2160) {
      // 90 days
      consistencyBonus += 0.03;
    }

    return consistencyBonus;
  }

  /**
   * Determine data density for a location
   */
  private determineDataDensity(
    location: LocationData
  ): 'sparse' | 'medium' | 'dense' {
    // Check if location is in known sparse regions
    for (const sparseRegion of this.SPARSE_REGIONS) {
      const distance = this.calculateDistance(
        location.lat,
        location.lon,
        sparseRegion.lat,
        sparseRegion.lon
      );
      if (distance < 500) {
        // Within 500km of sparse region center
        return 'sparse';
      }
    }

    // Simple heuristics based on latitude (approximation)
    const absLat = Math.abs(location.lat);

    // Arctic/Antarctic regions are sparse
    if (absLat > 70) return 'sparse';

    // Extreme latitudes are medium density
    if (absLat > 60) return 'medium';

    // Tropical and temperate zones vary
    // For testing purposes, classify based on longitude patterns
    const absLon = Math.abs(location.lon);

    // Ocean regions (simplified)
    if (absLon > 160 || (absLon < 30 && absLat < 30)) return 'medium';

    // Most populated continental areas
    return 'dense';
  }

  /**
   * Calculate distance between two geographic points using Haversine formula
   */
  private calculateDistance(
    lat1: number,
    lon1: number,
    lat2: number,
    lon2: number
  ): number {
    const R = 6371; // Earth's radius in kilometers
    const dLat = this.toRadians(lat2 - lat1);
    const dLon = this.toRadians(lon2 - lon1);

    const a =
      Math.sin(dLat / 2) * Math.sin(dLat / 2) +
      Math.cos(this.toRadians(lat1)) *
        Math.cos(this.toRadians(lat2)) *
        Math.sin(dLon / 2) *
        Math.sin(dLon / 2);

    const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
    return R * c;
  }

  /**
   * Estimate rewards for different scenarios
   */
  estimateRewardScenarios(): {
    highQualityFrequent: RewardComponents;
    standardQualityRegular: RewardComponents;
    sparseRegionProvider: RewardComponents;
    longTermProvider: RewardComponents;
  } {
    const commonDataTypes = [
      'temperature',
      'humidity',
      'pressure',
      'windSpeed',
    ];

    const highQualityFrequent = this.calculateReward({
      dataQuality: { accuracy: 0.98, completeness: 1.0, timeliness: 0.99 },
      location: {
        lat: 40.7128,
        lon: -74.006,
        name: 'New York, NY',
        dataDensity: 'dense',
      },
      frequency: { intervalSeconds: 300, consistency: 0.95, uptime: 0.98 },
      submissionCount: 1440, // 1 week of 5-minute intervals
      timeRangeHours: 168, // 1 week
      hasVerification: true,
      dataTypes: commonDataTypes,
    });

    const standardQualityRegular = this.calculateReward({
      dataQuality: { accuracy: 0.85, completeness: 0.95, timeliness: 0.9 },
      location: {
        lat: 34.0522,
        lon: -118.2437,
        name: 'Los Angeles, CA',
        dataDensity: 'medium',
      },
      frequency: { intervalSeconds: 900, consistency: 0.8, uptime: 0.85 },
      submissionCount: 200, // Less frequent submissions
      timeRangeHours: 168,
      hasVerification: false,
      dataTypes: commonDataTypes,
    });

    const sparseRegionProvider = this.calculateReward({
      dataQuality: { accuracy: 0.9, completeness: 0.98, timeliness: 0.95 },
      location: {
        lat: 71.0489,
        lon: -156.7889,
        name: 'Utqiagvik, AK',
        dataDensity: 'sparse',
      },
      frequency: { intervalSeconds: 600, consistency: 0.85, uptime: 0.9 },
      submissionCount: 672, // 10-minute intervals
      timeRangeHours: 168,
      hasVerification: true,
      dataTypes: commonDataTypes,
    });

    const longTermProvider = this.calculateReward({
      dataQuality: { accuracy: 0.92, completeness: 0.97, timeliness: 0.94 },
      location: {
        lat: 41.8781,
        lon: -87.6298,
        name: 'Chicago, IL',
        dataDensity: 'dense',
      },
      frequency: { intervalSeconds: 300, consistency: 0.92, uptime: 0.95 },
      submissionCount: 12960, // 90 days of 5-minute intervals
      timeRangeHours: 2160, // 90 days
      hasVerification: true,
      dataTypes: [...commonDataTypes, 'rainfall', 'uvIndex'],
    });

    return {
      highQualityFrequent,
      standardQualityRegular,
      sparseRegionProvider,
      longTermProvider,
    };
  }

  /**
   * Calculate expected rewards over time periods
   */
  calculateExpectedRewards(
    dailySubmissions: number,
    avgQuality: number,
    location: LocationData,
    timeframeHours: number
  ): {
    hourly: number;
    daily: number;
    weekly: number;
    monthly: number;
    total: number;
  } {
    // Estimate average reward per submission
    const estimatedReward = this.calculateReward({
      dataQuality: {
        accuracy: avgQuality,
        completeness: avgQuality,
        timeliness: avgQuality,
      },
      location,
      frequency: {
        intervalSeconds: 86400 / dailySubmissions,
        consistency: 0.85,
        uptime: 0.9,
      },
      submissionCount: dailySubmissions * (timeframeHours / 24),
      timeRangeHours: timeframeHours,
      hasVerification: avgQuality > 0.9,
      dataTypes: ['temperature', 'humidity', 'pressure', 'windSpeed'],
    }).totalReward;

    const hourlySubmissions = dailySubmissions / 24;
    const hourly = estimatedReward * hourlySubmissions;
    const daily = estimatedReward * dailySubmissions;
    const weekly = daily * 7;
    const monthly = daily * 30;
    const total = hourly * timeframeHours;

    return {
      hourly: this.roundToDecimals(hourly, 4),
      daily: this.roundToDecimals(daily, 4),
      weekly: this.roundToDecimals(weekly, 4),
      monthly: this.roundToDecimals(monthly, 4),
      total: this.roundToDecimals(total, 4),
    };
  }

  /**
   * Validate reward calculation parameters
   */
  validateCalculationInput(input: RewardCalculationInput): {
    isValid: boolean;
    errors: string[];
    warnings: string[];
  } {
    const errors: string[] = [];
    const warnings: string[] = [];

    // Validate quality metrics
    if (input.dataQuality.accuracy < 0 || input.dataQuality.accuracy > 1) {
      errors.push('Accuracy must be between 0 and 1');
    }
    if (
      input.dataQuality.completeness < 0 ||
      input.dataQuality.completeness > 1
    ) {
      errors.push('Completeness must be between 0 and 1');
    }
    if (input.dataQuality.timeliness < 0 || input.dataQuality.timeliness > 1) {
      errors.push('Timeliness must be between 0 and 1');
    }

    // Validate location
    if (Math.abs(input.location.lat) > 90) {
      errors.push('Latitude must be between -90 and 90');
    }
    if (Math.abs(input.location.lon) > 180) {
      errors.push('Longitude must be between -180 and 180');
    }

    // Validate frequency
    if (input.frequency.intervalSeconds <= 0) {
      errors.push('Frequency interval must be positive');
    }
    if (input.frequency.consistency < 0 || input.frequency.consistency > 1) {
      errors.push('Consistency must be between 0 and 1');
    }
    if (input.frequency.uptime < 0 || input.frequency.uptime > 1) {
      errors.push('Uptime must be between 0 and 1');
    }

    // Validate other parameters
    if (input.submissionCount < 0) {
      errors.push('Submission count must be non-negative');
    }
    if (input.timeRangeHours <= 0) {
      errors.push('Time range must be positive');
    }
    if (input.dataTypes.length === 0) {
      errors.push('At least one data type must be specified');
    }

    // Warnings for potentially unrealistic scenarios
    if (input.dataQuality.accuracy > 0.99) {
      warnings.push('Extremely high accuracy may be unrealistic');
    }
    if (input.frequency.intervalSeconds < 30) {
      warnings.push('Very frequent submissions may be impractical');
    }
    if (input.submissionCount / (input.timeRangeHours / 24) > 288) {
      // More than every 5 minutes
      warnings.push('Submission frequency seems very high for the time range');
    }

    return {
      isValid: errors.length === 0,
      errors,
      warnings,
    };
  }

  // Helper methods
  private toRadians(degrees: number): number {
    return degrees * (Math.PI / 180);
  }

  private roundToDecimals(value: number, decimals: number): number {
    return Math.round(value * Math.pow(10, decimals)) / Math.pow(10, decimals);
  }
}
