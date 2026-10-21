# Air Quality Monitoring Integration

Advanced guide to integrating air quality monitoring devices with WRFCoin's environmental data
network for comprehensive atmospheric monitoring.

## What You'll Learn

- Air quality sensor integration and calibration
- Real-time AQI calculation and reporting
- Multi-pollutant monitoring strategies
- Data validation and quality assurance
- Alert systems for pollution events
- Integration with climate data networks

## Prerequisites

- Completed [Weather Station Setup](./01-weather-station-setup.md)
- Understanding of air quality parameters and standards
- Basic knowledge of atmospheric chemistry
- Experience with environmental sensor networks

---

## 1. Air Quality Sensor Overview

### Key Pollutant Parameters

```yaml
# air-quality-spec.yml
pollutant_parameters:
  particulate_matter:
    pm25:
      name: 'PM2.5'
      description: 'Fine particulate matter ≤2.5μm'
      health_concern: 'Respiratory and cardiovascular effects'
      unit: 'μg/m³'
      measurement_range: '0-500'
      accuracy: '±10% at 25μg/m³'

    pm10:
      name: 'PM10'
      description: 'Coarse particulate matter ≤10μm'
      health_concern: 'Respiratory irritation'
      unit: 'μg/m³'
      measurement_range: '0-1000'
      accuracy: '±15% at 50μg/m³'

  gaseous_pollutants:
    ozone:
      name: 'Ground-level Ozone (O₃)'
      description: 'Secondary pollutant from NOx and VOCs'
      health_concern: 'Respiratory irritation, asthma'
      unit: 'ppb'
      measurement_range: '0-300'
      accuracy: '±5% at 50ppb'

    nitrogen_dioxide:
      name: 'Nitrogen Dioxide (NO₂)'
      description: 'Traffic and industrial emissions'
      health_concern: 'Respiratory inflammation'
      unit: 'ppb'
      measurement_range: '0-200'
      accuracy: '±10% at 50ppb'

    sulfur_dioxide:
      name: 'Sulfur Dioxide (SO₂)'
      description: 'Industrial and power plant emissions'
      health_concern: 'Respiratory irritation'
      unit: 'ppb'
      measurement_range: '0-100'
      accuracy: '±15% at 20ppb'

    carbon_monoxide:
      name: 'Carbon Monoxide (CO)'
      description: 'Incomplete combustion product'
      health_concern: 'Reduces oxygen carrying capacity'
      unit: 'ppm'
      measurement_range: '0-50'
      accuracy: '±5% at 10ppm'

  air_quality_index:
    scale: '0-500'
    categories:
      good: '0-50 (Green)'
      moderate: '51-100 (Yellow)'
      unhealthy_sensitive: '101-150 (Orange)'
      unhealthy: '151-200 (Red)'
      very_unhealthy: '201-300 (Purple)'
      hazardous: '301-500 (Maroon)'
```

### Sensor Hardware Requirements

```javascript
// hardware/air-quality-sensors.js
const airQualitySensors = {
  particulate_matter: {
    professional: {
      models: ['Sensirion SPS30', 'Plantower PMS7003', 'Honeywell HPMA115S0'],
      features: ['Laser scattering', 'Size distribution', 'Number concentration'],
      accuracy: 'High (±10%)',
      cost: 'Medium ($50-200)',
      lifespan: '5+ years',
    },
    research_grade: {
      models: ['TSI DustTrak DRX', 'MetOne BAM-1020', 'Thermo Fisher FH62C14'],
      features: ['Gravimetric correlation', 'Temperature compensation', 'Data logging'],
      accuracy: 'Very High (±5%)',
      cost: 'High ($5,000-50,000)',
      lifespan: '10+ years',
    },
  },

  gas_sensors: {
    electrochemical: {
      models: ['Alphasense B4 Series', 'Sensirion SGP40', 'Bosch BME688'],
      pollutants: ['NO₂', 'O₃', 'CO', 'SO₂'],
      accuracy: 'Good (±15%)',
      cost: 'Low ($10-100)',
      lifespan: '2-3 years',
    },
    optical: {
      models: ['Aeroqual SM50', '2B Technologies Model 202', 'Ecotech Serinus 10'],
      pollutants: ['O₃', 'NO₂', 'SO₂'],
      accuracy: 'High (±5%)',
      cost: 'Very High ($10,000+)',
      lifespan: '10+ years',
    },
  },

  multi_parameter: {
    integrated_units: {
      models: ['PurpleAir PA-II', 'AirGradient DIY', 'Clarity Node-S'],
      parameters: ['PM2.5', 'PM10', 'Temperature', 'Humidity', 'CO₂'],
      connectivity: ['WiFi', 'Cellular', 'LoRaWAN'],
      cost: 'Medium ($200-1000)',
      deployment: 'Plug-and-play',
    },
  },
};

module.exports = airQualitySensors;
```

---

## 2. Air Quality Monitor Implementation

### Multi-Sensor Air Quality Device

```javascript
// devices/air-quality-monitor.js
const EventEmitter = require('events');

class AirQualityMonitor extends EventEmitter {
  constructor(config) {
    super();
    this.config = config;
    this.sensors = new Map();
    this.calibrationData = new Map();
    this.currentReadings = new Map();
    this.aqiCalculator = new AQICalculator();
    this.isRunning = false;
  }

  async initialize() {
    console.log('🌬️  Initializing air quality monitor...');

    try {
      // Initialize PM sensors
      if (this.config.sensors.pm25?.enabled) {
        await this._initializePMSensor();
      }

      // Initialize gas sensors
      if (this.config.sensors.gases?.enabled) {
        await this._initializeGasSensors();
      }

      // Initialize environmental sensors (for compensation)
      if (this.config.sensors.environmental?.enabled) {
        await this._initializeEnvironmentalSensors();
      }

      // Load calibration data
      await this._loadCalibrationData();

      console.log(`✅ Air quality monitor initialized with ${this.sensors.size} sensors`);
    } catch (error) {
      console.error('❌ Air quality monitor initialization failed:', error);
      throw error;
    }
  }

  async _initializePMSensor() {
    const PMSensor = require('./sensors/pm-sensor');
    const pmConfig = this.config.sensors.pm25;

    const pmSensor = new PMSensor({
      model: pmConfig.model,
      port: pmConfig.port,
      baudRate: pmConfig.baudRate || 9600,
      warmupTime: pmConfig.warmupTime || 30000, // 30 seconds
    });

    await pmSensor.initialize();
    this.sensors.set('pm', pmSensor);

    console.log(`📊 PM sensor initialized: ${pmConfig.model}`);
  }

  async _initializeGasSensors() {
    const GasSensorArray = require('./sensors/gas-sensor-array');
    const gasConfig = this.config.sensors.gases;

    const gasSensors = new GasSensorArray({
      sensors: gasConfig.types, // ['NO2', 'O3', 'CO', 'SO2']
      i2cBus: gasConfig.i2cBus || 1,
      compensationEnabled: true,
    });

    await gasSensors.initialize();
    this.sensors.set('gases', gasSensors);

    console.log(`🧪 Gas sensor array initialized: ${gasConfig.types.join(', ')}`);
  }

  async _initializeEnvironmentalSensors() {
    const EnvironmentalSensor = require('./sensors/environmental-sensor');
    const envConfig = this.config.sensors.environmental;

    const envSensor = new EnvironmentalSensor({
      model: envConfig.model,
      i2cAddress: envConfig.i2cAddress || 0x76,
    });

    await envSensor.initialize();
    this.sensors.set('environmental', envSensor);

    console.log('🌡️  Environmental sensor initialized for compensation');
  }

  async start() {
    if (this.isRunning) {
      console.warn('⚠️  Air quality monitor already running');
      return;
    }

    console.log('🚀 Starting air quality monitoring...');
    this.isRunning = true;

    // Start continuous monitoring
    this._startContinuousMonitoring();

    // Start data submission
    this._startDataSubmission();

    // Start health monitoring
    this._startHealthMonitoring();

    this.emit('monitoring-started');
  }

  stop() {
    if (!this.isRunning) {
      return;
    }

    console.log('🛑 Stopping air quality monitoring...');
    this.isRunning = false;

    // Clear intervals would go here

    this.emit('monitoring-stopped');
  }

  _startContinuousMonitoring() {
    const monitoringInterval = this.config.monitoring?.interval || 60000; // 1 minute default

    setInterval(async () => {
      if (!this.isRunning) return;

      try {
        await this._collectAllReadings();
      } catch (error) {
        console.error('❌ Monitoring cycle error:', error);
        this.emit('monitoring-error', error);
      }
    }, monitoringInterval);

    console.log(`📊 Continuous monitoring started (${monitoringInterval}ms interval)`);
  }

  async _collectAllReadings() {
    const timestamp = new Date().toISOString();
    const readings = {
      timestamp,
      station_id: this.config.station_id,
      location: this.config.location,
    };

    try {
      // Collect PM readings
      if (this.sensors.has('pm')) {
        readings.particulate_matter = await this._readPMData();
      }

      // Collect gas readings
      if (this.sensors.has('gases')) {
        readings.gases = await this._readGasData();
      }

      // Collect environmental data for compensation
      if (this.sensors.has('environmental')) {
        readings.environmental = await this._readEnvironmentalData();
      }

      // Apply calibration and compensation
      const calibratedReadings = await this._applyCalibrationAndCompensation(readings);

      // Calculate AQI
      calibratedReadings.aqi = this.aqiCalculator.calculateCompositeAQI(calibratedReadings);

      // Validate data quality
      calibratedReadings.quality = this._assessDataQuality(calibratedReadings);

      // Store current readings
      this.currentReadings = calibratedReadings;

      // Emit data event
      this.emit('data-collected', calibratedReadings);

      // Check for alerts
      this._checkAlertConditions(calibratedReadings);

      console.log(`📊 AQI: ${calibratedReadings.aqi.value} (${calibratedReadings.aqi.category})`);
    } catch (error) {
      console.error('❌ Reading collection failed:', error);
      this.emit('collection-error', error);
    }
  }

  async _readPMData() {
    const pmSensor = this.sensors.get('pm');
    const rawData = await pmSensor.read();

    return {
      pm1_0: rawData.pm1_0,
      pm2_5: rawData.pm2_5,
      pm10: rawData.pm10,
      particle_counts: {
        pm0_3: rawData.particles_03um,
        pm0_5: rawData.particles_05um,
        pm1_0: rawData.particles_10um,
        pm2_5: rawData.particles_25um,
        pm5_0: rawData.particles_50um,
        pm10: rawData.particles_100um,
      },
      sensor_status: rawData.status,
    };
  }

  async _readGasData() {
    const gasSensors = this.sensors.get('gases');
    const rawData = await gasSensors.readAll();

    return {
      no2: rawData.NO2, // ppb
      o3: rawData.O3, // ppb
      co: rawData.CO, // ppm
      so2: rawData.SO2, // ppb
      sensor_temperatures: rawData.temperatures,
      sensor_status: rawData.status,
    };
  }

  async _readEnvironmentalData() {
    const envSensor = this.sensors.get('environmental');
    const rawData = await envSensor.read();

    return {
      temperature: rawData.temperature, // °C
      humidity: rawData.humidity, // %
      pressure: rawData.pressure, // hPa
      dewpoint: this._calculateDewpoint(rawData.temperature, rawData.humidity),
    };
  }

  async _applyCalibrationAndCompensation(readings) {
    const calibrated = JSON.parse(JSON.stringify(readings)); // Deep copy

    // Apply PM calibration
    if (readings.particulate_matter) {
      calibrated.particulate_matter = await this._calibratePMReadings(readings.particulate_matter);
    }

    // Apply gas sensor calibration and compensation
    if (readings.gases && readings.environmental) {
      calibrated.gases = await this._calibrateGasReadings(readings.gases, readings.environmental);
    }

    return calibrated;
  }

  async _calibratePMReadings(pmData) {
    const calibration = this.calibrationData.get('pm') || { pm2_5: {}, pm10: {} };

    // Apply humidity compensation for PM readings
    const envData = this.currentReadings?.environmental;
    let humidityFactor = 1.0;

    if (envData && envData.humidity > 60) {
      // Hygroscopic growth correction
      humidityFactor = 1.0 + (envData.humidity - 60) * 0.01;
    }

    return {
      pm1_0: this._applyCalibration(pmData.pm1_0, calibration.pm1_0) / humidityFactor,
      pm2_5: this._applyCalibration(pmData.pm2_5, calibration.pm2_5) / humidityFactor,
      pm10: this._applyCalibration(pmData.pm10, calibration.pm10) / humidityFactor,
      particle_counts: pmData.particle_counts,
      sensor_status: pmData.sensor_status,
    };
  }

  async _calibrateGasReadings(gasData, envData) {
    const calibrated = {};

    for (const [gas, value] of Object.entries(gasData)) {
      if (typeof value !== 'number') {
        calibrated[gas] = value;
        continue;
      }

      const calibration = this.calibrationData.get(`gas_${gas}`) || {};

      // Apply temperature and humidity compensation
      let compensatedValue = value;

      // Temperature compensation (typical for electrochemical sensors)
      if (calibration.temperatureCoeff && envData.temperature) {
        const tempDiff = envData.temperature - 20; // Reference: 20°C
        compensatedValue *= 1 + calibration.temperatureCoeff * tempDiff;
      }

      // Humidity compensation (for some gas sensors)
      if (calibration.humidityCoeff && envData.humidity) {
        const humidityDiff = envData.humidity - 50; // Reference: 50% RH
        compensatedValue *= 1 + calibration.humidityCoeff * humidityDiff;
      }

      // Apply calibration
      calibrated[gas] = this._applyCalibration(compensatedValue, calibration);
    }

    return calibrated;
  }

  _applyCalibration(value, calibration) {
    if (!calibration || (!calibration.slope && !calibration.offset)) {
      return value;
    }

    const slope = calibration.slope || 1.0;
    const offset = calibration.offset || 0.0;

    return value * slope + offset;
  }

  _assessDataQuality(readings) {
    const quality = {
      score: 100,
      flags: [],
      confidence: 'high',
    };

    // Check sensor status
    if (readings.particulate_matter?.sensor_status !== 'ok') {
      quality.flags.push('pm_sensor_error');
      quality.score -= 30;
    }

    if (readings.gases?.sensor_status !== 'ok') {
      quality.flags.push('gas_sensor_error');
      quality.score -= 20;
    }

    // Check for extreme values
    if (readings.particulate_matter?.pm2_5 > 500) {
      quality.flags.push('extreme_pm25');
      quality.score -= 20;
    }

    if (readings.gases?.no2 > 200) {
      quality.flags.push('extreme_no2');
      quality.score -= 15;
    }

    // Check environmental conditions
    if (readings.environmental) {
      if (readings.environmental.humidity > 95) {
        quality.flags.push('high_humidity');
        quality.score -= 10;
      }

      if (readings.environmental.temperature < -10 || readings.environmental.temperature > 50) {
        quality.flags.push('extreme_temperature');
        quality.score -= 15;
      }
    }

    // Determine confidence level
    if (quality.score >= 90) {
      quality.confidence = 'high';
    } else if (quality.score >= 70) {
      quality.confidence = 'medium';
    } else {
      quality.confidence = 'low';
    }

    return quality;
  }

  _checkAlertConditions(readings) {
    const alerts = [];

    // AQI-based alerts
    if (readings.aqi) {
      if (readings.aqi.value >= 151) {
        // Unhealthy
        alerts.push({
          type: 'aqi_unhealthy',
          severity: 'high',
          value: readings.aqi.value,
          category: readings.aqi.category,
          message: `Air quality is unhealthy: AQI ${readings.aqi.value}`,
        });
      } else if (readings.aqi.value >= 101) {
        // Unhealthy for sensitive groups
        alerts.push({
          type: 'aqi_sensitive',
          severity: 'medium',
          value: readings.aqi.value,
          category: readings.aqi.category,
          message: `Air quality unhealthy for sensitive groups: AQI ${readings.aqi.value}`,
        });
      }
    }

    // Pollutant-specific alerts
    if (readings.particulate_matter?.pm2_5 >= 55.5) {
      // Unhealthy for sensitive groups
      alerts.push({
        type: 'pm25_high',
        severity: readings.particulate_matter.pm2_5 >= 150.5 ? 'high' : 'medium',
        value: readings.particulate_matter.pm2_5,
        unit: 'μg/m³',
        message: `High PM2.5 levels detected: ${readings.particulate_matter.pm2_5} μg/m³`,
      });
    }

    if (readings.gases?.o3 >= 101) {
      // Unhealthy for sensitive groups
      alerts.push({
        type: 'ozone_high',
        severity: readings.gases.o3 >= 165 ? 'high' : 'medium',
        value: readings.gases.o3,
        unit: 'ppb',
        message: `High ozone levels detected: ${readings.gases.o3} ppb`,
      });
    }

    // Emit alerts
    if (alerts.length > 0) {
      this.emit('air-quality-alert', {
        timestamp: new Date().toISOString(),
        station_id: this.config.station_id,
        alerts,
      });

      for (const alert of alerts) {
        console.warn(`⚠️  ${alert.message}`);
      }
    }
  }

  _calculateDewpoint(temperature, humidity) {
    // Magnus formula approximation
    const a = 17.27;
    const b = 237.7;

    const alpha = (a * temperature) / (b + temperature) + Math.log(humidity / 100);
    const dewpoint = (b * alpha) / (a - alpha);

    return Math.round(dewpoint * 10) / 10;
  }

  async _loadCalibrationData() {
    try {
      const fs = require('fs').promises;
      const path = require('path');

      const calibrationFile = path.join(
        process.env.HOME || '/home/pi',
        '.wrfcoin',
        'air-quality-calibration.json'
      );

      const data = await fs.readFile(calibrationFile, 'utf8');
      const calibrationData = JSON.parse(data);

      for (const [sensor, cal] of Object.entries(calibrationData)) {
        this.calibrationData.set(sensor, cal);
      }

      console.log('📊 Calibration data loaded');
    } catch (error) {
      console.warn('⚠️  No calibration data found, using defaults');
    }
  }

  getCurrentReadings() {
    return this.currentReadings;
  }

  getStatus() {
    return {
      running: this.isRunning,
      sensors: this.sensors.size,
      last_reading: this.currentReadings?.timestamp,
      aqi: this.currentReadings?.aqi?.value,
      data_quality: this.currentReadings?.quality?.confidence,
    };
  }
}

// AQI Calculator Implementation
class AQICalculator {
  constructor() {
    this.breakpoints = {
      pm2_5: [
        { low: 0.0, high: 12.0, aqiLow: 0, aqiHigh: 50 },
        { low: 12.1, high: 35.4, aqiLow: 51, aqiHigh: 100 },
        { low: 35.5, high: 55.4, aqiLow: 101, aqiHigh: 150 },
        { low: 55.5, high: 150.4, aqiLow: 151, aqiHigh: 200 },
        { low: 150.5, high: 250.4, aqiLow: 201, aqiHigh: 300 },
        { low: 250.5, high: 350.4, aqiLow: 301, aqiHigh: 400 },
        { low: 350.5, high: 500.4, aqiLow: 401, aqiHigh: 500 },
      ],
      pm10: [
        { low: 0, high: 54, aqiLow: 0, aqiHigh: 50 },
        { low: 55, high: 154, aqiLow: 51, aqiHigh: 100 },
        { low: 155, high: 254, aqiLow: 101, aqiHigh: 150 },
        { low: 255, high: 354, aqiLow: 151, aqiHigh: 200 },
        { low: 355, high: 424, aqiLow: 201, aqiHigh: 300 },
        { low: 425, high: 504, aqiLow: 301, aqiHigh: 400 },
        { low: 505, high: 604, aqiLow: 401, aqiHigh: 500 },
      ],
      o3: [
        { low: 0, high: 54, aqiLow: 0, aqiHigh: 50 },
        { low: 55, high: 70, aqiLow: 51, aqiHigh: 100 },
        { low: 71, high: 85, aqiLow: 101, aqiHigh: 150 },
        { low: 86, high: 105, aqiLow: 151, aqiHigh: 200 },
        { low: 106, high: 200, aqiLow: 201, aqiHigh: 300 },
      ],
      no2: [
        { low: 0, high: 53, aqiLow: 0, aqiHigh: 50 },
        { low: 54, high: 100, aqiLow: 51, aqiHigh: 100 },
        { low: 101, high: 360, aqiLow: 101, aqiHigh: 150 },
        { low: 361, high: 649, aqiLow: 151, aqiHigh: 200 },
        { low: 650, high: 1249, aqiLow: 201, aqiHigh: 300 },
        { low: 1250, high: 1649, aqiLow: 301, aqiHigh: 400 },
        { low: 1650, high: 2049, aqiLow: 401, aqiHigh: 500 },
      ],
    };
  }

  calculateAQI(pollutant, concentration) {
    const breakpoints = this.breakpoints[pollutant];
    if (!breakpoints) {
      throw new Error(`Unknown pollutant: ${pollutant}`);
    }

    // Find the appropriate breakpoint
    for (const bp of breakpoints) {
      if (concentration >= bp.low && concentration <= bp.high) {
        // Linear interpolation
        const aqi =
          ((bp.aqiHigh - bp.aqiLow) / (bp.high - bp.low)) * (concentration - bp.low) + bp.aqiLow;
        return Math.round(aqi);
      }
    }

    // Handle values above the highest breakpoint
    const highest = breakpoints[breakpoints.length - 1];
    if (concentration > highest.high) {
      return 500; // Hazardous max
    }

    return 0; // Below lowest breakpoint
  }

  calculateCompositeAQI(readings) {
    const aqiValues = [];
    const pollutantAQIs = {};

    // Calculate AQI for each available pollutant
    if (readings.particulate_matter?.pm2_5 !== undefined) {
      const pm25AQI = this.calculateAQI('pm2_5', readings.particulate_matter.pm2_5);
      aqiValues.push(pm25AQI);
      pollutantAQIs.pm2_5 = pm25AQI;
    }

    if (readings.particulate_matter?.pm10 !== undefined) {
      const pm10AQI = this.calculateAQI('pm10', readings.particulate_matter.pm10);
      aqiValues.push(pm10AQI);
      pollutantAQIs.pm10 = pm10AQI;
    }

    if (readings.gases?.o3 !== undefined) {
      const o3AQI = this.calculateAQI('o3', readings.gases.o3);
      aqiValues.push(o3AQI);
      pollutantAQIs.o3 = o3AQI;
    }

    if (readings.gases?.no2 !== undefined) {
      const no2AQI = this.calculateAQI('no2', readings.gases.no2);
      aqiValues.push(no2AQI);
      pollutantAQIs.no2 = no2AQI;
    }

    // The overall AQI is the maximum of individual pollutant AQIs
    const maxAQI = Math.max(...aqiValues);
    const category = this.getAQICategory(maxAQI);
    const primaryPollutant = this.getPrimaryPollutant(pollutantAQIs, maxAQI);

    return {
      value: maxAQI,
      category,
      primary_pollutant: primaryPollutant,
      pollutant_aqis: pollutantAQIs,
      health_message: this.getHealthMessage(maxAQI),
      color: this.getAQIColor(maxAQI),
    };
  }

  getAQICategory(aqi) {
    if (aqi <= 50) return 'Good';
    if (aqi <= 100) return 'Moderate';
    if (aqi <= 150) return 'Unhealthy for Sensitive Groups';
    if (aqi <= 200) return 'Unhealthy';
    if (aqi <= 300) return 'Very Unhealthy';
    return 'Hazardous';
  }

  getPrimaryPollutant(pollutantAQIs, maxAQI) {
    for (const [pollutant, aqi] of Object.entries(pollutantAQIs)) {
      if (aqi === maxAQI) {
        return pollutant;
      }
    }
    return 'unknown';
  }

  getHealthMessage(aqi) {
    if (aqi <= 50) {
      return 'Air quality is satisfactory and poses little or no health risk.';
    } else if (aqi <= 100) {
      return 'Air quality is acceptable. Sensitive individuals may experience minor symptoms.';
    } else if (aqi <= 150) {
      return 'Sensitive groups may experience health effects. General public unlikely to be affected.';
    } else if (aqi <= 200) {
      return 'Everyone may experience health effects. Sensitive groups may experience serious effects.';
    } else if (aqi <= 300) {
      return 'Health alert: everyone may experience serious health effects.';
    } else {
      return 'Health warning: emergency conditions affecting entire population.';
    }
  }

  getAQIColor(aqi) {
    if (aqi <= 50) return '#00E400'; // Green
    if (aqi <= 100) return '#FFFF00'; // Yellow
    if (aqi <= 150) return '#FF7E00'; // Orange
    if (aqi <= 200) return '#FF0000'; // Red
    if (aqi <= 300) return '#8F3F97'; // Purple
    return '#7E0023'; // Maroon
  }
}

module.exports = { AirQualityMonitor, AQICalculator };
```

This comprehensive air quality monitoring integration provides professional-grade atmospheric
monitoring capabilities with real-time AQI calculations, multi-pollutant tracking, and automated
alert systems for the WRFCoin environmental network.
