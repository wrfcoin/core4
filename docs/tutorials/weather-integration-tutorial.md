# WRFCoin Weather Integration Tutorial

**Build a complete weather station integration that submits data to the blockchain and earns WRF
rewards**

## Overview

In this tutorial, you'll learn how to:

1. Set up a weather station integration
2. Register your station with WRFCoin
3. Submit weather data and earn rewards
4. Monitor data quality and optimize rewards
5. Build a real-time weather dashboard

**Prerequisites:**

- Basic programming knowledge (JavaScript, Python, or Rust)
- API key from WRFCoin Developer Portal
- Access to weather data (real sensors or simulated data)

**Estimated Time:** 30-45 minutes

## Table of Contents

1. [Project Setup](#project-setup)
2. [Weather Station Registration](#weather-station-registration)
3. [Data Collection & Validation](#data-collection--validation)
4. [Submission & Rewards](#submission--rewards)
5. [Real-Time Dashboard](#real-time-dashboard)
6. [Production Optimization](#production-optimization)
7. [Troubleshooting](#troubleshooting)

## Project Setup

### 1. Initialize Your Project

Create a new project directory and install dependencies:

#### JavaScript/TypeScript Setup

```bash
mkdir wrfcoin-weather-station
cd wrfcoin-weather-station

# Initialize package.json
npm init -y

# Install WRFCoin SDK and dependencies
npm install @wrfcoin/sdk dotenv axios
npm install -D @types/node typescript

# Create TypeScript config
npx tsc --init
```

#### Python Setup

```bash
mkdir wrfcoin-weather-station
cd wrfcoin-weather-station

# Create virtual environment
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install dependencies
pip install wrfcoin-sdk python-dotenv requests asyncio
```

#### Rust Setup

```bash
mkdir wrfcoin-weather-station
cd wrfcoin-weather-station

# Initialize Cargo project
cargo init

# Add dependencies to Cargo.toml
```

Add to `Cargo.toml`:

```toml
[dependencies]
wrfcoin-sdk = "0.3.0"
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
reqwest = { version = "0.11", features = ["json"] }
dotenv = "0.15"
chrono = { version = "0.4", features = ["serde"] }
```

### 2. Environment Configuration

Create a `.env` file with your API credentials:

```bash
# .env
WRFCOIN_API_KEY=your-api-key-here
WRFCOIN_NETWORK=mainnet
WRFCOIN_ENDPOINT=http://localhost:3001

# Weather station configuration
STATION_NAME=My Home Weather Station
STATION_LATITUDE=30.6280
STATION_LONGITUDE=-96.3344
STATION_ELEVATION=104

# Data submission settings
SUBMISSION_INTERVAL=300  # 5 minutes
QUALITY_THRESHOLD=80     # Minimum quality score
```

### 3. Basic SDK Setup

#### JavaScript/TypeScript

```typescript
// src/config.ts
import dotenv from 'dotenv';
import { WRFCoinSDK } from '@wrfcoin/sdk';

dotenv.config();

export const config = {
  apiKey: process.env.WRFCOIN_API_KEY!,
  network: process.env.WRFCOIN_NETWORK || 'mainnet',
  endpoint: process.env.WRFCOIN_ENDPOINT || 'http://localhost:3001',
  station: {
    name: process.env.STATION_NAME || 'Default Weather Station',
    latitude: parseFloat(process.env.STATION_LATITUDE || '0'),
    longitude: parseFloat(process.env.STATION_LONGITUDE || '0'),
    elevation: parseInt(process.env.STATION_ELEVATION || '0'),
  },
  submissionInterval: parseInt(process.env.SUBMISSION_INTERVAL || '300') * 1000,
  qualityThreshold: parseInt(process.env.QUALITY_THRESHOLD || '80'),
};

export const sdk = new WRFCoinSDK({
  apiKey: config.apiKey,
  network: config.network,
  endpoint: config.endpoint,
});
```

#### Python

```python
# src/config.py
import os
from dotenv import load_dotenv
from wrfcoin_sdk import WRFCoinClient

load_dotenv()

config = {
    'api_key': os.getenv('WRFCOIN_API_KEY'),
    'network': os.getenv('WRFCOIN_NETWORK', 'mainnet'),
    'endpoint': os.getenv('WRFCOIN_ENDPOINT', 'http://localhost:3001'),
    'station': {
        'name': os.getenv('STATION_NAME', 'Default Weather Station'),
        'latitude': float(os.getenv('STATION_LATITUDE', '0')),
        'longitude': float(os.getenv('STATION_LONGITUDE', '0')),
        'elevation': int(os.getenv('STATION_ELEVATION', '0'))
    },
    'submission_interval': int(os.getenv('SUBMISSION_INTERVAL', '300')),
    'quality_threshold': int(os.getenv('QUALITY_THRESHOLD', '80'))
}

client = WRFCoinClient(
    api_key=config['api_key'],
    network=config['network'],
    endpoint=config['endpoint']
)
```

## Weather Station Registration

### 1. Register Your Station

Before submitting data, you need to register your weather station with WRFCoin:

#### JavaScript/TypeScript

```typescript
// src/station.ts
import { sdk, config } from './config';

export interface StationInfo {
  id: string;
  name: string;
  location: {
    latitude: number;
    longitude: number;
    elevation: number;
  };
  equipment: {
    temperatureSensor: string;
    humiditySensor: string;
    pressureSensor: string;
    windSensor?: string;
    rainGauge?: string;
  };
  status: 'active' | 'inactive';
  registrationDate: string;
}

export class WeatherStation {
  private stationInfo: StationInfo | null = null;

  async register(): Promise<StationInfo> {
    try {
      const registration = await sdk.weather.registerStation({
        name: config.station.name,
        location: {
          latitude: config.station.latitude,
          longitude: config.station.longitude,
          elevation: config.station.elevation,
        },
        equipment: {
          temperatureSensor: 'Digital Temperature Sensor',
          humiditySensor: 'Capacitive Humidity Sensor',
          pressureSensor: 'Barometric Pressure Sensor',
          windSensor: 'Anemometer',
          rainGauge: 'Digital Rain Gauge',
        },
        dataSubmissionInterval: config.submissionInterval / 1000,
        contactInfo: {
          email: 'station-owner@example.com',
        },
      });

      this.stationInfo = registration;
      console.log('✅ Weather station registered successfully');
      console.log(`Station ID: ${registration.id}`);
      console.log(
        `Location: ${registration.location.latitude}, ${registration.location.longitude}`
      );

      return registration;
    } catch (error) {
      console.error('❌ Failed to register station:', error.message);
      throw error;
    }
  }

  async getStationInfo(): Promise<StationInfo | null> {
    if (this.stationInfo) {
      return this.stationInfo;
    }

    try {
      // Try to find existing station by location
      const stations = await sdk.weather.getStations();
      const existingStation = stations.find(
        station =>
          Math.abs(station.location.latitude - config.station.latitude) < 0.001 &&
          Math.abs(station.location.longitude - config.station.longitude) < 0.001
      );

      if (existingStation) {
        this.stationInfo = existingStation;
        console.log(`📍 Found existing station: ${existingStation.id}`);
        return existingStation;
      }

      console.log('🔍 No existing station found, need to register');
      return null;
    } catch (error) {
      console.error('❌ Failed to get station info:', error.message);
      return null;
    }
  }

  getId(): string | null {
    return this.stationInfo?.id || null;
  }
}
```

#### Python

```python
# src/station.py
import asyncio
from typing import Optional, Dict, Any
from src.config import client, config

class WeatherStation:
    def __init__(self):
        self.station_info: Optional[Dict[str, Any]] = None

    async def register(self) -> Dict[str, Any]:
        try:
            registration = await client.weather.register_station({
                'name': config['station']['name'],
                'location': {
                    'latitude': config['station']['latitude'],
                    'longitude': config['station']['longitude'],
                    'elevation': config['station']['elevation']
                },
                'equipment': {
                    'temperatureSensor': 'Digital Temperature Sensor',
                    'humiditySensor': 'Capacitive Humidity Sensor',
                    'pressureSensor': 'Barometric Pressure Sensor',
                    'windSensor': 'Anemometer',
                    'rainGauge': 'Digital Rain Gauge'
                },
                'dataSubmissionInterval': config['submission_interval'],
                'contactInfo': {
                    'email': 'station-owner@example.com'
                }
            })

            self.station_info = registration
            print('✅ Weather station registered successfully')
            print(f"Station ID: {registration['id']}")
            print(f"Location: {registration['location']['latitude']}, {registration['location']['longitude']}")

            return registration
        except Exception as error:
            print(f'❌ Failed to register station: {error}')
            raise error

    async def get_station_info(self) -> Optional[Dict[str, Any]]:
        if self.station_info:
            return self.station_info

        try:
            stations = await client.weather.get_stations()
            existing_station = None

            for station in stations:
                lat_diff = abs(station['location']['latitude'] - config['station']['latitude'])
                lon_diff = abs(station['location']['longitude'] - config['station']['longitude'])

                if lat_diff < 0.001 and lon_diff < 0.001:
                    existing_station = station
                    break

            if existing_station:
                self.station_info = existing_station
                print(f"📍 Found existing station: {existing_station['id']}")
                return existing_station

            print('🔍 No existing station found, need to register')
            return None
        except Exception as error:
            print(f'❌ Failed to get station info: {error}')
            return None

    def get_id(self) -> Optional[str]:
        return self.station_info['id'] if self.station_info else None
```

### 2. Initialize Station

Create a setup script to register your station:

#### JavaScript/TypeScript

```typescript
// src/setup.ts
import { WeatherStation } from './station';

async function setupStation() {
  const station = new WeatherStation();

  console.log('🚀 Setting up weather station...');

  // Check for existing station
  let stationInfo = await station.getStationInfo();

  if (!stationInfo) {
    console.log('📝 Registering new weather station...');
    stationInfo = await station.register();
  }

  console.log('✅ Station setup complete!');
  console.log(`Station ID: ${stationInfo.id}`);
  console.log(`Name: ${stationInfo.name}`);
  console.log(`Location: ${stationInfo.location.latitude}, ${stationInfo.location.longitude}`);

  return station;
}

// Run setup if this file is executed directly
if (require.main === module) {
  setupStation().catch(console.error);
}

export { setupStation };
```

## Data Collection & Validation

### 1. Weather Data Collection

Create a data collector that simulates or reads real weather sensor data:

#### JavaScript/TypeScript

```typescript
// src/dataCollector.ts
export interface WeatherReading {
  temperature: number; // °C
  humidity: number; // %
  pressure: number; // hPa
  windSpeed?: number; // m/s
  windDirection?: number; // degrees
  rainfall?: number; // mm
  solarRadiation?: number; // W/m²
  uvIndex?: number; // UV index
  timestamp: string; // ISO 8601
}

export class WeatherDataCollector {
  private sensors: {
    temperature: () => number;
    humidity: () => number;
    pressure: () => number;
    windSpeed: () => number;
    windDirection: () => number;
    rainfall: () => number;
  };

  constructor() {
    // Initialize sensor readers (simulated for this tutorial)
    this.sensors = {
      temperature: this.simulateTemperature,
      humidity: this.simulateHumidity,
      pressure: this.simulatePressure,
      windSpeed: this.simulateWindSpeed,
      windDirection: this.simulateWindDirection,
      rainfall: this.simulateRainfall,
    };
  }

  // Simulate temperature sensor (replace with real sensor reading)
  private simulateTemperature(): number {
    // Realistic temperature simulation with daily cycle
    const hour = new Date().getHours();
    const baseTemp = 20; // Base temperature
    const dailyVariation = 10 * Math.sin(((hour - 6) * Math.PI) / 12); // Daily cycle
    const randomVariation = (Math.random() - 0.5) * 2; // ±1°C random

    return baseTemp + dailyVariation + randomVariation;
  }

  private simulateHumidity(): number {
    // Humidity typically inversely related to temperature
    const temp = this.simulateTemperature();
    const baseHumidity = 70 - (temp - 20) * 1.5; // Rough inverse relationship
    const randomVariation = (Math.random() - 0.5) * 10; // ±5% random

    return Math.max(10, Math.min(100, baseHumidity + randomVariation));
  }

  private simulatePressure(): number {
    // Atmospheric pressure (typical range 980-1050 hPa)
    const basePressure = 1013.25;
    const weatherVariation = (Math.random() - 0.5) * 40; // ±20 hPa

    return basePressure + weatherVariation;
  }

  private simulateWindSpeed(): number {
    // Wind speed (0-30 m/s, typically 0-10 m/s)
    return Math.random() * 10;
  }

  private simulateWindDirection(): number {
    // Wind direction (0-360 degrees)
    return Math.random() * 360;
  }

  private simulateRainfall(): number {
    // Rainfall (usually 0, occasionally up to 10mm/hr)
    return Math.random() < 0.1 ? Math.random() * 10 : 0;
  }

  async collectReading(): Promise<WeatherReading> {
    try {
      const reading: WeatherReading = {
        temperature: this.sensors.temperature(),
        humidity: this.sensors.humidity(),
        pressure: this.sensors.pressure(),
        windSpeed: this.sensors.windSpeed(),
        windDirection: this.sensors.windDirection(),
        rainfall: this.sensors.rainfall(),
        timestamp: new Date().toISOString(),
      };

      // Validate reading
      this.validateReading(reading);

      return reading;
    } catch (error) {
      console.error('❌ Failed to collect weather reading:', error.message);
      throw error;
    }
  }

  private validateReading(reading: WeatherReading): void {
    // Basic validation checks
    if (reading.temperature < -50 || reading.temperature > 60) {
      throw new Error(`Invalid temperature: ${reading.temperature}°C`);
    }

    if (reading.humidity < 0 || reading.humidity > 100) {
      throw new Error(`Invalid humidity: ${reading.humidity}%`);
    }

    if (reading.pressure < 800 || reading.pressure > 1200) {
      throw new Error(`Invalid pressure: ${reading.pressure} hPa`);
    }

    if (reading.windSpeed && (reading.windSpeed < 0 || reading.windSpeed > 100)) {
      throw new Error(`Invalid wind speed: ${reading.windSpeed} m/s`);
    }
  }

  // For real sensor integration, replace simulation methods with actual sensor readers
  connectTemperatureSensor(reader: () => number): void {
    this.sensors.temperature = reader;
  }

  connectHumiditySensor(reader: () => number): void {
    this.sensors.humidity = reader;
  }

  connectPressureSensor(reader: () => number): void {
    this.sensors.pressure = reader;
  }

  // Example: Connect to actual sensors
  async connectToArduino(): Promise<void> {
    // Example implementation for Arduino/serial connection
    // This would require additional libraries like 'serialport'
    console.log('🔌 Connecting to Arduino sensors...');

    // TODO: Implement actual sensor connections
    // const SerialPort = require('serialport');
    // const port = new SerialPort('/dev/ttyUSB0', { baudRate: 9600 });

    console.log('⚠️  Using simulated sensor data for tutorial');
  }
}
```

#### Python

```python
# src/data_collector.py
import asyncio
import json
import math
import random
from datetime import datetime
from typing import Dict, Any, Callable, Optional

class WeatherReading:
    def __init__(
        self,
        temperature: float,
        humidity: float,
        pressure: float,
        wind_speed: Optional[float] = None,
        wind_direction: Optional[float] = None,
        rainfall: Optional[float] = None,
        solar_radiation: Optional[float] = None,
        uv_index: Optional[float] = None,
        timestamp: Optional[str] = None
    ):
        self.temperature = temperature
        self.humidity = humidity
        self.pressure = pressure
        self.wind_speed = wind_speed
        self.wind_direction = wind_direction
        self.rainfall = rainfall
        self.solar_radiation = solar_radiation
        self.uv_index = uv_index
        self.timestamp = timestamp or datetime.now().isoformat()

    def to_dict(self) -> Dict[str, Any]:
        return {
            'temperature': self.temperature,
            'humidity': self.humidity,
            'pressure': self.pressure,
            'windSpeed': self.wind_speed,
            'windDirection': self.wind_direction,
            'rainfall': self.rainfall,
            'solarRadiation': self.solar_radiation,
            'uvIndex': self.uv_index,
            'timestamp': self.timestamp
        }

class WeatherDataCollector:
    def __init__(self):
        self.sensors = {
            'temperature': self._simulate_temperature,
            'humidity': self._simulate_humidity,
            'pressure': self._simulate_pressure,
            'wind_speed': self._simulate_wind_speed,
            'wind_direction': self._simulate_wind_direction,
            'rainfall': self._simulate_rainfall
        }

    def _simulate_temperature(self) -> float:
        """Simulate temperature sensor with daily cycle"""
        hour = datetime.now().hour
        base_temp = 20
        daily_variation = 10 * math.sin((hour - 6) * math.pi / 12)
        random_variation = (random.random() - 0.5) * 2

        return base_temp + daily_variation + random_variation

    def _simulate_humidity(self) -> float:
        """Simulate humidity sensor inversely related to temperature"""
        temp = self._simulate_temperature()
        base_humidity = 70 - (temp - 20) * 1.5
        random_variation = (random.random() - 0.5) * 10

        return max(10, min(100, base_humidity + random_variation))

    def _simulate_pressure(self) -> float:
        """Simulate atmospheric pressure sensor"""
        base_pressure = 1013.25
        weather_variation = (random.random() - 0.5) * 40

        return base_pressure + weather_variation

    def _simulate_wind_speed(self) -> float:
        """Simulate wind speed sensor"""
        return random.random() * 10

    def _simulate_wind_direction(self) -> float:
        """Simulate wind direction sensor"""
        return random.random() * 360

    def _simulate_rainfall(self) -> float:
        """Simulate rainfall sensor"""
        return random.random() * 10 if random.random() < 0.1 else 0

    async def collect_reading(self) -> WeatherReading:
        """Collect a complete weather reading from all sensors"""
        try:
            reading = WeatherReading(
                temperature=self.sensors['temperature'](),
                humidity=self.sensors['humidity'](),
                pressure=self.sensors['pressure'](),
                wind_speed=self.sensors['wind_speed'](),
                wind_direction=self.sensors['wind_direction'](),
                rainfall=self.sensors['rainfall']()
            )

            self._validate_reading(reading)
            return reading

        except Exception as error:
            print(f'❌ Failed to collect weather reading: {error}')
            raise error

    def _validate_reading(self, reading: WeatherReading) -> None:
        """Validate sensor readings for reasonable values"""
        if reading.temperature < -50 or reading.temperature > 60:
            raise ValueError(f'Invalid temperature: {reading.temperature}°C')

        if reading.humidity < 0 or reading.humidity > 100:
            raise ValueError(f'Invalid humidity: {reading.humidity}%')

        if reading.pressure < 800 or reading.pressure > 1200:
            raise ValueError(f'Invalid pressure: {reading.pressure} hPa')

        if reading.wind_speed and (reading.wind_speed < 0 or reading.wind_speed > 100):
            raise ValueError(f'Invalid wind speed: {reading.wind_speed} m/s')

    def connect_temperature_sensor(self, reader: Callable[[], float]) -> None:
        """Connect actual temperature sensor"""
        self.sensors['temperature'] = reader

    def connect_humidity_sensor(self, reader: Callable[[], float]) -> None:
        """Connect actual humidity sensor"""
        self.sensors['humidity'] = reader

    def connect_pressure_sensor(self, reader: Callable[[], float]) -> None:
        """Connect actual pressure sensor"""
        self.sensors['pressure'] = reader

    async def connect_to_arduino(self) -> None:
        """Example implementation for Arduino sensor connection"""
        print('🔌 Connecting to Arduino sensors...')

        # TODO: Implement actual sensor connections
        # import serial
        # ser = serial.Serial('/dev/ttyUSB0', 9600)

        print('⚠️  Using simulated sensor data for tutorial')
```

### 2. Data Quality Validation

Implement physics-based validation to ensure data quality:

#### JavaScript/TypeScript

```typescript
// src/validator.ts
import { WeatherReading } from './dataCollector';

export interface ValidationResult {
  isValid: boolean;
  qualityScore: number;
  issues: string[];
  recommendations: string[];
}

export class WeatherDataValidator {
  async validateReading(reading: WeatherReading): Promise<ValidationResult> {
    const issues: string[] = [];
    const recommendations: string[] = [];
    let qualityScore = 100;

    // Physics-based validations
    qualityScore -= this.validateTemperatureHumidityRelationship(reading, issues, recommendations);
    qualityScore -= this.validatePressureConsistency(reading, issues, recommendations);
    qualityScore -= this.validateWindPatterns(reading, issues, recommendations);
    qualityScore -= this.validateTemporalConsistency(reading, issues, recommendations);

    // Data completeness check
    qualityScore -= this.validateDataCompleteness(reading, issues, recommendations);

    return {
      isValid: qualityScore >= 60, // Minimum threshold for submission
      qualityScore: Math.max(0, qualityScore),
      issues,
      recommendations,
    };
  }

  private validateTemperatureHumidityRelationship(
    reading: WeatherReading,
    issues: string[],
    recommendations: string[]
  ): number {
    // Check dew point physics
    const dewPoint = this.calculateDewPoint(reading.temperature, reading.humidity);

    if (dewPoint > reading.temperature) {
      issues.push('Dew point cannot exceed air temperature');
      recommendations.push('Check temperature and humidity sensor calibration');
      return 20;
    }

    // Check for unrealistic combinations
    if (reading.temperature > 35 && reading.humidity > 90) {
      issues.push('Very high temperature with very high humidity is unusual');
      recommendations.push('Verify sensors are not affected by local heat sources');
      return 10;
    }

    return 0;
  }

  private validatePressureConsistency(
    reading: WeatherReading,
    issues: string[],
    recommendations: string[]
  ): number {
    // Sea level pressure adjustment
    const elevation = 104; // From config, meters above sea level
    const expectedSeaLevelPressure = reading.pressure + elevation * 0.12;

    if (expectedSeaLevelPressure < 950 || expectedSeaLevelPressure > 1080) {
      issues.push('Pressure reading outside normal atmospheric range');
      recommendations.push('Check barometric sensor calibration and elevation settings');
      return 15;
    }

    return 0;
  }

  private validateWindPatterns(
    reading: WeatherReading,
    issues: string[],
    recommendations: string[]
  ): number {
    if (!reading.windSpeed || !reading.windDirection) {
      return 0; // Skip if wind data not available
    }

    // Very high wind speeds should be rare
    if (reading.windSpeed > 25) {
      issues.push('Extremely high wind speed detected');
      recommendations.push('Verify anemometer is functioning correctly and not obstructed');
      return 10;
    }

    return 0;
  }

  private validateTemporalConsistency(
    reading: WeatherReading,
    issues: string[],
    recommendations: string[]
  ): number {
    // This would check against recent readings for sudden changes
    // For tutorial purposes, we'll implement basic checks

    const now = new Date();
    const readingTime = new Date(reading.timestamp);
    const timeDiff = Math.abs(now.getTime() - readingTime.getTime());

    if (timeDiff > 5 * 60 * 1000) {
      // More than 5 minutes old
      issues.push('Reading timestamp is too old');
      recommendations.push('Ensure system clock is synchronized and data is submitted promptly');
      return 5;
    }

    return 0;
  }

  private validateDataCompleteness(
    reading: WeatherReading,
    issues: string[],
    recommendations: string[]
  ): number {
    let penalty = 0;

    // Core measurements should always be present
    if (!reading.temperature || !reading.humidity || !reading.pressure) {
      issues.push('Missing core weather measurements');
      recommendations.push('Ensure all primary sensors are connected and functional');
      penalty += 20;
    }

    // Optional measurements improve quality score
    if (!reading.windSpeed) {
      recommendations.push('Add wind measurements to improve data quality and rewards');
      penalty += 5;
    }

    if (!reading.rainfall) {
      recommendations.push('Add rainfall measurements for comprehensive weather data');
      penalty += 3;
    }

    return penalty;
  }

  private calculateDewPoint(temperature: number, humidity: number): number {
    // Magnus formula for dew point calculation
    const a = 17.27;
    const b = 237.7;

    const alpha = (a * temperature) / (b + temperature) + Math.log(humidity / 100);
    return (b * alpha) / (a - alpha);
  }

  // Advanced validation using historical data
  async validateAgainstHistorical(reading: WeatherReading): Promise<number> {
    // This would compare against historical weather patterns
    // For tutorial, return a baseline score
    return 85;
  }

  // Cross-validation with nearby stations
  async validateAgainstNearbyStations(reading: WeatherReading): Promise<number> {
    // This would check consistency with nearby weather stations
    // For tutorial, return a baseline score
    return 90;
  }
}
```

## Submission & Rewards

### 1. Data Submission Service

Create a service to submit validated weather data and earn rewards:

#### JavaScript/TypeScript

```typescript
// src/submissionService.ts
import { sdk, config } from './config';
import { WeatherReading } from './dataCollector';
import { WeatherDataValidator, ValidationResult } from './validator';

export interface SubmissionResult {
  success: boolean;
  transactionId?: string;
  qualityScore: number;
  reward: number;
  submissionTime: string;
  error?: string;
}

export interface SubmissionStats {
  totalSubmissions: number;
  successfulSubmissions: number;
  totalRewards: number;
  averageQualityScore: number;
  lastSubmission: string;
}

export class WeatherSubmissionService {
  private validator: WeatherDataValidator;
  private stats: SubmissionStats;

  constructor() {
    this.validator = new WeatherDataValidator();
    this.stats = {
      totalSubmissions: 0,
      successfulSubmissions: 0,
      totalRewards: 0,
      averageQualityScore: 0,
      lastSubmission: '',
    };
  }

  async submitWeatherData(stationId: string, reading: WeatherReading): Promise<SubmissionResult> {
    try {
      console.log(`🌡️  Submitting weather data from station ${stationId}`);
      console.log(`Temperature: ${reading.temperature.toFixed(1)}°C`);
      console.log(`Humidity: ${reading.humidity.toFixed(1)}%`);
      console.log(`Pressure: ${reading.pressure.toFixed(1)} hPa`);

      // Validate data quality
      const validation = await this.validator.validateReading(reading);

      if (!validation.isValid) {
        console.log('❌ Data validation failed:');
        validation.issues.forEach(issue => console.log(`  - ${issue}`));

        return {
          success: false,
          qualityScore: validation.qualityScore,
          reward: 0,
          submissionTime: new Date().toISOString(),
          error: `Validation failed: ${validation.issues.join(', ')}`,
        };
      }

      if (validation.qualityScore < config.qualityThreshold) {
        console.log(
          `⚠️  Quality score ${validation.qualityScore} below threshold ${config.qualityThreshold}`
        );
        validation.recommendations.forEach(rec => console.log(`  💡 ${rec}`));

        return {
          success: false,
          qualityScore: validation.qualityScore,
          reward: 0,
          submissionTime: new Date().toISOString(),
          error: `Quality score too low: ${validation.qualityScore}`,
        };
      }

      // Submit to blockchain
      const submission = await sdk.weather.submit({
        stationId,
        temperature: reading.temperature,
        humidity: reading.humidity,
        pressure: reading.pressure,
        windSpeed: reading.windSpeed,
        windDirection: reading.windDirection,
        rainfall: reading.rainfall,
        timestamp: reading.timestamp,

        // Include validation metadata
        qualityMetadata: {
          validationScore: validation.qualityScore,
          validationMethod: 'physics-based',
          validationTimestamp: new Date().toISOString(),
        },
      });

      // Update statistics
      this.updateStats(validation.qualityScore, submission.reward);

      console.log('✅ Weather data submitted successfully!');
      console.log(`📊 Quality Score: ${validation.qualityScore}/100`);
      console.log(`💰 Reward: ${submission.reward} WRF`);
      console.log(`🔗 Transaction: ${submission.transactionId}`);

      return {
        success: true,
        transactionId: submission.transactionId,
        qualityScore: validation.qualityScore,
        reward: submission.reward,
        submissionTime: new Date().toISOString(),
      };
    } catch (error) {
      console.error('❌ Submission failed:', error.message);
      this.stats.totalSubmissions++;

      return {
        success: false,
        qualityScore: 0,
        reward: 0,
        submissionTime: new Date().toISOString(),
        error: error.message,
      };
    }
  }

  private updateStats(qualityScore: number, reward: number): void {
    this.stats.totalSubmissions++;
    this.stats.successfulSubmissions++;
    this.stats.totalRewards += reward;

    // Update average quality score
    const prevTotal = this.stats.averageQualityScore * (this.stats.successfulSubmissions - 1);
    this.stats.averageQualityScore = (prevTotal + qualityScore) / this.stats.successfulSubmissions;

    this.stats.lastSubmission = new Date().toISOString();
  }

  getStats(): SubmissionStats {
    return { ...this.stats };
  }

  getSuccessRate(): number {
    if (this.stats.totalSubmissions === 0) return 0;
    return (this.stats.successfulSubmissions / this.stats.totalSubmissions) * 100;
  }

  // Optimize submission timing based on network conditions
  async getOptimalSubmissionTime(): Promise<Date> {
    try {
      // Check network congestion
      const networkStats = await sdk.network.getStats();

      // If network is busy, wait for better time
      if (networkStats.congestionLevel > 0.8) {
        const delay = Math.random() * 60 * 1000; // Random delay up to 1 minute
        return new Date(Date.now() + delay);
      }

      return new Date(); // Submit immediately
    } catch (error) {
      console.warn('Could not check network stats, submitting immediately');
      return new Date();
    }
  }

  // Batch submissions for efficiency
  async submitBatch(stationId: string, readings: WeatherReading[]): Promise<SubmissionResult[]> {
    const results: SubmissionResult[] = [];

    for (const reading of readings) {
      const result = await this.submitWeatherData(stationId, reading);
      results.push(result);

      // Add small delay between submissions to avoid rate limiting
      if (readings.length > 1) {
        await new Promise(resolve => setTimeout(resolve, 1000));
      }
    }

    return results;
  }
}
```

### 2. Automatic Data Collection Loop

Create the main application that continuously collects and submits data:

#### JavaScript/TypeScript

```typescript
// src/app.ts
import { WeatherStation } from './station';
import { WeatherDataCollector } from './dataCollector';
import { WeatherSubmissionService } from './submissionService';
import { config } from './config';

export class WeatherStationApp {
  private station: WeatherStation;
  private collector: WeatherDataCollector;
  private submissionService: WeatherSubmissionService;
  private isRunning: boolean = false;
  private intervalId: NodeJS.Timeout | null = null;

  constructor() {
    this.station = new WeatherStation();
    this.collector = new WeatherDataCollector();
    this.submissionService = new WeatherSubmissionService();
  }

  async start(): Promise<void> {
    console.log('🚀 Starting WRFCoin Weather Station...');

    try {
      // Initialize station
      await this.initializeStation();

      // Start data collection loop
      await this.startDataCollection();

      // Setup graceful shutdown
      this.setupShutdownHandlers();

      console.log('✅ Weather station is running!');
      console.log(`📡 Submitting data every ${config.submissionInterval / 1000} seconds`);
      console.log('Press Ctrl+C to stop');
    } catch (error) {
      console.error('❌ Failed to start weather station:', error.message);
      process.exit(1);
    }
  }

  private async initializeStation(): Promise<void> {
    // Get or register station
    let stationInfo = await this.station.getStationInfo();

    if (!stationInfo) {
      console.log('📝 Registering new weather station...');
      stationInfo = await this.station.register();
    }

    console.log(`📍 Station: ${stationInfo.name} (${stationInfo.id})`);

    // Connect to sensors
    await this.collector.connectToArduino();
  }

  private async startDataCollection(): Promise<void> {
    this.isRunning = true;

    // Immediate first collection
    await this.collectAndSubmitData();

    // Set up interval for regular collection
    this.intervalId = setInterval(async () => {
      if (this.isRunning) {
        await this.collectAndSubmitData();
      }
    }, config.submissionInterval);
  }

  private async collectAndSubmitData(): Promise<void> {
    try {
      const stationId = this.station.getId();
      if (!stationId) {
        throw new Error('Station not initialized');
      }

      console.log(`\n⏰ ${new Date().toLocaleString()} - Collecting weather data...`);

      // Collect sensor reading
      const reading = await this.collector.collectReading();

      // Submit to blockchain
      const result = await this.submissionService.submitWeatherData(stationId, reading);

      if (result.success) {
        const stats = this.submissionService.getStats();
        console.log(`📈 Total earned: ${stats.totalRewards.toFixed(3)} WRF`);
        console.log(`🎯 Success rate: ${this.submissionService.getSuccessRate().toFixed(1)}%`);
      }
    } catch (error) {
      console.error('❌ Data collection error:', error.message);
    }
  }

  async stop(): Promise<void> {
    console.log('\n🛑 Stopping weather station...');

    this.isRunning = false;

    if (this.intervalId) {
      clearInterval(this.intervalId);
      this.intervalId = null;
    }

    // Final statistics
    const stats = this.submissionService.getStats();
    console.log('\n📊 Final Statistics:');
    console.log(`Total submissions: ${stats.totalSubmissions}`);
    console.log(`Successful submissions: ${stats.successfulSubmissions}`);
    console.log(`Total rewards earned: ${stats.totalRewards.toFixed(3)} WRF`);
    console.log(`Average quality score: ${stats.averageQualityScore.toFixed(1)}`);
    console.log(`Success rate: ${this.submissionService.getSuccessRate().toFixed(1)}%`);

    console.log('✅ Weather station stopped');
  }

  private setupShutdownHandlers(): void {
    process.on('SIGINT', async () => {
      await this.stop();
      process.exit(0);
    });

    process.on('SIGTERM', async () => {
      await this.stop();
      process.exit(0);
    });
  }

  // Manual data collection (for testing)
  async collectOnce(): Promise<void> {
    await this.initializeStation();
    await this.collectAndSubmitData();
  }

  // Get live statistics
  getStats() {
    return this.submissionService.getStats();
  }
}

// Main execution
async function main() {
  const app = new WeatherStationApp();
  await app.start();
}

// Run if this file is executed directly
if (require.main === module) {
  main().catch(console.error);
}

export { WeatherStationApp };
```

## Real-Time Dashboard

### 1. Create a Web Dashboard

Build a simple web interface to monitor your weather station:

#### HTML Dashboard

```html
<!-- public/index.html -->
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>WRFCoin Weather Station Dashboard</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <style>
      body {
        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
        margin: 0;
        padding: 20px;
        background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
        min-height: 100vh;
      }

      .container {
        max-width: 1200px;
        margin: 0 auto;
      }

      .header {
        text-align: center;
        color: white;
        margin-bottom: 30px;
      }

      .stats-grid {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
        gap: 20px;
        margin-bottom: 30px;
      }

      .stat-card {
        background: white;
        border-radius: 12px;
        padding: 20px;
        box-shadow: 0 4px 6px rgba(0, 0, 0, 0.1);
        transition: transform 0.2s;
      }

      .stat-card:hover {
        transform: translateY(-2px);
      }

      .stat-value {
        font-size: 2.5em;
        font-weight: bold;
        color: #4f46e5;
        margin-bottom: 5px;
      }

      .stat-label {
        color: #6b7280;
        font-size: 0.9em;
        text-transform: uppercase;
        letter-spacing: 0.5px;
      }

      .chart-container {
        background: white;
        border-radius: 12px;
        padding: 20px;
        box-shadow: 0 4px 6px rgba(0, 0, 0, 0.1);
        margin-bottom: 20px;
      }

      .status {
        display: inline-block;
        padding: 4px 12px;
        border-radius: 20px;
        font-size: 0.8em;
        font-weight: 600;
      }

      .status.active {
        background: #10b981;
        color: white;
      }

      .status.inactive {
        background: #ef4444;
        color: white;
      }

      .refresh-btn {
        background: #4f46e5;
        color: white;
        border: none;
        padding: 10px 20px;
        border-radius: 8px;
        cursor: pointer;
        font-weight: 600;
        margin: 10px;
      }

      .refresh-btn:hover {
        background: #4338ca;
      }
    </style>
  </head>
  <body>
    <div class="container">
      <div class="header">
        <h1>🌡️ WRFCoin Weather Station</h1>
        <p>Real-time climate data collection and blockchain rewards</p>
      </div>

      <div class="stats-grid">
        <div class="stat-card">
          <div class="stat-value" id="temperature">--</div>
          <div class="stat-label">Temperature (°C)</div>
        </div>

        <div class="stat-card">
          <div class="stat-value" id="humidity">--</div>
          <div class="stat-label">Humidity (%)</div>
        </div>

        <div class="stat-card">
          <div class="stat-value" id="pressure">--</div>
          <div class="stat-label">Pressure (hPa)</div>
        </div>

        <div class="stat-card">
          <div class="stat-value" id="totalRewards">--</div>
          <div class="stat-label">Total Rewards (WRF)</div>
        </div>

        <div class="stat-card">
          <div class="stat-value" id="qualityScore">--</div>
          <div class="stat-label">Quality Score</div>
        </div>

        <div class="stat-card">
          <div class="stat-value">
            <span class="status" id="stationStatus">inactive</span>
          </div>
          <div class="stat-label">Station Status</div>
        </div>
      </div>

      <div class="chart-container">
        <h3>Temperature Trend (Last 24 Hours)</h3>
        <canvas id="temperatureChart"></canvas>
      </div>

      <div class="chart-container">
        <h3>Rewards Over Time</h3>
        <canvas id="rewardsChart"></canvas>
      </div>

      <div style="text-align: center;">
        <button class="refresh-btn" onclick="refreshData()">🔄 Refresh Data</button>
        <button class="refresh-btn" onclick="downloadData()">📁 Download Data</button>
      </div>
    </div>

    <script>
      // Chart instances
      let temperatureChart, rewardsChart;

      // Initialize charts
      function initCharts() {
        // Temperature chart
        const tempCtx = document.getElementById('temperatureChart').getContext('2d');
        temperatureChart = new Chart(tempCtx, {
          type: 'line',
          data: {
            labels: [],
            datasets: [
              {
                label: 'Temperature (°C)',
                data: [],
                borderColor: '#ef4444',
                backgroundColor: 'rgba(239, 68, 68, 0.1)',
                tension: 0.4,
              },
            ],
          },
          options: {
            responsive: true,
            scales: {
              y: {
                beginAtZero: false,
              },
            },
          },
        });

        // Rewards chart
        const rewardsCtx = document.getElementById('rewardsChart').getContext('2d');
        rewardsChart = new Chart(rewardsCtx, {
          type: 'bar',
          data: {
            labels: [],
            datasets: [
              {
                label: 'Rewards Earned (WRF)',
                data: [],
                backgroundColor: '#10b981',
                borderColor: '#059669',
                borderWidth: 1,
              },
            ],
          },
          options: {
            responsive: true,
            scales: {
              y: {
                beginAtZero: true,
              },
            },
          },
        });
      }

      // Fetch data from weather station API
      async function fetchWeatherData() {
        try {
          // This would connect to your weather station's API endpoint
          const response = await fetch('/api/weather/current');
          const data = await response.json();
          return data;
        } catch (error) {
          console.error('Failed to fetch weather data:', error);
          return null;
        }
      }

      async function fetchStationStats() {
        try {
          const response = await fetch('/api/station/stats');
          const data = await response.json();
          return data;
        } catch (error) {
          console.error('Failed to fetch station stats:', error);
          return null;
        }
      }

      // Update dashboard with live data
      async function updateDashboard() {
        const weatherData = await fetchWeatherData();
        const stationStats = await fetchStationStats();

        if (weatherData) {
          document.getElementById('temperature').textContent =
            weatherData.temperature?.toFixed(1) || '--';
          document.getElementById('humidity').textContent =
            weatherData.humidity?.toFixed(1) || '--';
          document.getElementById('pressure').textContent =
            weatherData.pressure?.toFixed(1) || '--';
        }

        if (stationStats) {
          document.getElementById('totalRewards').textContent =
            stationStats.totalRewards?.toFixed(3) || '--';
          document.getElementById('qualityScore').textContent =
            stationStats.averageQualityScore?.toFixed(0) || '--';

          const statusElement = document.getElementById('stationStatus');
          statusElement.textContent = stationStats.isActive ? 'active' : 'inactive';
          statusElement.className = `status ${stationStats.isActive ? 'active' : 'inactive'}`;
        }

        // Update charts (mock data for tutorial)
        updateCharts();
      }

      function updateCharts() {
        // Generate mock data for demonstration
        const now = new Date();
        const hours = [];
        const temperatures = [];
        const rewards = [];

        for (let i = 23; i >= 0; i--) {
          const time = new Date(now.getTime() - i * 60 * 60 * 1000);
          hours.push(time.getHours() + ':00');
          temperatures.push(20 + Math.sin((i * Math.PI) / 12) * 10 + Math.random() * 2);
          rewards.push(Math.random() * 0.5);
        }

        // Update temperature chart
        temperatureChart.data.labels = hours;
        temperatureChart.data.datasets[0].data = temperatures;
        temperatureChart.update();

        // Update rewards chart (last 7 days)
        const days = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
        const dailyRewards = days.map(() => Math.random() * 5 + 2);

        rewardsChart.data.labels = days;
        rewardsChart.data.datasets[0].data = dailyRewards;
        rewardsChart.update();
      }

      function refreshData() {
        updateDashboard();
      }

      function downloadData() {
        // Generate CSV data for download
        const csvData = 'timestamp,temperature,humidity,pressure,reward\n';
        // Add your data rows here

        const blob = new Blob([csvData], { type: 'text/csv' });
        const url = window.URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = 'weather_station_data.csv';
        a.click();
        window.URL.revokeObjectURL(url);
      }

      // Initialize dashboard
      document.addEventListener('DOMContentLoaded', function () {
        initCharts();
        updateDashboard();

        // Update every 30 seconds
        setInterval(updateDashboard, 30000);
      });
    </script>
  </body>
</html>
```

### 2. Dashboard API Endpoints

Create API endpoints to serve data to your dashboard:

#### JavaScript/TypeScript

```typescript
// src/dashboardServer.ts
import express from 'express';
import path from 'path';
import { WeatherStationApp } from './app';

export class DashboardServer {
  private app: express.Application;
  private weatherStation: WeatherStationApp;
  private port: number;

  constructor(weatherStation: WeatherStationApp, port: number = 8080) {
    this.app = express();
    this.weatherStation = weatherStation;
    this.port = port;
    this.setupRoutes();
  }

  private setupRoutes(): void {
    // Serve static files
    this.app.use(express.static(path.join(__dirname, '../public')));

    // API endpoints
    this.app.get('/api/weather/current', async (req, res) => {
      try {
        // Get latest weather reading
        const reading = await this.weatherStation['collector'].collectReading();
        res.json(reading);
      } catch (error) {
        res.status(500).json({ error: error.message });
      }
    });

    this.app.get('/api/station/stats', async (req, res) => {
      try {
        const stats = this.weatherStation.getStats();
        res.json({
          ...stats,
          isActive: true, // You can track this based on recent submissions
        });
      } catch (error) {
        res.status(500).json({ error: error.message });
      }
    });

    this.app.get('/api/station/history', async (req, res) => {
      try {
        // This would fetch historical data from your database
        // For tutorial, return mock data
        const history = [];
        const now = new Date();

        for (let i = 0; i < 24; i++) {
          history.push({
            timestamp: new Date(now.getTime() - i * 60 * 60 * 1000).toISOString(),
            temperature: 20 + Math.sin((i * Math.PI) / 12) * 10,
            humidity: 60 + Math.cos((i * Math.PI) / 12) * 20,
            pressure: 1013 + Math.sin((i * Math.PI) / 6) * 10,
            reward: Math.random() * 0.5,
          });
        }

        res.json(history);
      } catch (error) {
        res.status(500).json({ error: error.message });
      }
    });
  }

  start(): void {
    this.app.listen(this.port, () => {
      console.log(`📊 Dashboard server running at http://localhost:${this.port}`);
    });
  }
}
```

## Production Optimization

### 1. Error Handling & Resilience

Add robust error handling and retry mechanisms:

```typescript
// src/resilience.ts
export class ResilientWeatherStation extends WeatherStationApp {
  private maxRetries = 3;
  private retryDelay = 5000; // 5 seconds
  private healthCheckInterval = 60000; // 1 minute

  protected async collectAndSubmitData(): Promise<void> {
    let attempts = 0;

    while (attempts < this.maxRetries) {
      try {
        await super.collectAndSubmitData();
        return; // Success, exit retry loop
      } catch (error) {
        attempts++;
        console.error(`❌ Attempt ${attempts} failed: ${error.message}`);

        if (attempts < this.maxRetries) {
          console.log(`⏳ Retrying in ${this.retryDelay / 1000} seconds...`);
          await this.delay(this.retryDelay);
        }
      }
    }

    console.error(`❌ All ${this.maxRetries} attempts failed`);
  }

  private delay(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  // Health check system
  async performHealthCheck(): Promise<boolean> {
    try {
      // Check sensor connectivity
      await this.collector.collectReading();

      // Check API connectivity
      const status = await sdk.blockchain.getStatus();

      // Check station registration
      const stationInfo = await this.station.getStationInfo();

      return !!stationInfo && !!status;
    } catch (error) {
      console.error('🏥 Health check failed:', error.message);
      return false;
    }
  }
}
```

### 2. Configuration Management

Create a production configuration system:

```typescript
// src/config/production.ts
export const productionConfig = {
  // API Configuration
  api: {
    endpoint: process.env.WRFCOIN_ENDPOINT || 'https://api.wrfcoin.org',
    apiKey: process.env.WRFCOIN_API_KEY,
    network: 'mainnet',
    timeout: 30000,
    retries: 3,
  },

  // Station Configuration
  station: {
    name: process.env.STATION_NAME,
    location: {
      latitude: parseFloat(process.env.STATION_LATITUDE!),
      longitude: parseFloat(process.env.STATION_LONGITUDE!),
      elevation: parseInt(process.env.STATION_ELEVATION || '0'),
    },
    equipment: {
      temperatureSensor: process.env.TEMP_SENSOR_MODEL || 'DS18B20',
      humiditySensor: process.env.HUMIDITY_SENSOR_MODEL || 'DHT22',
      pressureSensor: process.env.PRESSURE_SENSOR_MODEL || 'BMP280',
    },
  },

  // Data Collection
  collection: {
    interval: parseInt(process.env.COLLECTION_INTERVAL || '300') * 1000,
    qualityThreshold: parseInt(process.env.QUALITY_THRESHOLD || '80'),
    batchSize: parseInt(process.env.BATCH_SIZE || '1'),
    validatePhysics: process.env.VALIDATE_PHYSICS !== 'false',
  },

  // Monitoring
  monitoring: {
    healthCheckInterval: 60000,
    logLevel: process.env.LOG_LEVEL || 'info',
    metricsEnabled: process.env.METRICS_ENABLED === 'true',
    dashboardPort: parseInt(process.env.DASHBOARD_PORT || '8080'),
  },

  // Security
  security: {
    enableEncryption: process.env.ENABLE_ENCRYPTION === 'true',
    rateLimitEnabled: process.env.RATE_LIMIT_ENABLED !== 'false',
    maxRequestsPerMinute: parseInt(process.env.MAX_REQUESTS_PER_MINUTE || '60'),
  },
};
```

## Troubleshooting

### Common Issues and Solutions

#### 1. **Low Quality Scores**

**Problem**: Your weather data consistently receives low quality scores.

**Solutions**:

- Check sensor calibration dates
- Verify sensors are properly shielded from direct sunlight/rain
- Ensure consistent submission intervals
- Compare readings with nearby weather stations
- Check for sensor drift over time

#### 2. **Submission Failures**

**Problem**: Weather data submissions are failing.

**Solutions**:

- Verify API key is valid and has correct permissions
- Check network connectivity to WRFCoin endpoints
- Ensure system clock is synchronized
- Monitor for rate limiting
- Implement exponential backoff for retries

#### 3. **Sensor Reading Errors**

**Problem**: Sensors are returning invalid or inconsistent readings.

**Solutions**:

- Check physical connections to sensors
- Verify power supply stability
- Test sensors individually
- Implement sensor-specific error handling
- Add redundant sensors for critical measurements

#### 4. **Network Connectivity Issues**

**Problem**: Intermittent network failures affecting submissions.

**Solutions**:

- Implement local data storage and batch submissions
- Add network health monitoring
- Use multiple network interfaces if available
- Implement offline mode with data queuing

### Debug Commands

Add these debug functions to help troubleshoot:

```typescript
// src/debug.ts
export class WeatherStationDebugger {
  constructor(private station: WeatherStationApp) {}

  async runDiagnostics(): Promise<void> {
    console.log('🔍 Running station diagnostics...\n');

    // Test sensor readings
    await this.testSensors();

    // Test API connectivity
    await this.testAPIConnectivity();

    // Test data validation
    await this.testDataValidation();

    // Test submission process
    await this.testSubmission();
  }

  private async testSensors(): Promise<void> {
    console.log('📡 Testing sensors...');
    try {
      const reading = await this.station['collector'].collectReading();
      console.log('✅ Sensor test passed');
      console.log(`  Temperature: ${reading.temperature.toFixed(1)}°C`);
      console.log(`  Humidity: ${reading.humidity.toFixed(1)}%`);
      console.log(`  Pressure: ${reading.pressure.toFixed(1)} hPa`);
    } catch (error) {
      console.log('❌ Sensor test failed:', error.message);
    }
    console.log();
  }

  private async testAPIConnectivity(): Promise<void> {
    console.log('🌐 Testing API connectivity...');
    try {
      const status = await sdk.blockchain.getStatus();
      console.log('✅ API connectivity test passed');
      console.log(`  Network: ${status.network}`);
      console.log(`  Block Height: ${status.blockHeight}`);
    } catch (error) {
      console.log('❌ API connectivity test failed:', error.message);
    }
    console.log();
  }

  // Add more diagnostic methods...
}
```

### Production Deployment Checklist

- [ ] API key configured and tested
- [ ] Sensors connected and calibrated
- [ ] Network connectivity verified
- [ ] Station registered with WRFCoin
- [ ] Data validation thresholds set
- [ ] Error handling and retry logic implemented
- [ ] Monitoring and alerting configured
- [ ] Backup power supply for sensors
- [ ] Regular maintenance schedule planned
- [ ] Documentation updated for operations team

## Next Steps

Congratulations! You've built a complete weather station integration with WRFCoin. Here are some
next steps to consider:

### 1. **Add More Sensors**

- Solar radiation sensor
- UV index sensor
- Air quality sensors (PM2.5, PM10)
- Soil temperature and moisture

### 2. **Implement Advanced Features**

- Weather forecasting models
- Data analytics and insights
- Mobile app notifications
- Integration with smart home systems

### 3. **Optimize for Rewards**

- Study weather patterns to predict high-value data periods
- Implement multiple stations for redundancy
- Participate in research data programs
- Contribute to climate modeling projects

### 4. **Scale Your Operation**

- Deploy multiple weather stations
- Create a weather station network
- Offer weather data services
- Partner with agricultural or research organizations

### Resources

- **WRFCoin Developer Portal**: [localhost:5000/developer](http://localhost:5000/developer)
- **API Documentation**: Complete reference for all endpoints
- **Community Forum**: Connect with other weather station operators
- **Technical Support**: Help with integration issues

---

**🎉 Congratulations!** You've successfully integrated a weather station with WRFCoin and are now
earning rewards for contributing valuable climate data to the blockchain ecosystem!
