# Weather Station Integration Guide

Complete guide to integrating IoT weather stations with the WRFCoin network for automated data
collection and blockchain rewards.

## What You'll Learn

- Hardware setup and sensor calibration
- Weather station registration on WRFCoin network
- Automated data collection and transmission
- Data validation and quality assurance
- Reward distribution and monitoring
- Troubleshooting and maintenance

## Prerequisites

- Completed [SDK Integration tutorials](../sdk-integration/)
- Basic understanding of IoT devices and sensors
- Familiarity with hardware setup and networking
- Access to compatible weather station hardware

---

## 1. Hardware Requirements

### Recommended Weather Station Components

```yaml
# hardware-spec.yml
weather_station:
  minimum_sensors:
    - temperature: '±0.1°C accuracy'
    - humidity: '±2% RH accuracy'
    - pressure: '±0.1 hPa accuracy'
    - wind_speed: '±0.1 m/s accuracy'
    - wind_direction: '±3° accuracy'
    - precipitation: '0.1mm resolution'

  optional_sensors:
    - uv_index: 'UV sensor'
    - solar_radiation: 'Pyranometer'
    - soil_temperature: 'Multiple depth sensors'
    - soil_moisture: 'Multiple depth sensors'
    - air_quality: 'PM2.5, PM10, O3, NO2, SO2, CO'
    - visibility: 'Optical sensor'
    - lightning: 'Lightning detector'

  connectivity:
    primary: 'WiFi 802.11 b/g/n'
    backup: 'Cellular (4G/5G)'
    local: 'Ethernet (optional)'

  power:
    main: 'Solar panel + battery backup'
    backup: 'AC adapter'
    consumption: '<5W average'

  housing:
    rating: 'IP65 or higher'
    temperature_range: '-40°C to +60°C'
    humidity_range: '0-100% RH'
    wind_resistance: 'Up to 50 m/s'

  computing:
    processor: 'ARM Cortex-A series or equivalent'
    memory: 'Minimum 1GB RAM'
    storage: 'Minimum 8GB eMMC'
    os: 'Linux-based (Raspberry Pi OS, Ubuntu)'
```

### Compatible Hardware Platforms

```javascript
// hardware/supported-platforms.js
const supportedPlatforms = {
  raspberryPi: {
    models: ['Pi 4B', 'Pi 3B+', 'Pi Zero 2W'],
    os: 'Raspberry Pi OS',
    sensors: {
      temperature: 'DS18B20, DHT22, SHT30',
      humidity: 'DHT22, SHT30, BME280',
      pressure: 'BME280, BMP388',
      wind: 'Davis Anemometer, Inspeed Vortex',
      rain: 'Davis Rain Collector, Hydreon RG-11',
      airQuality: 'PMS5003, SDS011, BME680',
    },
  },

  arduino: {
    models: ['ESP32', 'ESP8266', 'Arduino MKR WiFi 1010'],
    sensors: {
      temperature: 'DHT22, DS18B20',
      humidity: 'DHT22, SHT30',
      pressure: 'BME280',
      basic: 'Entry-level stations',
    },
  },

  commercial: {
    davis: {
      models: ['Vantage Vue', 'Vantage Pro2'],
      integration: 'Serial/USB interface',
      accuracy: 'Professional grade',
    },
    lacrosse: {
      models: ['WS-2902', 'WS-1001-WIF'],
      integration: 'WiFi/API',
      costEffective: true,
    },
    ambient: {
      models: ['WS-2902A', 'WS-1002-WIFI'],
      integration: 'Direct API',
      cloudSupport: true,
    },
  },
};

module.exports = supportedPlatforms;
```

---

## 2. Software Installation and Setup

### WRFCoin IoT SDK Installation

```bash
# Install WRFCoin IoT SDK
npm install -g @wrfcoin/iot-station
# or
pip install wrfcoin-iot-station

# For Raspberry Pi
curl -sSL https://get.wrfcoin.network/iot-setup.sh | bash

# Verify installation
wrfcoin-station --version
```

### Environment Configuration

```bash
# /etc/wrfcoin/station.conf
[network]
endpoint = https://api.wrfcoin.network
network = mainnet
api_key = YOUR_API_KEY_HERE
api_secret = YOUR_API_SECRET_HERE

[station]
station_id = WEATHER_STATION_001
name = "My Weather Station"
location_lat = 40.7128
location_lon = -122.4194
elevation = 16
timezone = America/Los_Angeles

[sensors]
enabled = temperature,humidity,pressure,wind_speed,wind_direction,precipitation
poll_interval = 60  # seconds
validation_enabled = true
quality_checks = true

[data]
submission_interval = 300  # 5 minutes
batch_size = 5
offline_storage = /var/lib/wrfcoin/offline_data
max_offline_days = 7

[logging]
level = INFO
file = /var/log/wrfcoin/station.log
max_size = 100MB
backup_count = 5
```

### Hardware Detection and Calibration

```javascript
// setup/hardware-detection.js
const { IoTDeviceManager, SensorCalibrator } = require('@wrfcoin/iot-station');

class WeatherStationSetup {
  constructor() {
    this.deviceManager = new IoTDeviceManager();
    this.calibrator = new SensorCalibrator();
    this.detectedSensors = new Map();
  }

  async detectHardware() {
    console.log('🔍 Detecting connected sensors...');

    const sensors = await this.deviceManager.scanForSensors();

    for (const sensor of sensors) {
      console.log(`📡 Found ${sensor.type}: ${sensor.model} (${sensor.address})`);

      // Test sensor communication
      try {
        const testReading = await sensor.read();
        console.log(`✅ ${sensor.type} test reading: ${JSON.stringify(testReading)}`);
        this.detectedSensors.set(sensor.type, sensor);
      } catch (error) {
        console.error(`❌ ${sensor.type} communication error:`, error.message);
      }
    }

    return this.detectedSensors;
  }

  async calibrateSensors() {
    console.log('🎯 Starting sensor calibration...');

    for (const [type, sensor] of this.detectedSensors) {
      console.log(`📊 Calibrating ${type} sensor...`);

      try {
        const calibrationResult = await this.calibrator.calibrate(sensor, {
          referenceValues: this._getReferenceValues(type),
          samples: 10,
          stabilizationTime: 30000, // 30 seconds
        });

        console.log(`✅ ${type} calibration complete:`, {
          offset: calibrationResult.offset,
          multiplier: calibrationResult.multiplier,
          accuracy: calibrationResult.accuracy,
        });

        // Save calibration data
        await sensor.saveCalibration(calibrationResult);
      } catch (error) {
        console.error(`❌ ${type} calibration failed:`, error.message);
      }
    }
  }

  _getReferenceValues(sensorType) {
    // Reference values for calibration
    const references = {
      temperature: {
        ice_point: 0.0, // °C
        room_temp: 23.0, // °C
        boiling_point: 100.0, // °C (at sea level)
      },
      pressure: {
        sea_level: 1013.25, // hPa
      },
      humidity: {
        saturated_salt: 75.3, // % RH (NaCl solution)
      },
    };

    return references[sensorType] || {};
  }

  async generateConfig() {
    const config = {
      station: {
        id: process.env.STATION_ID || `WS_${Date.now()}`,
        name: process.env.STATION_NAME || 'WRFCoin Weather Station',
        location: {
          latitude: parseFloat(process.env.LATITUDE) || 0,
          longitude: parseFloat(process.env.LONGITUDE) || 0,
          elevation: parseFloat(process.env.ELEVATION) || 0,
        },
      },
      sensors: {},
      network: {
        endpoint: process.env.WRFCOIN_ENDPOINT || 'https://api.wrfcoin.network',
        apiKey: process.env.WRFCOIN_API_KEY,
        submitInterval: 300000, // 5 minutes
      },
    };

    // Add detected sensors to config
    for (const [type, sensor] of this.detectedSensors) {
      config.sensors[type] = {
        model: sensor.model,
        address: sensor.address,
        enabled: true,
        pollInterval: this._getPollInterval(type),
        calibration: await sensor.getCalibration(),
      };
    }

    return config;
  }

  _getPollInterval(sensorType) {
    const intervals = {
      temperature: 60000, // 1 minute
      humidity: 60000, // 1 minute
      pressure: 60000, // 1 minute
      wind_speed: 10000, // 10 seconds
      wind_direction: 10000, // 10 seconds
      precipitation: 30000, // 30 seconds
      uv_index: 300000, // 5 minutes
      air_quality: 60000, // 1 minute
    };

    return intervals[sensorType] || 60000;
  }
}

module.exports = WeatherStationSetup;
```

---

## 3. Weather Station Registration

### Network Registration Process

```javascript
// registration/station-registration.js
const { WrfcoinClient } = require('@wrfcoin/sdk');
const crypto = require('crypto');

class StationRegistration {
  constructor(config) {
    this.config = config;
    this.client = new WrfcoinClient({
      network: config.network.endpoint,
      apiKey: config.network.apiKey,
      apiSecret: config.network.apiSecret,
    });
  }

  async registerStation() {
    console.log('📝 Registering weather station with WRFCoin network...');

    try {
      // Generate station identity
      const stationIdentity = this._generateStationIdentity();

      // Prepare registration payload
      const registrationData = {
        stationId: this.config.station.id,
        name: this.config.station.name,
        description: `Automated weather station - ${this.config.station.name}`,
        location: this.config.station.location,
        deviceInfo: {
          model: 'WRFCoin IoT Station v1.0',
          manufacturer: 'WRFCoin Community',
          firmwareVersion: await this._getFirmwareVersion(),
          serialNumber: stationIdentity.serialNumber,
          installationDate: new Date().toISOString(),
        },
        capabilities: this._getStationCapabilities(),
        operatingSchedule: {
          timezone: process.env.TZ || 'UTC',
          operationalHours: '24/7',
          maintenanceWindow: 'Sunday 02:00-04:00 UTC',
        },
        contactInfo: {
          operator: process.env.OPERATOR_NAME || 'Anonymous',
          email: process.env.OPERATOR_EMAIL,
          organization: process.env.ORGANIZATION,
        },
        technicalSpecs: this._getTechnicalSpecs(),
        signature: stationIdentity.signature,
      };

      // Submit registration
      const response = await this.client.climate.registerWeatherStation(registrationData);

      if (response.success) {
        console.log('✅ Station registered successfully!');
        console.log(`📍 Station ID: ${response.stationId}`);
        console.log(`🔗 Registration TX: ${response.transactionHash}`);
        console.log(
          `📊 Network URL: https://explorer.wrfcoin.network/station/${response.stationId}`
        );

        // Save registration information
        await this._saveRegistrationInfo({
          stationId: response.stationId,
          transactionHash: response.transactionHash,
          registrationDate: new Date().toISOString(),
          networkId: response.networkId,
          status: 'active',
        });

        return response;
      } else {
        throw new Error(`Registration failed: ${response.error}`);
      }
    } catch (error) {
      console.error('❌ Station registration failed:', error.message);
      throw error;
    }
  }

  _generateStationIdentity() {
    // Generate unique station identity
    const deviceId = this._getDeviceId();
    const timestamp = Date.now();
    const nonce = crypto.randomBytes(16).toString('hex');

    const identityString = `${deviceId}:${this.config.station.id}:${timestamp}:${nonce}`;
    const signature = crypto
      .createHmac('sha256', this.config.network.apiSecret)
      .update(identityString)
      .digest('hex');

    return {
      serialNumber: crypto
        .createHash('sha256')
        .update(deviceId)
        .digest('hex')
        .substring(0, 16)
        .toUpperCase(),
      timestamp,
      nonce,
      signature,
    };
  }

  _getDeviceId() {
    // Try to get hardware-specific identifier
    try {
      const fs = require('fs');

      // Raspberry Pi CPU serial
      if (fs.existsSync('/proc/cpuinfo')) {
        const cpuinfo = fs.readFileSync('/proc/cpuinfo', 'utf8');
        const serialMatch = cpuinfo.match(/Serial\s*:\s*([a-f0-9]+)/i);
        if (serialMatch) {
          return serialMatch[1];
        }
      }

      // MAC address fallback
      const os = require('os');
      const interfaces = os.networkInterfaces();
      for (const name of Object.keys(interfaces)) {
        for (const iface of interfaces[name]) {
          if (!iface.internal && iface.mac !== '00:00:00:00:00:00') {
            return iface.mac.replace(/:/g, '');
          }
        }
      }
    } catch (error) {
      console.warn('Could not determine hardware ID, using random ID');
    }

    // Random fallback
    return crypto.randomBytes(8).toString('hex');
  }

  _getStationCapabilities() {
    const capabilities = [];

    for (const [sensorType, sensorConfig] of Object.entries(this.config.sensors)) {
      if (sensorConfig.enabled) {
        capabilities.push(sensorType);
      }
    }

    // Add special capabilities
    if (this.config.features?.airQuality) {
      capabilities.push('air_quality_monitoring');
    }
    if (this.config.features?.soilMonitoring) {
      capabilities.push('soil_monitoring');
    }
    if (this.config.features?.solarRadiation) {
      capabilities.push('solar_radiation');
    }

    return capabilities;
  }

  _getTechnicalSpecs() {
    return {
      dataSubmissionInterval: this.config.network.submitInterval / 1000, // seconds
      sensors: Object.keys(this.config.sensors).length,
      powerSource: 'solar_battery',
      connectivity: ['wifi', 'ethernet'],
      dataStorage: {
        local: true,
        offlineDays: 7,
        compression: true,
      },
      qualityAssurance: {
        selfCalibration: true,
        redundantSensors: false,
        dataValidation: true,
      },
    };
  }

  async _getFirmwareVersion() {
    try {
      const packageJson = require('../package.json');
      return packageJson.version;
    } catch {
      return '1.0.0';
    }
  }

  async _saveRegistrationInfo(info) {
    const fs = require('fs').promises;
    const path = require('path');

    const registrationFile = path.join(
      process.env.HOME || '/home/pi',
      '.wrfcoin',
      'registration.json'
    );

    // Ensure directory exists
    await fs.mkdir(path.dirname(registrationFile), { recursive: true });

    // Save registration info
    await fs.writeFile(registrationFile, JSON.stringify(info, null, 2));

    console.log(`💾 Registration info saved to: ${registrationFile}`);
  }

  async verifyRegistration() {
    try {
      const response = await this.client.climate.getWeatherStation(this.config.station.id);

      if (response && response.status === 'active') {
        console.log('✅ Station registration verified');
        console.log(`📊 Total submissions: ${response.totalSubmissions}`);
        console.log(`🎯 Validation accuracy: ${response.validationAccuracy}%`);
        return true;
      } else {
        console.warn('⚠️  Station not found or inactive');
        return false;
      }
    } catch (error) {
      console.error('❌ Registration verification failed:', error.message);
      return false;
    }
  }
}

module.exports = StationRegistration;
```

---

## 4. Data Collection and Transmission

### Automated Data Collection Service

```javascript
// services/data-collection.js
const EventEmitter = require('events');

class DataCollectionService extends EventEmitter {
  constructor(config, deviceManager) {
    super();
    this.config = config;
    this.deviceManager = deviceManager;
    this.sensors = new Map();
    this.collectionQueue = [];
    this.isRunning = false;
    this.intervalHandles = new Map();
    this.lastReadings = new Map();
  }

  async initialize() {
    console.log('🔧 Initializing data collection service...');

    // Initialize sensors
    for (const [type, sensorConfig] of Object.entries(this.config.sensors)) {
      if (sensorConfig.enabled) {
        try {
          const sensor = await this.deviceManager.initializeSensor(type, sensorConfig);
          this.sensors.set(type, sensor);
          console.log(`✅ ${type} sensor initialized`);
        } catch (error) {
          console.error(`❌ Failed to initialize ${type} sensor:`, error.message);
        }
      }
    }

    console.log(`📡 Initialized ${this.sensors.size} sensors`);
  }

  start() {
    if (this.isRunning) {
      console.warn('⚠️  Data collection already running');
      return;
    }

    console.log('🚀 Starting data collection...');
    this.isRunning = true;

    // Start collection for each sensor
    for (const [type, sensor] of this.sensors) {
      const interval = this.config.sensors[type].pollInterval;

      const handle = setInterval(async () => {
        try {
          await this._collectSensorData(type, sensor);
        } catch (error) {
          console.error(`❌ Error collecting ${type} data:`, error.message);
          this.emit('sensor-error', { type, error });
        }
      }, interval);

      this.intervalHandles.set(type, handle);
      console.log(`📊 Started ${type} collection (${interval}ms interval)`);
    }

    // Start data submission service
    this._startSubmissionService();

    this.emit('collection-started');
  }

  stop() {
    if (!this.isRunning) {
      return;
    }

    console.log('🛑 Stopping data collection...');
    this.isRunning = false;

    // Clear all intervals
    for (const [type, handle] of this.intervalHandles) {
      clearInterval(handle);
      console.log(`📊 Stopped ${type} collection`);
    }
    this.intervalHandles.clear();

    this.emit('collection-stopped');
  }

  async _collectSensorData(type, sensor) {
    const timestamp = new Date().toISOString();

    try {
      // Read sensor data
      const rawReading = await sensor.read();

      // Validate reading
      const validatedReading = this._validateReading(type, rawReading);

      // Apply calibration
      const calibratedReading = await this._applyCalibraton(type, validatedReading);

      // Create data entry
      const dataEntry = {
        sensorType: type,
        timestamp,
        rawValue: rawReading,
        calibratedValue: calibratedReading,
        metadata: {
          sensorModel: sensor.model,
          calibrationDate: sensor.lastCalibration,
          temperature: sensor.internalTemperature, // For temperature compensation
          voltage: sensor.voltage, // For health monitoring
          signalStrength: sensor.signalStrength,
        },
        qualityFlags: this._assessDataQuality(type, calibratedReading, timestamp),
      };

      // Store reading
      this.lastReadings.set(type, dataEntry);
      this.collectionQueue.push(dataEntry);

      // Emit data event
      this.emit('data-collected', dataEntry);

      // Log significant changes
      this._logSignificantChanges(type, calibratedReading);
    } catch (error) {
      console.error(`❌ ${type} sensor read failed:`, error.message);

      // Record sensor error
      this.collectionQueue.push({
        sensorType: type,
        timestamp,
        error: error.message,
        errorType: 'sensor_read_failure',
      });
    }
  }

  _validateReading(type, reading) {
    const validationRules = {
      temperature: { min: -60, max: 80, unit: '°C' },
      humidity: { min: 0, max: 100, unit: '%' },
      pressure: { min: 800, max: 1200, unit: 'hPa' },
      wind_speed: { min: 0, max: 100, unit: 'm/s' },
      wind_direction: { min: 0, max: 360, unit: '°' },
      precipitation: { min: 0, max: 500, unit: 'mm/h' },
      uv_index: { min: 0, max: 15, unit: 'index' },
    };

    const rule = validationRules[type];
    if (!rule) {
      return reading; // No validation rule
    }

    if (typeof reading !== 'number' || isNaN(reading)) {
      throw new Error(`Invalid ${type} reading: not a number`);
    }

    if (reading < rule.min || reading > rule.max) {
      throw new Error(
        `${type} reading out of range: ${reading} ${rule.unit} (valid: ${rule.min}-${rule.max})`
      );
    }

    return reading;
  }

  async _applyCalibraton(type, reading) {
    const sensor = this.sensors.get(type);
    if (!sensor || !sensor.calibration) {
      return reading;
    }

    const { offset, multiplier } = sensor.calibration;
    return reading * multiplier + offset;
  }

  _assessDataQuality(type, value, timestamp) {
    const flags = [];

    // Check for rapid changes (potential sensor issues)
    const lastReading = this.lastReadings.get(type);
    if (lastReading && lastReading.calibratedValue !== undefined) {
      const timeDiff = new Date(timestamp) - new Date(lastReading.timestamp);
      const valueDiff = Math.abs(value - lastReading.calibratedValue);

      const rapidChangeThresholds = {
        temperature: { value: 5, time: 60000 }, // 5°C in 1 minute
        humidity: { value: 20, time: 60000 }, // 20% in 1 minute
        pressure: { value: 5, time: 60000 }, // 5 hPa in 1 minute
      };

      const threshold = rapidChangeThresholds[type];
      if (threshold && timeDiff < threshold.time && valueDiff > threshold.value) {
        flags.push('rapid_change');
      }
    }

    // Check for extreme values
    const extremeValueThresholds = {
      temperature: { min: -40, max: 60 },
      humidity: { min: 5, max: 95 },
      pressure: { min: 850, max: 1100 },
      wind_speed: { max: 50 },
    };

    const extreme = extremeValueThresholds[type];
    if (extreme) {
      if (
        (extreme.min !== undefined && value < extreme.min) ||
        (extreme.max !== undefined && value > extreme.max)
      ) {
        flags.push('extreme_value');
      }
    }

    // Check sensor health
    const sensor = this.sensors.get(type);
    if (sensor.voltage && sensor.voltage < sensor.minVoltage) {
      flags.push('low_voltage');
    }

    if (sensor.signalStrength && sensor.signalStrength < -80) {
      flags.push('weak_signal');
    }

    return flags;
  }

  _logSignificantChanges(type, value) {
    const lastReading = this.lastReadings.get(type);
    if (!lastReading) return;

    const changeThresholds = {
      temperature: 2, // 2°C
      humidity: 10, // 10%
      pressure: 2, // 2 hPa
      wind_speed: 5, // 5 m/s
    };

    const threshold = changeThresholds[type];
    if (threshold) {
      const change = Math.abs(value - lastReading.calibratedValue);
      if (change >= threshold) {
        console.log(`📈 Significant ${type} change: ${lastReading.calibratedValue} → ${value}`);
      }
    }
  }

  _startSubmissionService() {
    const submitInterval = this.config.network.submitInterval;

    setInterval(async () => {
      if (this.collectionQueue.length > 0) {
        await this._submitCollectedData();
      }
    }, submitInterval);

    console.log(`📤 Data submission service started (${submitInterval}ms interval)`);
  }

  async _submitCollectedData() {
    if (this.collectionQueue.length === 0) {
      return;
    }

    try {
      // Prepare submission batch
      const batchData = this._prepareBatchSubmission();

      console.log(`📤 Submitting batch of ${batchData.length} readings...`);

      // Submit to WRFCoin network
      const response = await this.deviceManager.submitBatchData(batchData);

      if (response.success) {
        console.log(`✅ Batch submitted successfully (TX: ${response.transactionHash})`);

        // Clear submitted data from queue
        this.collectionQueue.splice(0, batchData.length);

        this.emit('data-submitted', {
          batchSize: batchData.length,
          transactionHash: response.transactionHash,
          timestamp: new Date().toISOString(),
        });
      } else {
        console.error('❌ Batch submission failed:', response.error);
        this.emit('submission-error', response.error);
      }
    } catch (error) {
      console.error('❌ Data submission error:', error.message);
      this.emit('submission-error', error);
    }
  }

  _prepareBatchSubmission() {
    const maxBatchSize = this.config.data?.batchSize || 10;
    const batch = this.collectionQueue.slice(0, maxBatchSize);

    return batch
      .filter(entry => !entry.error)
      .map(entry => ({
        stationId: this.config.station.id,
        timestamp: entry.timestamp,
        sensorType: entry.sensorType,
        value: entry.calibratedValue,
        unit: this._getSensorUnit(entry.sensorType),
        quality: {
          flags: entry.qualityFlags,
          confidence: this._calculateConfidence(entry),
          calibrated: true,
        },
        metadata: entry.metadata,
      }));
  }

  _getSensorUnit(type) {
    const units = {
      temperature: '°C',
      humidity: '%',
      pressure: 'hPa',
      wind_speed: 'm/s',
      wind_direction: '°',
      precipitation: 'mm',
      uv_index: 'index',
    };
    return units[type] || 'unknown';
  }

  _calculateConfidence(entry) {
    let confidence = 100;

    // Reduce confidence based on quality flags
    if (entry.qualityFlags.includes('rapid_change')) confidence -= 20;
    if (entry.qualityFlags.includes('extreme_value')) confidence -= 30;
    if (entry.qualityFlags.includes('low_voltage')) confidence -= 10;
    if (entry.qualityFlags.includes('weak_signal')) confidence -= 15;

    return Math.max(confidence, 0);
  }

  getStatus() {
    return {
      running: this.isRunning,
      sensors: this.sensors.size,
      queueLength: this.collectionQueue.length,
      lastReadings: Object.fromEntries(
        Array.from(this.lastReadings).map(([type, reading]) => [
          type,
          {
            value: reading.calibratedValue,
            timestamp: reading.timestamp,
            quality: reading.qualityFlags,
          },
        ])
      ),
    };
  }
}

module.exports = DataCollectionService;
```

---

## 5. Monitoring and Maintenance

### Station Health Monitoring

```javascript
// monitoring/health-monitor.js
class StationHealthMonitor {
  constructor(config, dataService) {
    this.config = config;
    this.dataService = dataService;
    this.healthChecks = new Map();
    this.alerts = [];
    this.lastHealthReport = null;
  }

  start() {
    console.log('🏥 Starting health monitoring...');

    // Setup health checks
    this._setupHealthChecks();

    // Start monitoring
    setInterval(() => {
      this._performHealthChecks();
    }, 60000); // Every minute

    // Generate daily health reports
    setInterval(
      () => {
        this._generateHealthReport();
      },
      24 * 60 * 60 * 1000
    ); // Daily
  }

  _setupHealthChecks() {
    this.healthChecks.set('sensor_connectivity', () => this._checkSensorConnectivity());
    this.healthChecks.set('data_quality', () => this._checkDataQuality());
    this.healthChecks.set('network_connectivity', () => this._checkNetworkConnectivity());
    this.healthChecks.set('storage_space', () => this._checkStorageSpace());
    this.healthChecks.set('power_status', () => this._checkPowerStatus());
    this.healthChecks.set('temperature_health', () => this._checkSystemTemperature());
  }

  async _performHealthChecks() {
    const results = {};

    for (const [check, fn] of this.healthChecks) {
      try {
        results[check] = await fn();
      } catch (error) {
        results[check] = {
          status: 'error',
          message: error.message,
          timestamp: new Date().toISOString(),
        };
      }
    }

    // Process results and generate alerts
    this._processHealthResults(results);
  }

  async _checkSensorConnectivity() {
    const results = {};

    for (const [type, sensor] of this.dataService.sensors) {
      try {
        const testReading = await sensor.read();
        results[type] = {
          status: 'healthy',
          lastReading: testReading,
          timestamp: new Date().toISOString(),
        };
      } catch (error) {
        results[type] = {
          status: 'failed',
          error: error.message,
          timestamp: new Date().toISOString(),
        };
      }
    }

    return {
      status: Object.values(results).every(r => r.status === 'healthy') ? 'healthy' : 'degraded',
      sensors: results,
    };
  }

  _checkDataQuality() {
    const recentData = this.dataService.getRecentReadings(3600000); // Last hour
    const qualityIssues = [];

    for (const reading of recentData) {
      if (reading.qualityFlags && reading.qualityFlags.length > 0) {
        qualityIssues.push({
          sensor: reading.sensorType,
          flags: reading.qualityFlags,
          timestamp: reading.timestamp,
        });
      }
    }

    const qualityScore = Math.max(0, 100 - qualityIssues.length * 5);

    return {
      status: qualityScore > 80 ? 'healthy' : qualityScore > 60 ? 'warning' : 'critical',
      score: qualityScore,
      issues: qualityIssues,
      totalReadings: recentData.length,
    };
  }

  async _checkNetworkConnectivity() {
    try {
      const startTime = Date.now();
      const response = await fetch(this.config.network.endpoint + '/health');
      const latency = Date.now() - startTime;

      if (response.ok) {
        return {
          status: latency < 1000 ? 'healthy' : 'warning',
          latency,
          endpoint: this.config.network.endpoint,
        };
      } else {
        throw new Error(`HTTP ${response.status}`);
      }
    } catch (error) {
      return {
        status: 'failed',
        error: error.message,
        endpoint: this.config.network.endpoint,
      };
    }
  }

  async _checkStorageSpace() {
    try {
      const fs = require('fs').promises;
      const stats = await fs.statvfs(this.config.data.offline_storage || '/tmp');

      const totalSpace = stats.f_blocks * stats.f_frsize;
      const freeSpace = stats.f_bavail * stats.f_frsize;
      const usedPercent = ((totalSpace - freeSpace) / totalSpace) * 100;

      return {
        status: usedPercent < 80 ? 'healthy' : usedPercent < 90 ? 'warning' : 'critical',
        usedPercent: Math.round(usedPercent),
        freeSpace: Math.round(freeSpace / 1024 / 1024), // MB
        totalSpace: Math.round(totalSpace / 1024 / 1024), // MB
      };
    } catch (error) {
      return {
        status: 'unknown',
        error: error.message,
      };
    }
  }

  async _checkPowerStatus() {
    try {
      // Check if running on Raspberry Pi
      const fs = require('fs');

      if (fs.existsSync('/sys/class/power_supply/BAT0')) {
        // Battery-powered system
        const capacity = fs.readFileSync('/sys/class/power_supply/BAT0/capacity', 'utf8').trim();
        const status = fs.readFileSync('/sys/class/power_supply/BAT0/status', 'utf8').trim();

        return {
          status: parseInt(capacity) > 20 ? 'healthy' : 'warning',
          batteryLevel: parseInt(capacity),
          chargingStatus: status,
        };
      } else {
        // Assume AC powered
        return {
          status: 'healthy',
          powerSource: 'AC',
        };
      }
    } catch (error) {
      return {
        status: 'unknown',
        error: error.message,
      };
    }
  }

  async _checkSystemTemperature() {
    try {
      const fs = require('fs');

      // Raspberry Pi CPU temperature
      if (fs.existsSync('/sys/class/thermal/thermal_zone0/temp')) {
        const tempRaw = fs.readFileSync('/sys/class/thermal/thermal_zone0/temp', 'utf8').trim();
        const temperature = parseInt(tempRaw) / 1000; // Convert to Celsius

        return {
          status: temperature < 70 ? 'healthy' : temperature < 80 ? 'warning' : 'critical',
          temperature,
          unit: '°C',
        };
      } else {
        return {
          status: 'unknown',
          message: 'Temperature monitoring not available',
        };
      }
    } catch (error) {
      return {
        status: 'error',
        error: error.message,
      };
    }
  }

  _processHealthResults(results) {
    const newAlerts = [];

    for (const [check, result] of Object.entries(results)) {
      if (result.status === 'critical' || result.status === 'failed') {
        newAlerts.push({
          type: 'critical',
          check,
          message: result.error || result.message || `${check} check failed`,
          timestamp: new Date().toISOString(),
        });
      } else if (result.status === 'warning') {
        newAlerts.push({
          type: 'warning',
          check,
          message: result.message || `${check} needs attention`,
          timestamp: new Date().toISOString(),
        });
      }
    }

    // Add new alerts
    this.alerts.push(...newAlerts);

    // Keep only recent alerts (last 24 hours)
    const cutoff = new Date(Date.now() - 24 * 60 * 60 * 1000);
    this.alerts = this.alerts.filter(alert => new Date(alert.timestamp) > cutoff);

    // Log new alerts
    for (const alert of newAlerts) {
      if (alert.type === 'critical') {
        console.error(`🚨 CRITICAL: ${alert.message}`);
      } else {
        console.warn(`⚠️  WARNING: ${alert.message}`);
      }
    }
  }

  async _generateHealthReport() {
    const report = {
      timestamp: new Date().toISOString(),
      stationId: this.config.station.id,
      uptime: process.uptime(),
      status: this._calculateOverallStatus(),
      sensors: this._getSensorReport(),
      network: this._getNetworkReport(),
      storage: this._getStorageReport(),
      alerts: this.alerts.length,
      recommendations: this._generateRecommendations(),
    };

    console.log('📊 Daily health report generated');
    this.lastHealthReport = report;

    // Save report to file
    await this._saveHealthReport(report);

    return report;
  }

  _calculateOverallStatus() {
    if (this.alerts.some(a => a.type === 'critical')) {
      return 'critical';
    } else if (this.alerts.some(a => a.type === 'warning')) {
      return 'warning';
    } else {
      return 'healthy';
    }
  }

  _getSensorReport() {
    const status = this.dataService.getStatus();
    return {
      active: status.sensors,
      queueLength: status.queueLength,
      lastReadings: status.lastReadings,
    };
  }

  _getNetworkReport() {
    // This would be populated from the latest network check
    return {
      connectivity: 'healthy',
      latency: 'unknown',
    };
  }

  _getStorageReport() {
    // This would be populated from the latest storage check
    return {
      status: 'healthy',
      usage: 'unknown',
    };
  }

  _generateRecommendations() {
    const recommendations = [];

    // Based on recent alerts, generate maintenance recommendations
    const recentCritical = this.alerts.filter(a => a.type === 'critical');
    const recentWarnings = this.alerts.filter(a => a.type === 'warning');

    if (recentCritical.length > 0) {
      recommendations.push('Immediate attention required for critical issues');
    }

    if (recentWarnings.length > 5) {
      recommendations.push('Multiple warnings detected - schedule maintenance check');
    }

    // Check for pattern in sensor issues
    const sensorIssues = this.alerts.filter(a => a.check === 'sensor_connectivity');
    if (sensorIssues.length > 3) {
      recommendations.push('Check sensor connections and cables');
    }

    return recommendations;
  }

  async _saveHealthReport(report) {
    const fs = require('fs').promises;
    const path = require('path');

    const reportsDir = path.join(process.env.HOME || '/home/pi', '.wrfcoin', 'reports');
    await fs.mkdir(reportsDir, { recursive: true });

    const filename = `health-report-${new Date().toISOString().split('T')[0]}.json`;
    const filepath = path.join(reportsDir, filename);

    await fs.writeFile(filepath, JSON.stringify(report, null, 2));
    console.log(`💾 Health report saved: ${filepath}`);
  }

  getHealthStatus() {
    return {
      status: this._calculateOverallStatus(),
      alerts: this.alerts,
      lastReport: this.lastHealthReport,
    };
  }
}

module.exports = StationHealthMonitor;
```

---

## Next Steps

1. **Explore [Cross-Chain Integration](../cross-chain/)**
2. **Build [Real-time Dashboards](../applications/02-realtime-dashboard.md)**
3. **Deploy [Production Weather Station](./02-production-deployment.md)**

---

## Troubleshooting

### Common Issues

**Sensor Not Detected:**

```bash
# Check I2C devices
sudo i2cdetect -y 1

# Check USB devices
lsusb

# Check GPIO status
gpio readall
```

**Network Connection Issues:**

```bash
# Test API connectivity
curl -H "Authorization: Bearer $API_KEY" https://api.wrfcoin.network/health

# Check WiFi status
iwconfig

# Test DNS resolution
nslookup api.wrfcoin.network
```

**Data Quality Issues:**

- Verify sensor calibration dates
- Check for physical obstructions
- Validate power supply stability
- Review environmental conditions

This comprehensive weather station integration guide provides everything needed to deploy a
professional-grade IoT weather station connected to the WRFCoin network.
