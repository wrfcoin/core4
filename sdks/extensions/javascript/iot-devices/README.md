# Wrfcoin IoT Integration Libraries

This package provides JavaScript libraries for connecting IoT devices, particularly weather stations
and air quality monitors, to the Wrfcoin blockchain network. These libraries make it easy to
register devices, submit data, and handle various device operations.

## Features

- **Device Registration**: Register weather stations and air quality monitors with the Wrfcoin
  network
- **Automatic Data Submission**: Schedule automatic data collection and submission
- **Status Monitoring**: Track device status and health
- **Air Quality Analysis**: Calculate Air Quality Index (AQI) based on various pollutants
- **Event Handling**: Listen for important events like data submission and alerts
- **Offline Queue**: Store data when network connection is unavailable
- **Device Management**: Centralized management of multiple devices
- **Canonical Weather Payloads**: Emits `wrf.weather.observation.v1` payloads by default for submit
  paths

## Installation

```bash
npm install @wrfcoin/iot-sdk
```

## Usage

Production clients require an injected physical-device adapter. The SDK does not generate sensor
readings, health, calibration, or firmware results. An adapter must return observations read from
the connected device and should throw when the device cannot be read.

### Basic Usage

```javascript
const {
  WeatherStationClient,
  AirQualityMonitorClient,
  IoTDeviceManager,
} = require('@wrfcoin/iot-sdk');
const WrfcoinSDK = require('@wrfcoin/sdk');

// Initialize the Wrfcoin SDK
const sdk = new WrfcoinSDK({
  apiUrl: 'https://api.wrfcoin.network',
  network: 'testnet', // or 'mainnet' for production
  apiKey: 'YOUR_API_KEY',
});

// Create a weather station client
const weatherStation = new WeatherStationClient({
  sdk,
  adapter: physicalWeatherStationAdapter,
  deviceInfo: {
    name: 'My Weather Station',
    model: 'WS-2000',
    firmwareVersion: '1.0.0',
    location: {
      latitude: 37.7749,
      longitude: -122.4194,
      elevation: 16,
    },
    parameters: ['temperature', 'humidity', 'pressure'],
  },
  updateInterval: 15, // minutes
  network: 'testnet',
});

// Register the device
await weatherStation.register();

// Start automatic data submission
await weatherStation.start();
```

Submitted weather payloads are canonicalized to:

```json
{
  "schema_version": "wrf.weather.observation.v1",
  "station_id": "device-or-station-id",
  "observed_at_unix": 1700110000,
  "observation": {
    "temperature_c": 21.4,
    "humidity": 58.0,
    "pressure_hpa": 1009.4
  }
}
```

Legacy keys (`temperature`, `windSpeed`, `rainfall`, etc.) are accepted as input for migration and
normalized to canonical fields at submit time.

### Using the IoT Device Manager

```javascript
// Create a device manager
const deviceManager = new IoTDeviceManager({
  sdk,
  network: 'testnet',
});

// Add a weather station
const weatherStation = deviceManager.addWeatherStation(
  'weather-station-1',
  {
    name: 'Weather Station Alpha',
    model: 'WS-2000',
    firmwareVersion: '1.0.0',
    location: {
      latitude: 37.7749,
      longitude: -122.4194,
      elevation: 16,
    },
    parameters: ['temperature', 'humidity', 'pressure'],
  },
  { adapter: physicalWeatherStationAdapter }
);

// Add an air quality monitor
const airQualityMonitor = deviceManager.addAirQualityMonitor(
  'air-quality-1',
  {
    name: 'Air Quality Monitor Beta',
    model: 'AQM-500',
    firmwareVersion: '2.1.0',
    location: {
      latitude: 37.7749,
      longitude: -122.4194,
      elevation: 16,
    },
    pollutants: ['pm25', 'pm10', 'o3'],
  },
  { adapter: physicalAirQualityAdapter }
);

// Register all devices
await weatherStation.register();
await airQualityMonitor.register();

// Start all devices
await deviceManager.startAll();
```

## Event Handling

The library provides an event system for handling important events:

```javascript
// Listen for data submission events
weatherStation.on('data-submitted', data => {
  console.log('Weather data submitted:', data.data);
});

// Listen for air quality alerts
airQualityMonitor.on('alert-triggered', alert => {
  console.warn('Air quality alert triggered:', alert.alert);
});

// Listen for errors
weatherStation.on('error', error => {
  console.error('Weather station error:', error.message);
});

// Optional migration telemetry: legacy input keys detected and canonicalized
weatherStation.on('legacy-format-detected', telemetry => {
  console.warn('Legacy weather fields normalized:', telemetry.legacyKeys);
});
```

## Extending the Library

You can extend the base classes to implement custom functionality:

```javascript
class CustomWeatherStation extends WeatherStationClient {
  async _readDeviceData() {
    // Implement custom sensor reading logic
    return {
      temperature: await this._readTemperatureSensor(),
      humidity: await this._readHumiditySensor(),
      pressure: await this._readPressureSensor(),
      timestamp: new Date().toISOString(),
    };
  }

  async _readTemperatureSensor() {
    // Your custom implementation
  }

  // Add more custom methods...
}
```

## Examples

See the [examples](./examples) directory for complete examples.

## License

MIT
