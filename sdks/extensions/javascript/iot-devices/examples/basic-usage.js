/**
 * Basic Usage Example for Wrfcoin IoT Libraries
 *
 * This example demonstrates how to use the IoT libraries to connect
 * weather stations and air quality monitors to the Wrfcoin network.
 */

// Import the Wrfcoin SDK
const WrfcoinSDK = require('../../WrfcoinSDK');

// Import the IoT libraries
const {
  WeatherStationClient,
  AirQualityMonitorClient,
  IoTDeviceManager,
} = require('../index');

// Initialize the Wrfcoin SDK
const sdk = new WrfcoinSDK({
  apiUrl: 'https://api.wrfcoin.network',
  network: 'testnet', // or 'mainnet' for production
  apiKey: 'YOUR_API_KEY', // Replace with your API key
});

// Example 1: Using the IoTDeviceManager
async function deviceManagerExample() {
  console.log('======= IoT Device Manager Example =======');

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
      parameters: [
        'temperature',
        'humidity',
        'pressure',
        'windSpeed',
        'windDirection',
        'rainfall',
      ],
    },
    {
      updateInterval: 5, // 5 minutes
    }
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
      pollutants: ['pm25', 'pm10', 'o3', 'co', 'no2', 'so2'],
    },
    {
      updateInterval: 10, // 10 minutes
    }
  );

  // Register all devices with the Wrfcoin network
  await weatherStation.register();
  await airQualityMonitor.register();

  // Listen for events
  deviceManager.on('data-received', data => {
    console.log(`Received data from device ${data.deviceId}:`, data.data);
  });

  deviceManager.on('error', error => {
    console.error(`Error from device ${error.deviceId}:`, error.message);
  });

  deviceManager.on('alert', alert => {
    console.warn(`Alert from device ${alert.deviceId}:`, alert.alert);
  });

  // Start all devices
  console.log('Starting all devices...');
  await deviceManager.startAll();

  // Fetch data from all devices
  console.log('Fetching data from all devices...');
  const dataResults = await deviceManager.fetchAllData();
  console.log('Data fetch results:', dataResults);

  // Stop all devices after 1 minute
  setTimeout(() => {
    console.log('Stopping all devices...');
    deviceManager.stopAll();
  }, 60000);
}

// Example 2: Using WeatherStationClient directly
async function weatherStationExample() {
  console.log('======= Weather Station Example =======');

  // Create a weather station client
  const weatherStation = new WeatherStationClient({
    sdk,
    deviceInfo: {
      name: 'Rooftop Weather Station',
      model: 'WS-3000',
      firmwareVersion: '1.2.0',
      location: {
        latitude: 40.7128,
        longitude: -74.006,
        elevation: 25,
      },
      parameters: [
        'temperature',
        'humidity',
        'pressure',
        'windSpeed',
        'rainfall',
        'solarRadiation',
      ],
    },
    updateInterval: 15, // 15 minutes
    network: 'testnet',
  });

  // Register the device
  console.log('Registering weather station...');
  const registrationResult = await weatherStation.register();
  console.log('Registration result:', registrationResult);

  // Listen for events
  weatherStation.on('data-submitted', data => {
    console.log('Weather data submitted:', data.data);
  });

  weatherStation.on('error', error => {
    console.error('Weather station error:', error.message);
  });

  // Start automatic data submission
  console.log('Starting automatic data submission...');
  await weatherStation.start();

  // Read and submit data manually
  console.log('Reading and submitting data manually...');
  const data = await weatherStation.readAndSubmitData();
  console.log('Manual data submission result:', data);

  // Stop automatic data submission after 2 minutes
  setTimeout(() => {
    console.log('Stopping automatic data submission...');
    weatherStation.stop();
  }, 120000);
}

// Example 3: Using AirQualityMonitorClient directly
async function airQualityMonitorExample() {
  console.log('======= Air Quality Monitor Example =======');

  // Create an air quality monitor client
  const airQualityMonitor = new AirQualityMonitorClient({
    sdk,
    deviceInfo: {
      name: 'City Center Air Monitor',
      model: 'AQM-1000',
      firmwareVersion: '2.0.1',
      location: {
        latitude: 51.5074,
        longitude: -0.1278,
        elevation: 11,
      },
      pollutants: ['pm25', 'pm10', 'o3', 'no2'],
    },
    updateInterval: 10, // 10 minutes
    network: 'testnet',
  });

  // Register the device
  console.log('Registering air quality monitor...');
  const registrationResult = await airQualityMonitor.register();
  console.log('Registration result:', registrationResult);

  // Listen for events
  airQualityMonitor.on('data-submitted', data => {
    console.log('Air quality data submitted:', data.data);
  });

  airQualityMonitor.on('alert-triggered', alert => {
    console.warn('Air quality alert triggered:', alert.alert);
  });

  airQualityMonitor.on('error', error => {
    console.error('Air quality monitor error:', error.message);
  });

  // Start automatic data submission
  console.log('Starting automatic data submission...');
  await airQualityMonitor.start();

  // Calculate AQI for a PM2.5 value
  const pm25 = 15.5;
  const aqi = airQualityMonitor.calculatePM25AQI(pm25);
  console.log(`PM2.5 value ${pm25} µg/m³ corresponds to AQI: ${aqi}`);

  // Stop automatic data submission after 2 minutes
  setTimeout(() => {
    console.log('Stopping automatic data submission...');
    airQualityMonitor.stop();
  }, 120000);
}

// Run the examples
async function runExamples() {
  try {
    // Run the device manager example
    await deviceManagerExample();

    // Wait a bit before starting the next example
    await new Promise(resolve => setTimeout(resolve, 5000));

    // Run the weather station example
    await weatherStationExample();

    // Wait a bit before starting the next example
    await new Promise(resolve => setTimeout(resolve, 5000));

    // Run the air quality monitor example
    await airQualityMonitorExample();
  } catch (error) {
    console.error('Error running examples:', error);
  }
}

// Run the examples if this file is executed directly
if (require.main === module) {
  runExamples().catch(error => {
    console.error('Error:', error);
  });
}
