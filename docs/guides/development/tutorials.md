# Wrfcoin Developer Tutorials

This guide provides practical examples and step-by-step instructions for common development tasks
when working with the Wrfcoin platform. These tutorials are designed to help developers quickly
integrate with the Wrfcoin ecosystem.

## Table of Contents

1. [Setting Up a Development Environment](#setting-up-a-development-environment)
2. [Creating a Simple Weather Data Node](#creating-a-simple-weather-data-node)
3. [Implementing Cross-Chain Communication](#implementing-cross-chain-communication)
4. [Building Custom Weather Data Validators](#building-custom-weather-data-validators)
5. [Developing IoT Device Integrations](#developing-iot-device-integrations)
6. [Creating Computational NFTs](#creating-computational-nfts)
7. [Working with the Climate Oracle Network](#working-with-the-climate-oracle-network)
8. [Wallet Integration Basics](#wallet-integration-basics)
9. [Implementing Governance Features](#implementing-governance-features)
10. [Handling Weather Station Data](#handling-weather-station-data)

---

## Setting Up a Development Environment

This tutorial walks you through setting up a local development environment for Wrfcoin development.

### Prerequisites

- Node.js (v18 or higher)
- Rust (latest stable version)
- PostgreSQL database
- Git

### Step 1: Clone the Repository

```bash
git clone https://github.com/wrfcoin/wrfcoin-core.git
cd wrfcoin-core
```

### Step 2: Install Dependencies

```bash
# Install Node.js dependencies
npm install

# Install Rust components (if working with core blockchain)
cd rust-core
cargo build
cd ..
```

### Step 3: Configure Environment

Create a `.env` file in the root directory with the following settings:

```
# Network Configuration
NETWORK=testnet
NODE_TYPE=full

# Database Connection
DATABASE_URL=postgresql://username:password@localhost:5432/wrfcoin

# Blockchain Settings
ENABLE_MINING=true
ENABLE_VALIDATOR=false
ENABLE_STORAGE_NODE=false
```

### Step 4: Start Development Server

```bash
# Start Wrfcoin Core in development mode
npm run dev
```

Visit `http://localhost:5000` to access the development interface.

---

## Creating a Simple Weather Data Node

This tutorial guides you through creating a basic weather data node that collects and submits data
to the Wrfcoin network.

### Prerequisites

- Completed development environment setup
- Basic understanding of weather data formats
- Access to weather data source (API or physical sensor)

### Step 1: Set Up Required Components

Create a new directory for your weather node:

```bash
mkdir -p my-weather-node/src
cd my-weather-node
npm init -y
```

Install necessary dependencies:

```bash
npm install @wrfcoin/sdk axios winston
```

### Step 2: Create Data Collection Script

Create a file `src/collector.js`:

```javascript
const axios = require('axios');
const { WeatherDataFormatter } = require('@wrfcoin/sdk');

// Replace with your actual weather data source API
const WEATHER_API_URL = 'https://api.weathersource.com/v1/current';
const API_KEY = process.env.WEATHER_API_KEY;

async function collectWeatherData(location) {
  try {
    const response = await axios.get(WEATHER_API_URL, {
      params: {
        lat: location.lat,
        lon: location.lon,
        apiKey: API_KEY,
      },
    });

    // Format the data according to Wrfcoin standards
    const formatter = new WeatherDataFormatter();
    const formattedData = formatter.format(response.data);

    return formattedData;
  } catch (error) {
    console.error('Error collecting weather data:', error);
    throw error;
  }
}

module.exports = { collectWeatherData };
```

### Step 3: Create Node Service

Create a file `src/index.js`:

```javascript
const { WrfcoinNode, WeatherDataValidator } = require('@wrfcoin/sdk');
const { collectWeatherData } = require('./collector');
const winston = require('winston');

// Setup logging
const logger = winston.createLogger({
  level: 'info',
  format: winston.format.json(),
  transports: [
    new winston.transports.Console(),
    new winston.transports.File({ filename: 'weather-node.log' }),
  ],
});

async function main() {
  try {
    // Initialize the node
    const node = new WrfcoinNode({
      nodeType: 'weather',
      network: 'testnet',
      logger,
    });

    await node.connect();
    logger.info('Connected to Wrfcoin network');

    // Define location for weather data collection
    const location = {
      lat: 40.7128,
      lon: -74.006,
      name: 'New York City',
    };

    // Set up data validation
    const validator = new WeatherDataValidator();

    // Start data collection and submission cycle
    setInterval(async () => {
      try {
        // Collect data
        const weatherData = await collectWeatherData(location);

        // Validate data before submission
        const validationResult = validator.validate(weatherData);

        if (validationResult.valid) {
          // Submit data to network
          const txHash = await node.submitWeatherData(weatherData);
          logger.info(`Weather data submitted successfully: ${txHash}`);
        } else {
          logger.warn(`Invalid weather data: ${validationResult.errors.join(', ')}`);
        }
      } catch (error) {
        logger.error('Error in data collection cycle:', error);
      }
    }, 300000); // Submit data every 5 minutes
  } catch (error) {
    logger.error('Failed to start weather node:', error);
    process.exit(1);
  }
}

main();
```

### Step 4: Run Your Weather Node

```bash
# Set your API key
export WEATHER_API_KEY=your_api_key_here

# Start the node
node src/index.js
```

Your node will now collect weather data every 5 minutes and submit it to the Wrfcoin network.

---

## Implementing Cross-Chain Communication

This tutorial demonstrates how to set up cross-chain communication between Wrfcoin and other
blockchain networks.

### Prerequisites

- Completed development environment setup
- Basic understanding of cross-chain protocols
- Access to target blockchain network

### Step 1: Install Required Dependencies

```bash
npm install @wrfcoin/sdk @wrfcoin/cross-chain ethers @axelar-network/axelar-gmp-sdk-solidity
```

### Step 2: Create Cross-Chain Bridge Configuration

Create a file `bridge-config.js`:

```javascript
module.exports = {
  wrfcoin: {
    rpcUrl: 'https://testnet.wrfcoin.network/rpc',
    chainId: '0x2',
    bridgeContract: '0xaBcD...1234',
  },
  ethereum: {
    rpcUrl: 'https://goerli.infura.io/v3/your-infura-key',
    chainId: '0x5',
    bridgeContract: '0x1234...aBcD',
  },
  axelar: {
    gateway: '0xC249632c2D86b1E9d91E41b9d00A24AA6dE6488d',
    gasReceiver: '0xbE406F0189A0B4cf3A05C286473D23791Dd44Cc6',
  },
};
```

### Step 3: Implement Communication Bridge

Create a file `cross-chain-bridge.js`:

```javascript
const { ethers } = require('ethers');
const {
  AxelarGMPRecoveryAPI,
  AxelarQueryAPI,
  AxelarGateway,
} = require('@axelar-network/axelarjs-sdk');
const { WrfcoinBridge } = require('@wrfcoin/cross-chain');
const config = require('./bridge-config');

class CrossChainBridge {
  constructor() {
    // Initialize Wrfcoin connection
    this.wrfcoinProvider = new ethers.providers.JsonRpcProvider(config.wrfcoin.rpcUrl);

    // Initialize Ethereum connection
    this.ethereumProvider = new ethers.providers.JsonRpcProvider(config.ethereum.rpcUrl);

    // Initialize bridge contracts
    this.wrfcoinBridge = new WrfcoinBridge(config.wrfcoin.bridgeContract, this.wrfcoinProvider);

    // Initialize Axelar SDK
    this.axelarQuery = new AxelarQueryAPI({
      environment: 'testnet',
    });

    this.axelarRecovery = new AxelarGMPRecoveryAPI({
      environment: 'testnet',
    });
  }

  async sendWeatherDataToEthereum(weatherData, destinationAddress) {
    try {
      // Format the weather data for cross-chain transmission
      const formattedData = this.formatWeatherData(weatherData);

      // Create and sign the transaction
      const tx = await this.wrfcoinBridge.sendMessageToEthereum(
        config.ethereum.chainId,
        destinationAddress,
        formattedData,
        { gasLimit: 300000 }
      );

      // Wait for transaction confirmation
      const receipt = await tx.wait();
      console.log(`Cross-chain message sent with tx hash: ${receipt.transactionHash}`);

      // Get the Axelar transaction hash for monitoring
      const axelarTxHash = receipt.logs[0].topics[1];
      return axelarTxHash;
    } catch (error) {
      console.error('Error sending cross-chain message:', error);
      throw error;
    }
  }

  async checkMessageStatus(axelarTxHash) {
    try {
      const status = await this.axelarRecovery.queryTransactionStatus(axelarTxHash);
      return status;
    } catch (error) {
      console.error('Error checking message status:', error);
      throw error;
    }
  }

  formatWeatherData(weatherData) {
    // Convert weather data to a format suitable for cross-chain transmission
    // This typically involves stringification and compression
    return ethers.utils.defaultAbiCoder.encode(
      ['string', 'uint256', 'uint256', 'uint256', 'uint256', 'uint256'],
      [
        weatherData.location,
        Math.floor(weatherData.timestamp / 1000),
        Math.floor(weatherData.temperature * 100),
        Math.floor(weatherData.humidity * 100),
        Math.floor(weatherData.pressure * 100),
        Math.floor(weatherData.windSpeed * 100),
      ]
    );
  }
}

module.exports = CrossChainBridge;
```

### Step 4: Use the Bridge in Your Application

```javascript
const CrossChainBridge = require('./cross-chain-bridge');

async function sendWeatherDataCrossChain() {
  const bridge = new CrossChainBridge();

  // Sample weather data to send
  const weatherData = {
    location: 'New York City',
    timestamp: Date.now(),
    temperature: 22.5, // Celsius
    humidity: 65.0, // Percentage
    pressure: 1013.2, // hPa
    windSpeed: 5.7, // m/s
  };

  // Ethereum contract address that will receive the data
  const destinationAddress = '0x5678...EFGH';

  try {
    // Send the data cross-chain
    const axelarTxHash = await bridge.sendWeatherDataToEthereum(weatherData, destinationAddress);
    console.log(`Data sent cross-chain with Axelar tx hash: ${axelarTxHash}`);

    // Monitor status
    setTimeout(async () => {
      const status = await bridge.checkMessageStatus(axelarTxHash);
      console.log(`Cross-chain message status: ${status}`);
    }, 60000); // Check after 1 minute
  } catch (error) {
    console.error('Failed to send data cross-chain:', error);
  }
}

sendWeatherDataCrossChain();
```

---

## Building Custom Weather Data Validators

This tutorial explains how to create custom validation logic for weather data.

### Prerequisites

- Completed development environment setup
- Understanding of weather data parameters
- Basic knowledge of validation techniques

### Step 1: Create a Basic Validator

Create a file `custom-validator.js`:

```javascript
const { BaseValidator } = require('@wrfcoin/sdk');

class CustomWeatherValidator extends BaseValidator {
  constructor(options = {}) {
    super();
    this.options = {
      temperatureRange: options.temperatureRange || { min: -100, max: 100 },
      humidityRange: options.humidityRange || { min: 0, max: 100 },
      pressureRange: options.pressureRange || { min: 800, max: 1200 },
      windSpeedRange: options.windSpeedRange || { min: 0, max: 200 },
      requiredFields: options.requiredFields || [
        'timestamp',
        'location',
        'temperature',
        'humidity',
        'pressure',
        'windSpeed',
      ],
    };
  }

  validate(weatherData) {
    const errors = [];

    // Check required fields
    for (const field of this.options.requiredFields) {
      if (weatherData[field] === undefined) {
        errors.push(`Missing required field: ${field}`);
      }
    }

    // Validate timestamp
    if (weatherData.timestamp) {
      const timestamp = new Date(weatherData.timestamp).getTime();
      const now = Date.now();
      if (isNaN(timestamp)) {
        errors.push('Invalid timestamp format');
      } else if (timestamp > now + 300000) {
        // More than 5 minutes in the future
        errors.push('Timestamp is too far in the future');
      } else if (timestamp < now - 86400000) {
        // More than 1 day in the past
        errors.push('Timestamp is too old');
      }
    }

    // Validate temperature
    if (weatherData.temperature !== undefined) {
      if (typeof weatherData.temperature !== 'number') {
        errors.push('Temperature must be a number');
      } else if (
        weatherData.temperature < this.options.temperatureRange.min ||
        weatherData.temperature > this.options.temperatureRange.max
      ) {
        errors.push(
          `Temperature outside valid range: ${this.options.temperatureRange.min} to ${this.options.temperatureRange.max}`
        );
      }
    }

    // Validate humidity
    if (weatherData.humidity !== undefined) {
      if (typeof weatherData.humidity !== 'number') {
        errors.push('Humidity must be a number');
      } else if (
        weatherData.humidity < this.options.humidityRange.min ||
        weatherData.humidity > this.options.humidityRange.max
      ) {
        errors.push(
          `Humidity outside valid range: ${this.options.humidityRange.min} to ${this.options.humidityRange.max}`
        );
      }
    }

    // Validate pressure
    if (weatherData.pressure !== undefined) {
      if (typeof weatherData.pressure !== 'number') {
        errors.push('Pressure must be a number');
      } else if (
        weatherData.pressure < this.options.pressureRange.min ||
        weatherData.pressure > this.options.pressureRange.max
      ) {
        errors.push(
          `Pressure outside valid range: ${this.options.pressureRange.min} to ${this.options.pressureRange.max}`
        );
      }
    }

    // Validate wind speed
    if (weatherData.windSpeed !== undefined) {
      if (typeof weatherData.windSpeed !== 'number') {
        errors.push('Wind speed must be a number');
      } else if (
        weatherData.windSpeed < this.options.windSpeedRange.min ||
        weatherData.windSpeed > this.options.windSpeedRange.max
      ) {
        errors.push(
          `Wind speed outside valid range: ${this.options.windSpeedRange.min} to ${this.options.windSpeedRange.max}`
        );
      }
    }

    return {
      valid: errors.length === 0,
      errors,
    };
  }

  // Advanced: Add anomaly detection
  detectAnomalies(weatherData, historicalData) {
    const anomalies = [];

    // Skip if no historical data
    if (!historicalData || historicalData.length === 0) {
      return { hasAnomalies: false, anomalies };
    }

    // Calculate mean and standard deviation for each parameter
    const stats = this.calculateStats(historicalData);

    // Check for temperature anomalies (more than 3 standard deviations from mean)
    if (Math.abs(weatherData.temperature - stats.temperature.mean) > 3 * stats.temperature.stdDev) {
      anomalies.push('Temperature value is anomalous');
    }

    // Check for pressure anomalies
    if (Math.abs(weatherData.pressure - stats.pressure.mean) > 3 * stats.pressure.stdDev) {
      anomalies.push('Pressure value is anomalous');
    }

    return {
      hasAnomalies: anomalies.length > 0,
      anomalies,
    };
  }

  calculateStats(historicalData) {
    // Calculate statistics for historical data
    const stats = {
      temperature: { values: [], mean: 0, stdDev: 0 },
      humidity: { values: [], mean: 0, stdDev: 0 },
      pressure: { values: [], mean: 0, stdDev: 0 },
      windSpeed: { values: [], mean: 0, stdDev: 0 },
    };

    // Extract values
    historicalData.forEach(data => {
      if (data.temperature !== undefined) stats.temperature.values.push(data.temperature);
      if (data.humidity !== undefined) stats.humidity.values.push(data.humidity);
      if (data.pressure !== undefined) stats.pressure.values.push(data.pressure);
      if (data.windSpeed !== undefined) stats.windSpeed.values.push(data.windSpeed);
    });

    // Calculate mean and standard deviation for each parameter
    for (const param in stats) {
      if (stats[param].values.length > 0) {
        stats[param].mean =
          stats[param].values.reduce((sum, val) => sum + val, 0) / stats[param].values.length;

        const squaredDiffs = stats[param].values.map(val => Math.pow(val - stats[param].mean, 2));
        const avgSquaredDiff =
          squaredDiffs.reduce((sum, val) => sum + val, 0) / squaredDiffs.length;
        stats[param].stdDev = Math.sqrt(avgSquaredDiff);
      }
    }

    return stats;
  }
}

module.exports = CustomWeatherValidator;
```

### Step 2: Use the Custom Validator

```javascript
const CustomWeatherValidator = require('./custom-validator');

// Create an instance with custom ranges
const validator = new CustomWeatherValidator({
  temperatureRange: { min: -50, max: 50 }, // More restrictive temperature range
  pressureRange: { min: 900, max: 1100 }, // More restrictive pressure range
});

// Weather data to validate
const weatherData = {
  timestamp: Date.now(),
  location: 'New York City',
  temperature: 22.5,
  humidity: 65.0,
  pressure: 1013.2,
  windSpeed: 5.7,
};

// Perform validation
const validationResult = validator.validate(weatherData);

if (validationResult.valid) {
  console.log('Weather data is valid.');
} else {
  console.log('Weather data validation failed:');
  validationResult.errors.forEach(error => console.log(`- ${error}`));
}

// Historical data for anomaly detection
const historicalData = [
  { temperature: 22.1, humidity: 64.0, pressure: 1012.8, windSpeed: 5.2 },
  { temperature: 22.3, humidity: 64.5, pressure: 1013.0, windSpeed: 5.5 },
  { temperature: 22.0, humidity: 65.2, pressure: 1012.9, windSpeed: 5.3 },
  // ... more historical data points
];

// Check for anomalies
const anomalyResult = validator.detectAnomalies(weatherData, historicalData);

if (anomalyResult.hasAnomalies) {
  console.log('Anomalies detected:');
  anomalyResult.anomalies.forEach(anomaly => console.log(`- ${anomaly}`));
} else {
  console.log('No anomalies detected.');
}
```

---

## Developing IoT Device Integrations

This tutorial demonstrates how to integrate IoT weather devices with the Wrfcoin network.

### Prerequisites

- A compatible IoT weather station or sensor
- Completed development environment setup
- Basic understanding of IoT communication protocols

### Step 1: Install Required Dependencies

```bash
npm install @wrfcoin/sdk @wrfcoin/iot serialport mqtt
```

### Step 2: Create Device Communication Module

Create a file `device-connector.js`:

```javascript
const SerialPort = require('serialport');
const { WrfcoinIoT } = require('@wrfcoin/iot');
const mqtt = require('mqtt');

class DeviceConnector {
  constructor(options) {
    this.options = options;
    this.wrfcoinIoT = new WrfcoinIoT({
      networkType: options.networkType || 'testnet',
      deviceId: options.deviceId,
      deviceType: options.deviceType || 'weather_station',
      privateKey: options.privateKey,
    });
  }

  async connectSerialDevice() {
    return new Promise((resolve, reject) => {
      this.serialPort = new SerialPort(
        this.options.serialPort,
        {
          baudRate: this.options.baudRate || 9600,
        },
        err => {
          if (err) {
            reject(err);
            return;
          }
          console.log(`Connected to serial device at ${this.options.serialPort}`);
          this.serialPort.on('data', data => this.handleSerialData(data));
          resolve();
        }
      );
    });
  }

  async connectMqttDevice() {
    return new Promise((resolve, reject) => {
      this.mqttClient = mqtt.connect(this.options.mqttBroker, {
        username: this.options.mqttUsername,
        password: this.options.mqttPassword,
      });

      this.mqttClient.on('connect', () => {
        console.log(`Connected to MQTT broker at ${this.options.mqttBroker}`);
        this.mqttClient.subscribe(this.options.mqttTopic);
        resolve();
      });

      this.mqttClient.on('message', (topic, message) => {
        this.handleMqttMessage(topic, message);
      });

      this.mqttClient.on('error', err => {
        reject(err);
      });
    });
  }

  handleSerialData(data) {
    try {
      // Parse the serial data based on your device's format
      // This is an example for a device that sends JSON data
      const dataString = data.toString().trim();

      // Some devices send data in chunks, so we may need to buffer
      if (!dataString.endsWith('}')) {
        this.dataBuffer = (this.dataBuffer || '') + dataString;
        return;
      }

      const fullData = this.dataBuffer ? this.dataBuffer + dataString : dataString;
      this.dataBuffer = '';

      const weatherData = JSON.parse(fullData);
      this.processWeatherData(weatherData);
    } catch (error) {
      console.error('Error processing serial data:', error);
    }
  }

  handleMqttMessage(topic, message) {
    try {
      const weatherData = JSON.parse(message.toString());
      this.processWeatherData(weatherData);
    } catch (error) {
      console.error('Error processing MQTT message:', error);
    }
  }

  async processWeatherData(rawData) {
    try {
      // Transform raw device data to Wrfcoin format
      const wrfcoinData = this.transformData(rawData);

      // Submit the data to the Wrfcoin network
      const result = await this.wrfcoinIoT.submitWeatherData(wrfcoinData);
      console.log(`Data submitted to Wrfcoin network with hash: ${result.hash}`);

      // Optionally record the submission in a local database
      if (this.options.recordSubmissions) {
        this.recordSubmission(wrfcoinData, result.hash);
      }
    } catch (error) {
      console.error('Error submitting weather data:', error);
    }
  }

  transformData(rawData) {
    // Transform the device-specific data format to Wrfcoin format
    // This will vary based on your device's output

    return {
      deviceId: this.options.deviceId,
      timestamp: Date.now(),
      location: this.options.deviceLocation,
      readings: {
        temperature: rawData.temp || rawData.temperature,
        humidity: rawData.hum || rawData.humidity,
        pressure: rawData.pres || rawData.pressure || rawData.barometer,
        windSpeed: rawData.wind || rawData.windSpeed,
        windDirection: rawData.windDir || rawData.windDirection,
        rainfall: rawData.rain || rawData.rainfall,
        solarRadiation: rawData.solar || rawData.solarRadiation,
        uvIndex: rawData.uv || rawData.uvIndex,
      },
      metadata: {
        deviceModel: this.options.deviceModel,
        firmwareVersion: this.options.firmwareVersion,
        batteryLevel: rawData.battery,
      },
    };
  }

  recordSubmission(data, hash) {
    // Implementation for local recording of submissions
    // This could write to a local file or database
    console.log(`Recording submission ${hash} to local storage`);
  }

  async registerDevice() {
    try {
      const result = await this.wrfcoinIoT.registerDevice({
        deviceType: this.options.deviceType,
        deviceModel: this.options.deviceModel,
        location: this.options.deviceLocation,
        capabilities: this.options.capabilities || ['temperature', 'humidity', 'pressure', 'wind'],
      });

      console.log(`Device registered with ID: ${result.deviceId}`);
      return result;
    } catch (error) {
      console.error('Error registering device:', error);
      throw error;
    }
  }
}

module.exports = DeviceConnector;
```

### Step 3: Create an Integration Script

Create a file `weather-station-integration.js`:

```javascript
const DeviceConnector = require('./device-connector');
const fs = require('fs');

// Load configuration
let config;
try {
  const configFile = fs.readFileSync('device-config.json', 'utf8');
  config = JSON.parse(configFile);
} catch (error) {
  console.error('Failed to load configuration file:', error);
  process.exit(1);
}

async function main() {
  try {
    const connector = new DeviceConnector({
      // Device identification
      deviceId: config.deviceId,
      deviceType: config.deviceType,
      deviceModel: config.deviceModel,
      firmwareVersion: config.firmwareVersion,

      // Location information
      deviceLocation: config.location,

      // Connection details
      connectionType: config.connectionType, // 'serial' or 'mqtt'
      serialPort: config.serialPort,
      baudRate: config.baudRate,
      mqttBroker: config.mqttBroker,
      mqttTopic: config.mqttTopic,
      mqttUsername: config.mqttUsername,
      mqttPassword: config.mqttPassword,

      // Wrfcoin network settings
      networkType: config.networkType || 'testnet',
      privateKey: config.privateKey,

      // Additional options
      recordSubmissions: config.recordSubmissions || false,
      capabilities: config.capabilities || [],
    });

    // Register device if not already registered
    if (!config.deviceId) {
      const registrationResult = await connector.registerDevice();

      // Save the device ID to config
      config.deviceId = registrationResult.deviceId;
      fs.writeFileSync('device-config.json', JSON.stringify(config, null, 2));

      console.log(`Device registered with ID: ${config.deviceId}`);
    }

    // Connect to the device based on connection type
    if (config.connectionType === 'serial') {
      await connector.connectSerialDevice();
    } else if (config.connectionType === 'mqtt') {
      await connector.connectMqttDevice();
    } else {
      throw new Error(`Unsupported connection type: ${config.connectionType}`);
    }

    console.log('Weather station integration running...');
    console.log('Press Ctrl+C to exit');
  } catch (error) {
    console.error('Failed to start weather station integration:', error);
    process.exit(1);
  }
}

main();
```

### Step 4: Create a Configuration File

Create a file `device-config.json`:

```json
{
  "deviceType": "weather_station",
  "deviceModel": "AcuRite 5-in-1",
  "firmwareVersion": "1.2.3",
  "location": {
    "latitude": 40.7128,
    "longitude": -74.006,
    "altitude": 10,
    "name": "New York City"
  },
  "connectionType": "serial",
  "serialPort": "/dev/ttyUSB0",
  "baudRate": 9600,
  "mqttBroker": "mqtt://broker.example.com",
  "mqttTopic": "weather/station1",
  "mqttUsername": "username",
  "mqttPassword": "password",
  "networkType": "testnet",
  "privateKey": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
  "recordSubmissions": true,
  "capabilities": ["temperature", "humidity", "pressure", "wind", "rainfall"]
}
```

### Step 5: Run the Integration Script

```bash
node weather-station-integration.js
```

---

## Creating Computational NFTs

This tutorial explains how to create computational NFTs on the Wrfcoin network.

### Prerequisites

- Completed development environment setup
- Basic understanding of NFTs and smart contracts
- Access to Wrfcoin testnet

### Step 1: Install Dependencies

```bash
npm install @wrfcoin/sdk ethers
```

### Step 2: Create Computational NFT Creator

Create a file `computational-nft.js`:

```javascript
const { ethers } = require('ethers');
const { WrfcoinSDK, ComputationalNFT } = require('@wrfcoin/sdk');

class ComputationalNFTCreator {
  constructor(options) {
    this.options = options;

    // Initialize Wrfcoin SDK
    this.wrfcoin = new WrfcoinSDK({
      network: options.network || 'testnet',
      privateKey: options.privateKey,
    });

    // Initialize ComputationalNFT interface
    this.nftInterface = new ComputationalNFT(this.wrfcoin);
  }

  async createDatasetNFT(dataset, computationSpecs) {
    try {
      // Prepare dataset metadata
      const metadata = {
        name: dataset.name,
        description: dataset.description,
        datasetType: dataset.type,
        timeframe: dataset.timeframe,
        resolution: dataset.resolution,
        variables: dataset.variables,
        location: dataset.location,
        dataSize: dataset.size,
        preview: dataset.preview,
        computationSpecifications: computationSpecs,
        creator: this.options.creatorInfo,
      };

      // Prepare the data hash
      const dataHash = await this.hashDataset(dataset.data);

      // Mint the NFT
      const tx = await this.nftInterface.mint({
        metadata,
        dataHash,
        royalties: dataset.royalties || 5, // Default 5% royalties
        computationRights: computationSpecs.allowedOperations,
        accessControl: dataset.accessControl || 'public',
      });

      // Wait for transaction confirmation
      const receipt = await tx.wait();

      // Get the NFT ID from the event logs
      const nftId = this.extractNFTIdFromLogs(receipt.logs);

      return {
        transactionHash: receipt.transactionHash,
        nftId,
        metadata,
      };
    } catch (error) {
      console.error('Error creating computational NFT:', error);
      throw error;
    }
  }

  async hashDataset(data) {
    // This is a placeholder for actual data hashing
    // In a real implementation, you would compute a cryptographic hash
    // of the actual dataset, potentially using IPFS or similar

    // For a real implementation
    return ethers.utils.keccak256(ethers.utils.toUtf8Bytes(JSON.stringify(data)));
  }

  extractNFTIdFromLogs(logs) {
    // Find the NFT minted event in the logs
    // This is a placeholder for the actual implementation
    return '123456789';
  }

  async uploadDataset(dataset) {
    try {
      // This is a placeholder for actual data upload
      // In a real implementation, you might upload to IPFS or
      // a specialized data storage solution

      console.log('Uploading dataset to storage...');

      // Simulate upload time
      await new Promise(resolve => setTimeout(resolve, 2000));

      // Return a mock storage reference
      return {
        storageType: 'ipfs',
        cid:
          'Qm' +
          Math.random().toString(36).substring(2, 15) +
          Math.random().toString(36).substring(2, 15),
        url: `https://ipfs.example.com/${Math.random().toString(36).substring(2, 15)}`,
      };
    } catch (error) {
      console.error('Error uploading dataset:', error);
      throw error;
    }
  }

  async getNFTDetails(nftId) {
    try {
      const details = await this.nftInterface.getDetails(nftId);
      return details;
    } catch (error) {
      console.error('Error getting NFT details:', error);
      throw error;
    }
  }

  async addComputationRight(nftId, operation, address) {
    try {
      const tx = await this.nftInterface.grantComputationRight(nftId, operation, address);
      const receipt = await tx.wait();

      return {
        transactionHash: receipt.transactionHash,
        successful: true,
      };
    } catch (error) {
      console.error('Error adding computation right:', error);
      throw error;
    }
  }

  async executeComputation(nftId, computation) {
    try {
      const tx = await this.nftInterface.executeComputation(nftId, computation);
      const receipt = await tx.wait();

      // Get the computation ID from the event logs
      const computationId = this.extractComputationIdFromLogs(receipt.logs);

      return {
        transactionHash: receipt.transactionHash,
        computationId,
      };
    } catch (error) {
      console.error('Error executing computation:', error);
      throw error;
    }
  }

  extractComputationIdFromLogs(logs) {
    // Find the computation started event in the logs
    // This is a placeholder for the actual implementation
    return '987654321';
  }

  async checkComputationStatus(computationId) {
    try {
      const status = await this.nftInterface.getComputationStatus(computationId);
      return status;
    } catch (error) {
      console.error('Error checking computation status:', error);
      throw error;
    }
  }
}

module.exports = ComputationalNFTCreator;
```

### Step 3: Create Example Usage Script

Create a file `create-weather-nft.js`:

```javascript
const ComputationalNFTCreator = require('./computational-nft');
const fs = require('fs');

async function main() {
  try {
    // Create NFT creator instance
    const creator = new ComputationalNFTCreator({
      network: 'testnet',
      privateKey: process.env.PRIVATE_KEY,
      creatorInfo: {
        name: 'Weather Research Institute',
        id: 'WRI-12345',
        website: 'https://weather-research.example.com',
      },
    });

    // Load sample weather dataset
    const rawData = fs.readFileSync('sample-weather-data.json', 'utf8');
    const weatherData = JSON.parse(rawData);

    // Prepare dataset metadata
    const dataset = {
      name: 'New York City Historical Weather Data 2010-2020',
      description:
        'Comprehensive historical weather dataset for New York City covering years 2010-2020 with hourly resolution.',
      type: 'historical_weather',
      timeframe: {
        start: '2010-01-01T00:00:00Z',
        end: '2020-12-31T23:59:59Z',
      },
      resolution: 'hourly',
      variables: ['temperature', 'humidity', 'pressure', 'windSpeed', 'precipitation'],
      location: {
        name: 'New York City',
        latitude: 40.7128,
        longitude: -74.006,
        radius: 10, // km
      },
      size: weatherData.length,
      preview: weatherData.slice(0, 10), // First 10 records as preview
      data: weatherData,
      royalties: 10, // 10% royalty fee for computations
      accessControl: 'public',
    };

    // Define computation specifications
    const computationSpecs = {
      allowedOperations: ['average', 'max', 'min', 'correlation', 'extremes', 'trends'],
      computationEnvironments: ['python-3.8', 'r-4.0'],
      maxComputationTime: 3600, // seconds
      resources: {
        maxMemory: 4, // GB
        maxCpu: 2, // cores
      },
    };

    console.log('Uploading dataset to storage...');
    const storageRef = await creator.uploadDataset(dataset);

    console.log(`Dataset uploaded to ${storageRef.storageType}:`);
    console.log(`- CID: ${storageRef.cid}`);
    console.log(`- URL: ${storageRef.url}`);

    console.log('Creating computational NFT...');
    const nftResult = await creator.createDatasetNFT(dataset, computationSpecs);

    console.log(`NFT created successfully with ID: ${nftResult.nftId}`);
    console.log(`Transaction hash: ${nftResult.transactionHash}`);

    // Grant computation rights to a specific address
    console.log('Granting computation rights to research partner...');
    const partnerAddress = '0x1234567890123456789012345678901234567890';
    const rightResult = await creator.addComputationRight(
      nftResult.nftId,
      'full_access',
      partnerAddress
    );

    console.log(`Computation rights granted. Transaction hash: ${rightResult.transactionHash}`);

    // Execute a computation on the NFT
    console.log('Executing sample computation...');
    const computationResult = await creator.executeComputation(nftResult.nftId, {
      type: 'average',
      variables: ['temperature'],
      timeframe: {
        start: '2015-01-01T00:00:00Z',
        end: '2015-12-31T23:59:59Z',
      },
      filters: {
        seasons: ['winter', 'summer'],
      },
    });

    console.log(`Computation started with ID: ${computationResult.computationId}`);
    console.log(`Transaction hash: ${computationResult.transactionHash}`);

    // Check computation status after a delay
    console.log('Waiting for computation to complete...');
    setTimeout(async () => {
      const status = await creator.checkComputationStatus(computationResult.computationId);
      console.log(`Computation status: ${status.status}`);

      if (status.status === 'completed') {
        console.log('Computation results:');
        console.log(status.results);
      } else if (status.status === 'failed') {
        console.log(`Computation failed: ${status.error}`);
      }
    }, 10000); // Check after 10 seconds
  } catch (error) {
    console.error('Failed to create computational NFT:', error);
  }
}

main();
```

### Step 4: Create a Sample Dataset

Create a file `sample-weather-data.json` with a small subset of weather data:

```json
[
  {
    "timestamp": "2020-01-01T00:00:00Z",
    "temperature": 2.3,
    "humidity": 76,
    "pressure": 1012.5,
    "windSpeed": 4.2,
    "precipitation": 0.0
  },
  {
    "timestamp": "2020-01-01T01:00:00Z",
    "temperature": 2.1,
    "humidity": 77,
    "pressure": 1012.3,
    "windSpeed": 4.0,
    "precipitation": 0.0
  },
  {
    "timestamp": "2020-01-01T02:00:00Z",
    "temperature": 1.9,
    "humidity": 78,
    "pressure": 1012.0,
    "windSpeed": 3.8,
    "precipitation": 0.0
  }
]
```

### Step 5: Run the Script

```bash
# Set your private key as an environment variable
export PRIVATE_KEY=0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890

# Run the script
node create-weather-nft.js
```

---

## Working with the Climate Oracle Network

This tutorial explains how to interact with the Climate Oracle Network to validate and use weather
data.

### Prerequisites

- Completed development environment setup
- Understanding of oracle networks
- Basic knowledge of cryptographic signatures

### Step 1: Install Required Dependencies

```bash
npm install @wrfcoin/sdk @wrfcoin/oracle ethers
```

### Step 2: Create Oracle Interaction Module

Create a file `climate-oracle.js`:

```javascript
const { ethers } = require('ethers');
const { WrfcoinSDK } = require('@wrfcoin/sdk');
const { OracleClient } = require('@wrfcoin/oracle');

class ClimateOracleInteraction {
  constructor(options = {}) {
    this.wrfcoin = new WrfcoinSDK({
      network: options.network || 'testnet',
      privateKey: options.privateKey,
    });

    this.oracleClient = new OracleClient({
      wrfcoin: this.wrfcoin,
      oracleNetworkUrl: options.oracleNetworkUrl || 'https://oracle.testnet.wrfcoin.network',
    });
  }

  async requestValidation(weatherData) {
    try {
      // Prepare the validation request
      const request = {
        type: 'weather_validation',
        data: weatherData,
        timestamp: Date.now(),
        requester: this.wrfcoin.getAddress(),
        requiredValidators: 3, // Require at least 3 validators
        location: weatherData.location,
        validationCriteria: {
          allowedTemperatureDeviation: 2.0, // °C
          allowedPressureDeviation: 5.0, // hPa
          timeframeToleranceMinutes: 30,
        },
      };

      // Sign the request
      const signature = await this.wrfcoin.signMessage(
        ethers.utils.keccak256(ethers.utils.toUtf8Bytes(JSON.stringify(request)))
      );

      // Submit the validation request to the network
      const validationId = await this.oracleClient.submitValidationRequest(request, signature);

      return {
        validationId,
        request,
      };
    } catch (error) {
      console.error('Error requesting validation:', error);
      throw error;
    }
  }

  async checkValidationStatus(validationId) {
    try {
      const status = await this.oracleClient.getValidationStatus(validationId);
      return status;
    } catch (error) {
      console.error('Error checking validation status:', error);
      throw error;
    }
  }

  async requestForecastData(forecastRequest) {
    try {
      // Prepare the forecast data request
      const request = {
        type: 'weather_forecast',
        location: forecastRequest.location,
        timeframe: forecastRequest.timeframe,
        variables: forecastRequest.variables,
        resolution: forecastRequest.resolution,
        timestamp: Date.now(),
        requester: this.wrfcoin.getAddress(),
        modelPreferences: forecastRequest.modelPreferences || [],
      };

      // Sign the request
      const signature = await this.wrfcoin.signMessage(
        ethers.utils.keccak256(ethers.utils.toUtf8Bytes(JSON.stringify(request)))
      );

      // Submit the forecast request to the network
      const forecastId = await this.oracleClient.submitForecastRequest(request, signature);

      return {
        forecastId,
        request,
      };
    } catch (error) {
      console.error('Error requesting forecast data:', error);
      throw error;
    }
  }

  async getForecastResult(forecastId) {
    try {
      const result = await this.oracleClient.getForecastResult(forecastId);
      return result;
    } catch (error) {
      console.error('Error getting forecast result:', error);
      throw error;
    }
  }

  async getOracleNetworkStatus() {
    try {
      const status = await this.oracleClient.getNetworkStatus();
      return status;
    } catch (error) {
      console.error('Error getting oracle network status:', error);
      throw error;
    }
  }

  async getOracleValidators() {
    try {
      const validators = await this.oracleClient.getValidators();
      return validators;
    } catch (error) {
      console.error('Error getting oracle validators:', error);
      throw error;
    }
  }

  async submitWeatherReport(reportData) {
    try {
      // Prepare the weather report
      const report = {
        type: 'weather_report',
        data: reportData.data,
        location: reportData.location,
        timestamp: Date.now(),
        source: reportData.source,
        reporter: this.wrfcoin.getAddress(),
        metadata: reportData.metadata || {},
      };

      // Sign the report
      const signature = await this.wrfcoin.signMessage(
        ethers.utils.keccak256(ethers.utils.toUtf8Bytes(JSON.stringify(report)))
      );

      // Submit the report to the oracle network
      const reportId = await this.oracleClient.submitWeatherReport(report, signature);

      return {
        reportId,
        report,
      };
    } catch (error) {
      console.error('Error submitting weather report:', error);
      throw error;
    }
  }
}

module.exports = ClimateOracleInteraction;
```

### Step 3: Create Example Usage Script

Create a file `use-climate-oracle.js`:

```javascript
const ClimateOracleInteraction = require('./climate-oracle');

async function main() {
  try {
    // Create Oracle interaction instance
    const oracle = new ClimateOracleInteraction({
      network: 'testnet',
      privateKey: process.env.PRIVATE_KEY,
    });

    // Get oracle network status
    console.log('Getting oracle network status...');
    const networkStatus = await oracle.getOracleNetworkStatus();

    console.log('Oracle Network Status:');
    console.log(`- Active Validators: ${networkStatus.activeValidators}`);
    console.log(`- Total Validators: ${networkStatus.totalValidators}`);
    console.log(`- Network Health: ${networkStatus.health}`);
    console.log(`- Average Response Time: ${networkStatus.averageResponseTime}ms`);

    // Get list of validators
    console.log('\nGetting validator information...');
    const validators = await oracle.getOracleValidators();

    console.log(`Found ${validators.length} validators:`);
    validators.slice(0, 3).forEach((validator, index) => {
      console.log(`\nValidator ${index + 1}:`);
      console.log(`- Address: ${validator.address}`);
      console.log(`- Reputation Score: ${validator.reputation}`);
      console.log(`- Validation Count: ${validator.validationCount}`);
      console.log(`- Specializations: ${validator.specializations.join(', ')}`);
    });

    // Request weather data validation
    console.log('\nRequesting validation for sample weather data...');
    const weatherData = {
      location: {
        name: 'New York City',
        latitude: 40.7128,
        longitude: -74.006,
      },
      timestamp: new Date().toISOString(),
      readings: {
        temperature: 22.5,
        humidity: 65.0,
        pressure: 1013.2,
        windSpeed: 5.7,
        windDirection: 270,
        precipitation: 0.0,
      },
      source: 'WeatherStation NYC-123',
    };

    const validationResult = await oracle.requestValidation(weatherData);
    console.log(`Validation request submitted with ID: ${validationResult.validationId}`);

    // Check validation status after a delay
    console.log('\nWaiting for validation result...');
    setTimeout(async () => {
      const validationStatus = await oracle.checkValidationStatus(validationResult.validationId);

      console.log(`Validation Status: ${validationStatus.status}`);

      if (validationStatus.status === 'completed') {
        console.log('Validation Results:');
        console.log(`- Valid: ${validationStatus.result.valid}`);
        console.log(`- Confidence: ${validationStatus.result.confidence}`);
        console.log(`- Validators Participated: ${validationStatus.result.validatorCount}`);

        if (validationStatus.result.issues && validationStatus.result.issues.length > 0) {
          console.log('- Issues Detected:');
          validationStatus.result.issues.forEach(issue => {
            console.log(`  * ${issue.type}: ${issue.description}`);
          });
        }
      } else if (validationStatus.status === 'pending') {
        console.log('Validation is still in progress...');
      } else {
        console.log(`Validation failed: ${validationStatus.error}`);
      }
    }, 10000); // Check after 10 seconds

    // Request weather forecast data
    console.log('\nRequesting weather forecast...');
    const forecastRequest = {
      location: {
        name: 'New York City',
        latitude: 40.7128,
        longitude: -74.006,
      },
      timeframe: {
        start: new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString(), // Tomorrow
        end: new Date(Date.now() + 5 * 24 * 60 * 60 * 1000).toISOString(), // 5 days from now
      },
      variables: ['temperature', 'precipitation', 'humidity', 'windSpeed'],
      resolution: '1h', // 1-hour resolution
      modelPreferences: ['ecmwf', 'gfs'], // Preferred forecast models
    };

    const forecastResult = await oracle.requestForecastData(forecastRequest);
    console.log(`Forecast request submitted with ID: ${forecastResult.forecastId}`);

    // Check forecast result after a delay
    console.log('\nWaiting for forecast result...');
    setTimeout(async () => {
      const forecast = await oracle.getForecastResult(forecastResult.forecastId);

      console.log(`Forecast Status: ${forecast.status}`);

      if (forecast.status === 'completed') {
        console.log('Forecast Results:');
        console.log(`- Source Model: ${forecast.data.model}`);
        console.log(`- Forecast Points: ${forecast.data.points.length}`);
        console.log(
          `- Time Range: ${forecast.data.timeframe.start} to ${forecast.data.timeframe.end}`
        );
        console.log(`- Confidence Level: ${forecast.data.confidence}`);

        // Display first 3 forecast points
        console.log('\nSample Forecast Data:');
        forecast.data.points.slice(0, 3).forEach((point, index) => {
          console.log(`\nPoint ${index + 1} (${point.timestamp}):`);
          Object.keys(point.values).forEach(variable => {
            console.log(`- ${variable}: ${point.values[variable]}`);
          });
        });
      } else if (forecast.status === 'pending') {
        console.log('Forecast is still being processed...');
      } else {
        console.log(`Forecast request failed: ${forecast.error}`);
      }
    }, 15000); // Check after 15 seconds

    // Submit a weather report
    console.log('\nSubmitting a weather report...');
    const reportData = {
      location: {
        name: 'New York City',
        latitude: 40.7128,
        longitude: -74.006,
      },
      data: {
        temperature: 22.5,
        humidity: 65.0,
        pressure: 1013.2,
        windSpeed: 5.7,
        windDirection: 270,
        precipitation: 0.0,
        clouds: 25, // percentage
        visibility: 10000, // meters
      },
      source: 'Manual Observer NYC-001',
      metadata: {
        observer: 'Weather Research Institute',
        instrumentType: 'Professional Weather Station',
        calibrationDate: '2025-01-15',
      },
    };

    const reportResult = await oracle.submitWeatherReport(reportData);
    console.log(`Weather report submitted with ID: ${reportResult.reportId}`);
  } catch (error) {
    console.error('An error occurred:', error);
  }
}

main();
```

### Step 4: Run the Script

```bash
# Set your private key as an environment variable
export PRIVATE_KEY=0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890

# Run the script
node use-climate-oracle.js
```

---

## Wallet Integration Basics

This tutorial covers the basics of integrating Wrfcoin wallets into your application.

### Prerequisites

- Completed development environment setup
- Basic understanding of crypto wallets
- Knowledge of cryptographic keys

### Step 1: Install Dependencies

```bash
npm install @wrfcoin/sdk ethers bip39
```

### Step 2: Create Wallet Integration Module

Create a file `wallet-integration.js`:

```javascript
const { ethers } = require('ethers');
const { WrfcoinSDK, WrfcoinWallet } = require('@wrfcoin/sdk');
const bip39 = require('bip39');

class WalletIntegration {
  constructor(options = {}) {
    this.network = options.network || 'testnet';
    this.wrfcoin = new WrfcoinSDK({
      network: this.network,
      // No private key needed yet - will be set when wallet is created/imported
    });
  }

  async createWallet() {
    try {
      // Generate a random mnemonic
      const mnemonic = bip39.generateMnemonic();

      // Create wallet from mnemonic
      const wallet = WrfcoinWallet.fromMnemonic(mnemonic);

      // Update SDK with new wallet
      this.wrfcoin.setWallet(wallet);

      return {
        address: wallet.address,
        mnemonic: mnemonic,
        privateKey: wallet.privateKey,
      };
    } catch (error) {
      console.error('Error creating wallet:', error);
      throw error;
    }
  }

  async importWalletFromMnemonic(mnemonic) {
    try {
      // Validate mnemonic
      if (!bip39.validateMnemonic(mnemonic)) {
        throw new Error('Invalid mnemonic phrase');
      }

      // Create wallet from mnemonic
      const wallet = WrfcoinWallet.fromMnemonic(mnemonic);

      // Update SDK with imported wallet
      this.wrfcoin.setWallet(wallet);

      return {
        address: wallet.address,
        privateKey: wallet.privateKey,
      };
    } catch (error) {
      console.error('Error importing wallet from mnemonic:', error);
      throw error;
    }
  }

  async importWalletFromPrivateKey(privateKey) {
    try {
      // Create wallet from private key
      const wallet = new WrfcoinWallet(privateKey);

      // Update SDK with imported wallet
      this.wrfcoin.setWallet(wallet);

      return {
        address: wallet.address,
      };
    } catch (error) {
      console.error('Error importing wallet from private key:', error);
      throw error;
    }
  }

  async getWalletBalance(address) {
    try {
      const balance = await this.wrfcoin.getBalance(address);
      return {
        address,
        balance: ethers.utils.formatEther(balance),
        balanceWei: balance.toString(),
      };
    } catch (error) {
      console.error('Error getting wallet balance:', error);
      throw error;
    }
  }

  async sendTransaction(toAddress, amount, options = {}) {
    try {
      // Validate amount
      const amountWei = ethers.utils.parseEther(amount.toString());

      // Prepare transaction
      const tx = {
        to: toAddress,
        value: amountWei,
        gasLimit: options.gasLimit || 21000,
        gasPrice: options.gasPrice ? ethers.utils.parseUnits(options.gasPrice, 'gwei') : undefined,
        nonce: options.nonce,
        data: options.data || '0x',
      };

      // Send transaction
      const txResponse = await this.wrfcoin.sendTransaction(tx);

      // Wait for transaction confirmation
      const receipt = await txResponse.wait();

      return {
        hash: receipt.transactionHash,
        blockNumber: receipt.blockNumber,
        gasUsed: receipt.gasUsed.toString(),
        status: receipt.status === 1 ? 'success' : 'failed',
      };
    } catch (error) {
      console.error('Error sending transaction:', error);
      throw error;
    }
  }

  async getTransactionHistory(address) {
    try {
      const history = await this.wrfcoin.getTransactionHistory(address);

      // Format the transaction history
      return history.map(tx => ({
        hash: tx.hash,
        from: tx.from,
        to: tx.to,
        value: ethers.utils.formatEther(tx.value),
        timestamp: new Date(tx.timestamp * 1000).toISOString(),
        blockNumber: tx.blockNumber,
        status: tx.status === 1 ? 'success' : 'failed',
        type: tx.type || 'transfer',
      }));
    } catch (error) {
      console.error('Error getting transaction history:', error);
      throw error;
    }
  }

  async generateAddressQRCode(address) {
    try {
      // This would typically use a QR code generation library
      // For this example, we'll just return the URL to a QR code service
      return `https://api.qrserver.com/v1/create-qr-code/?size=200x200&data=${address}`;
    } catch (error) {
      console.error('Error generating QR code:', error);
      throw error;
    }
  }

  async signMessage(message) {
    try {
      const signature = await this.wrfcoin.signMessage(message);
      return {
        message,
        signature,
        address: this.wrfcoin.getAddress(),
      };
    } catch (error) {
      console.error('Error signing message:', error);
      throw error;
    }
  }

  static verifySignature(message, signature, address) {
    try {
      const recoveredAddress = ethers.utils.verifyMessage(message, signature);
      return {
        valid: recoveredAddress.toLowerCase() === address.toLowerCase(),
        recoveredAddress,
      };
    } catch (error) {
      console.error('Error verifying signature:', error);
      return { valid: false, error: error.message };
    }
  }
}

module.exports = WalletIntegration;
```

### Step 3: Create Example Usage Script

Create a file `use-wallet.js`:

```javascript
const WalletIntegration = require('./wallet-integration');

async function main() {
  try {
    // Create wallet integration instance
    const walletIntegration = new WalletIntegration({
      network: 'testnet',
    });

    // Create a new wallet
    console.log('Creating a new wallet...');
    const newWallet = await walletIntegration.createWallet();

    console.log('New Wallet Created:');
    console.log(`- Address: ${newWallet.address}`);
    console.log(`- Mnemonic: ${newWallet.mnemonic}`);
    console.log(`- Private Key: ${newWallet.privateKey}`);

    // Get the wallet balance
    console.log('\nChecking wallet balance...');
    const balanceInfo = await walletIntegration.getWalletBalance(newWallet.address);

    console.log('Wallet Balance:');
    console.log(`- Address: ${balanceInfo.address}`);
    console.log(`- Balance: ${balanceInfo.balance} WRF`);

    // Import a wallet from mnemonic
    console.log('\nImporting wallet from mnemonic...');
    const importedWallet = await walletIntegration.importWalletFromMnemonic(newWallet.mnemonic);

    console.log('Imported Wallet:');
    console.log(`- Address: ${importedWallet.address}`);
    console.log(`- Private Key: ${importedWallet.privateKey}`);

    // Generate QR code for the wallet address
    console.log('\nGenerating QR code for wallet address...');
    const qrCodeUrl = await walletIntegration.generateAddressQRCode(importedWallet.address);

    console.log(`QR Code URL: ${qrCodeUrl}`);

    // Sign a message with the wallet
    console.log('\nSigning a message with the wallet...');
    const messageToSign = 'Hello, Wrfcoin!';
    const signatureResult = await walletIntegration.signMessage(messageToSign);

    console.log('Message Signature:');
    console.log(`- Message: ${signatureResult.message}`);
    console.log(`- Signature: ${signatureResult.signature}`);
    console.log(`- Signing Address: ${signatureResult.address}`);

    // Verify the signature
    console.log('\nVerifying the signature...');
    const verificationResult = WalletIntegration.verifySignature(
      messageToSign,
      signatureResult.signature,
      signatureResult.address
    );

    console.log(`Signature Verification: ${verificationResult.valid ? 'Valid' : 'Invalid'}`);
    if (verificationResult.valid) {
      console.log(`Recovered Address: ${verificationResult.recoveredAddress}`);
    } else {
      console.log(`Verification Error: ${verificationResult.error}`);
    }

    // To send a transaction, we would need to have some balance in the wallet
    // This is just a demonstration of the API
    console.log('\nSending a transaction (simulated)...');
    console.log('(Actual transaction would fail without balance)');

    try {
      const txResult = await walletIntegration.sendTransaction(
        'Wrf1recipientAddressExample12345',
        0.001, // amount in WRF
        { gasPrice: '5' } // gas price in Gwei
      );

      console.log('Transaction Result:');
      console.log(`- Hash: ${txResult.hash}`);
      console.log(`- Block Number: ${txResult.blockNumber}`);
      console.log(`- Gas Used: ${txResult.gasUsed}`);
      console.log(`- Status: ${txResult.status}`);
    } catch (error) {
      console.log(`Transaction failed (expected): ${error.message}`);
    }

    // Get transaction history
    console.log('\nGetting transaction history...');
    try {
      const history = await walletIntegration.getTransactionHistory(newWallet.address);

      console.log(`Found ${history.length} transactions:`);
      history.forEach((tx, index) => {
        console.log(`\nTransaction ${index + 1}:`);
        console.log(`- Hash: ${tx.hash}`);
        console.log(`- From: ${tx.from}`);
        console.log(`- To: ${tx.to}`);
        console.log(`- Value: ${tx.value} WRF`);
        console.log(`- Timestamp: ${tx.timestamp}`);
        console.log(`- Status: ${tx.status}`);
      });
    } catch (error) {
      console.log(`Failed to get transaction history (may be empty): ${error.message}`);
    }
  } catch (error) {
    console.error('An error occurred:', error);
  }
}

main();
```

### Step 4: Run the Script

```bash
node use-wallet.js
```

---

## Implementing Governance Features

This tutorial demonstrates how to integrate with Wrfcoin's governance features.

### Prerequisites

- Completed development environment setup
- Understanding of blockchain governance mechanisms
- Basic knowledge of voting systems

### Step 1: Install Dependencies

```bash
npm install @wrfcoin/sdk @wrfcoin/governance ethers
```

### Step 2: Create Governance Integration Module

Create a file `governance-integration.js`:

```javascript
const { ethers } = require('ethers');
const { WrfcoinSDK } = require('@wrfcoin/sdk');
const { GovernanceClient } = require('@wrfcoin/governance');

class GovernanceIntegration {
  constructor(options = {}) {
    this.wrfcoin = new WrfcoinSDK({
      network: options.network || 'testnet',
      privateKey: options.privateKey,
    });

    this.governanceClient = new GovernanceClient({
      wrfcoin: this.wrfcoin,
    });
  }

  async getGovernanceParameters() {
    try {
      const parameters = await this.governanceClient.getParameters();
      return parameters;
    } catch (error) {
      console.error('Error getting governance parameters:', error);
      throw error;
    }
  }

  async createProposal(proposalData) {
    try {
      // Prepare proposal
      const proposal = {
        title: proposalData.title,
        description: proposalData.description,
        type: proposalData.type,
        changes: proposalData.changes,
        votingPeriod: proposalData.votingPeriod || 7 * 24 * 60 * 60, // Default: 1 week in seconds
        proposer: this.wrfcoin.getAddress(),
      };

      // Submit proposal
      const proposalId = await this.governanceClient.createProposal(proposal);

      return {
        proposalId,
        proposal,
      };
    } catch (error) {
      console.error('Error creating proposal:', error);
      throw error;
    }
  }

  async getProposal(proposalId) {
    try {
      const proposal = await this.governanceClient.getProposal(proposalId);
      return proposal;
    } catch (error) {
      console.error('Error getting proposal:', error);
      throw error;
    }
  }

  async getActiveProposals() {
    try {
      const proposals = await this.governanceClient.getActiveProposals();
      return proposals;
    } catch (error) {
      console.error('Error getting active proposals:', error);
      throw error;
    }
  }

  async getProposalHistory(count = 10, offset = 0) {
    try {
      const history = await this.governanceClient.getProposalHistory(count, offset);
      return history;
    } catch (error) {
      console.error('Error getting proposal history:', error);
      throw error;
    }
  }

  async castVote(proposalId, vote, reason = '') {
    try {
      // Valid votes: 'yes', 'no', 'abstain'
      if (!['yes', 'no', 'abstain'].includes(vote)) {
        throw new Error('Invalid vote. Vote must be one of: yes, no, abstain');
      }

      // Cast vote
      const txHash = await this.governanceClient.castVote(proposalId, vote, reason);

      return {
        proposalId,
        vote,
        txHash,
      };
    } catch (error) {
      console.error('Error casting vote:', error);
      throw error;
    }
  }

  async getVotingPower(address) {
    try {
      const votingPower = await this.governanceClient.getVotingPower(address);
      return {
        address,
        votingPower,
        formattedVotingPower: ethers.utils.formatEther(votingPower),
      };
    } catch (error) {
      console.error('Error getting voting power:', error);
      throw error;
    }
  }

  async getVotesByAddress(address, count = 10, offset = 0) {
    try {
      const votes = await this.governanceClient.getVotesByAddress(address, count, offset);
      return votes;
    } catch (error) {
      console.error('Error getting votes by address:', error);
      throw error;
    }
  }

  async delegateVotingPower(delegatee) {
    try {
      const txHash = await this.governanceClient.delegateVotingPower(delegatee);

      return {
        delegator: this.wrfcoin.getAddress(),
        delegatee,
        txHash,
      };
    } catch (error) {
      console.error('Error delegating voting power:', error);
      throw error;
    }
  }

  async getParameterHistory(parameter, count = 10) {
    try {
      const history = await this.governanceClient.getParameterHistory(parameter, count);
      return history;
    } catch (error) {
      console.error('Error getting parameter history:', error);
      throw error;
    }
  }

  async getGovernancePhase() {
    try {
      const phase = await this.governanceClient.getGovernancePhase();
      return phase;
    } catch (error) {
      console.error('Error getting governance phase:', error);
      throw error;
    }
  }
}

module.exports = GovernanceIntegration;
```

### Step 3: Create Example Usage Script

Create a file `use-governance.js`:

```javascript
const GovernanceIntegration = require('./governance-integration');

async function main() {
  try {
    // Create governance integration instance
    const governance = new GovernanceIntegration({
      network: 'testnet',
      privateKey: process.env.PRIVATE_KEY,
    });

    // Get current governance parameters
    console.log('Getting governance parameters...');
    const parameters = await governance.getGovernanceParameters();

    console.log('Current Governance Parameters:');
    Object.keys(parameters).forEach(key => {
      console.log(`- ${key}: ${parameters[key]}`);
    });

    // Get current governance phase
    console.log('\nGetting current governance phase...');
    const phase = await governance.getGovernancePhase();

    console.log('Current Governance Phase:');
    console.log(`- Phase: ${phase.phase}`);
    console.log(`- Started: ${new Date(phase.startTime * 1000).toISOString()}`);
    if (phase.endTime) {
      console.log(`- Ends: ${new Date(phase.endTime * 1000).toISOString()}`);
    }

    // Get user's voting power
    console.log('\nGetting voting power...');
    const votingPower = await governance.getVotingPower(governance.wrfcoin.getAddress());

    console.log('Voting Power:');
    console.log(`- Address: ${votingPower.address}`);
    console.log(`- Voting Power: ${votingPower.formattedVotingPower}`);

    // Get active proposals
    console.log('\nGetting active proposals...');
    const activeProposals = await governance.getActiveProposals();

    console.log(`Found ${activeProposals.length} active proposals:`);
    activeProposals.forEach((proposal, index) => {
      console.log(`\nProposal ${index + 1}:`);
      console.log(`- ID: ${proposal.id}`);
      console.log(`- Title: ${proposal.title}`);
      console.log(`- Type: ${proposal.type}`);
      console.log(`- Proposer: ${proposal.proposer}`);
      console.log(`- Status: ${proposal.status}`);
      console.log(`- Voting Ends: ${new Date(proposal.votingEnds * 1000).toISOString()}`);
      console.log(`- Yes Votes: ${proposal.votes.yes}`);
      console.log(`- No Votes: ${proposal.votes.no}`);
      console.log(`- Abstain Votes: ${proposal.votes.abstain}`);
    });

    // Create a new proposal
    console.log('\nCreating a new governance proposal...');
    const proposalData = {
      title: 'Increase Validator Reward Rate',
      description:
        'This proposal aims to increase the reward rate for weather data validators from 3% to 5% to incentivize more validators to join the network.',
      type: 'parameter_change',
      changes: [
        {
          parameter: 'validatorRewardRate',
          currentValue: '0.03',
          proposedValue: '0.05',
          justification: 'Increase network security and data quality',
        },
      ],
      votingPeriod: 5 * 24 * 60 * 60, // 5 days in seconds
    };

    const proposalResult = await governance.createProposal(proposalData);
    console.log(`Proposal created with ID: ${proposalResult.proposalId}`);

    // Get proposal details
    console.log('\nGetting details of the created proposal...');
    const proposal = await governance.getProposal(proposalResult.proposalId);

    console.log('Proposal Details:');
    console.log(`- ID: ${proposal.id}`);
    console.log(`- Title: ${proposal.title}`);
    console.log(`- Description: ${proposal.description}`);
    console.log(`- Type: ${proposal.type}`);
    console.log(`- Proposer: ${proposal.proposer}`);
    console.log(`- Status: ${proposal.status}`);
    console.log(`- Created: ${new Date(proposal.created * 1000).toISOString()}`);
    console.log(`- Voting Ends: ${new Date(proposal.votingEnds * 1000).toISOString()}`);

    console.log('\nProposed Changes:');
    proposal.changes.forEach((change, index) => {
      console.log(`\nChange ${index + 1}:`);
      console.log(`- Parameter: ${change.parameter}`);
      console.log(`- Current Value: ${change.currentValue}`);
      console.log(`- Proposed Value: ${change.proposedValue}`);
      console.log(`- Justification: ${change.justification}`);
    });

    // Cast a vote on the proposal
    console.log('\nCasting a vote on the proposal...');
    const voteResult = await governance.castVote(
      proposalResult.proposalId,
      'yes',
      'I support increasing validator rewards to improve network security.'
    );

    console.log('Vote Cast:');
    console.log(`- Proposal ID: ${voteResult.proposalId}`);
    console.log(`- Vote: ${voteResult.vote}`);
    console.log(`- Transaction Hash: ${voteResult.txHash}`);

    // Get votes by address
    console.log('\nGetting votes by address...');
    const votes = await governance.getVotesByAddress(governance.wrfcoin.getAddress());

    console.log(`Found ${votes.length} votes by address:`);
    votes.forEach((vote, index) => {
      console.log(`\nVote ${index + 1}:`);
      console.log(`- Proposal ID: ${vote.proposalId}`);
      console.log(`- Proposal Title: ${vote.proposalTitle}`);
      console.log(`- Vote: ${vote.vote}`);
      console.log(`- Reason: ${vote.reason}`);
      console.log(`- Timestamp: ${new Date(vote.timestamp * 1000).toISOString()}`);
    });

    // Get parameter history
    console.log('\nGetting history for validatorRewardRate parameter...');
    const parameterHistory = await governance.getParameterHistory('validatorRewardRate');

    console.log(`Found ${parameterHistory.length} changes for validatorRewardRate:`);
    parameterHistory.forEach((change, index) => {
      console.log(`\nChange ${index + 1}:`);
      console.log(`- Old Value: ${change.oldValue}`);
      console.log(`- New Value: ${change.newValue}`);
      console.log(`- Changed At: ${new Date(change.timestamp * 1000).toISOString()}`);
      console.log(`- Proposal ID: ${change.proposalId}`);
      console.log(`- Reason: ${change.reason}`);
    });
  } catch (error) {
    console.error('An error occurred:', error);
  }
}

main();
```

### Step 4: Run the Script

```bash
# Set your private key as an environment variable
export PRIVATE_KEY=0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890

# Run the script
node use-governance.js
```

---

## Handling Weather Station Data

This tutorial covers the handling and processing of weather station data formats.

### Prerequisites

- Completed development environment setup
- Understanding of weather data formats
- Basic knowledge of data processing

### Step 1: Install Dependencies

```bash
npm install @wrfcoin/sdk @wrfcoin/weather-data csv-parser fs axios
```

### Step 2: Create Weather Data Handler

Create a file `weather-data-handler.js`:

```javascript
const fs = require('fs');
const csvParser = require('csv-parser');
const axios = require('axios');
const { WeatherDataProcessor, WeatherDataValidator } = require('@wrfcoin/weather-data');

class WeatherDataHandler {
  constructor(options = {}) {
    this.options = options;
    this.processor = new WeatherDataProcessor(options);
    this.validator = new WeatherDataValidator(options);
  }

  async processCSVFile(filePath, options = {}) {
    return new Promise((resolve, reject) => {
      const results = [];

      fs.createReadStream(filePath)
        .pipe(csvParser(options))
        .on('data', data => results.push(data))
        .on('end', () => {
          try {
            // Transform the CSV data into standard Wrfcoin format
            const transformedData = this.transformCSVData(results, options);

            // Validate the transformed data
            const validationResults = this.validateDataset(transformedData);

            resolve({
              originalData: results,
              transformedData,
              validationResults,
            });
          } catch (error) {
            reject(error);
          }
        })
        .on('error', error => reject(error));
    });
  }

  transformCSVData(rawData, options = {}) {
    const mappings = options.mappings || {
      timestamp: 'timestamp',
      temperature: 'temperature',
      humidity: 'humidity',
      pressure: 'pressure',
      windSpeed: 'wind_speed',
      windDirection: 'wind_direction',
      precipitation: 'precipitation',
    };

    const locationInfo = options.location || {
      name: 'Unknown',
      latitude: 0,
      longitude: 0,
    };

    const transformedData = rawData.map(record => {
      const transformedRecord = {
        timestamp: this.parseTimestamp(record[mappings.timestamp]),
        location: locationInfo,
        readings: {},
      };

      // Map the fields based on the provided mappings
      for (const key in mappings) {
        if (key !== 'timestamp' && record[mappings[key]] !== undefined) {
          transformedRecord.readings[key] = this.parseValue(record[mappings[key]]);
        }
      }

      return transformedRecord;
    });

    return transformedData;
  }

  parseTimestamp(value) {
    if (!value) return new Date().toISOString();

    try {
      // Try to parse as ISO string
      const date = new Date(value);
      if (!isNaN(date.getTime())) {
        return date.toISOString();
      }

      // Try to parse as Unix timestamp
      const unixTimestamp = parseInt(value);
      if (!isNaN(unixTimestamp)) {
        return new Date(unixTimestamp * 1000).toISOString();
      }

      // Handle other format patterns as needed

      throw new Error(`Unable to parse timestamp: ${value}`);
    } catch (error) {
      console.error(`Error parsing timestamp "${value}":`, error);
      return new Date().toISOString();
    }
  }

  parseValue(value) {
    if (value === undefined || value === null || value === '') {
      return null;
    }

    // Try to parse as number
    const numValue = parseFloat(value);
    if (!isNaN(numValue)) {
      return numValue;
    }

    // Return as string if can't be parsed as number
    return value;
  }

  validateDataset(dataset) {
    const results = {
      valid: true,
      invalidRecords: [],
      validRecords: [],
      validationSummary: {
        totalRecords: dataset.length,
        validRecords: 0,
        invalidRecords: 0,
      },
    };

    // Validate each record in the dataset
    dataset.forEach((record, index) => {
      const validationResult = this.validator.validate(record);

      if (validationResult.valid) {
        results.validRecords.push({
          index,
          record,
        });
        results.validationSummary.validRecords++;
      } else {
        results.invalidRecords.push({
          index,
          record,
          errors: validationResult.errors,
        });
        results.validationSummary.invalidRecords++;
        results.valid = false;
      }
    });

    return results;
  }

  async fetchWeatherDataFromAPI(apiUrl, apiKey, params = {}) {
    try {
      const response = await axios.get(apiUrl, {
        params: {
          ...params,
          apiKey,
        },
      });

      // Transform the API response data to Wrfcoin format
      const transformedData = this.transformAPIData(response.data, params);

      // Validate the transformed data
      const validationResults = this.validateDataset(transformedData);

      return {
        originalData: response.data,
        transformedData,
        validationResults,
      };
    } catch (error) {
      console.error('Error fetching weather data from API:', error);
      throw error;
    }
  }

  transformAPIData(apiData, options = {}) {
    // This method should be customized based on the specific API format
    // Below is a generic example that assumes a common structure

    if (Array.isArray(apiData)) {
      // If the API returns an array of records
      return apiData.map(record => this.transformAPIRecord(record, options));
    } else if (apiData.data && Array.isArray(apiData.data)) {
      // If the API returns an object with a data array
      return apiData.data.map(record => this.transformAPIRecord(record, options));
    } else if (apiData.results && Array.isArray(apiData.results)) {
      // If the API returns an object with a results array
      return apiData.results.map(record => this.transformAPIRecord(record, options));
    } else {
      // If the API returns a single record
      return [this.transformAPIRecord(apiData, options)];
    }
  }

  transformAPIRecord(record, options = {}) {
    // This method should be customized based on the specific API format
    // Below is a generic example

    const locationInfo = options.location || {
      name: record.location?.name || record.city || 'Unknown',
      latitude: record.location?.latitude || record.lat || 0,
      longitude: record.location?.longitude || record.lon || 0,
    };

    const transformedRecord = {
      timestamp: record.timestamp || record.time || record.date || new Date().toISOString(),
      location: locationInfo,
      readings: {},
    };

    // Map common weather parameters
    if (record.temperature !== undefined)
      transformedRecord.readings.temperature = record.temperature;
    if (record.temp !== undefined) transformedRecord.readings.temperature = record.temp;

    if (record.humidity !== undefined) transformedRecord.readings.humidity = record.humidity;
    if (record.relative_humidity !== undefined)
      transformedRecord.readings.humidity = record.relative_humidity;

    if (record.pressure !== undefined) transformedRecord.readings.pressure = record.pressure;
    if (record.barometric_pressure !== undefined)
      transformedRecord.readings.pressure = record.barometric_pressure;
    if (record.sea_level_pressure !== undefined)
      transformedRecord.readings.pressure = record.sea_level_pressure;

    if (record.wind_speed !== undefined) transformedRecord.readings.windSpeed = record.wind_speed;
    if (record.windSpeed !== undefined) transformedRecord.readings.windSpeed = record.windSpeed;

    if (record.wind_direction !== undefined)
      transformedRecord.readings.windDirection = record.wind_direction;
    if (record.windDirection !== undefined)
      transformedRecord.readings.windDirection = record.windDirection;

    if (record.precipitation !== undefined)
      transformedRecord.readings.precipitation = record.precipitation;
    if (record.precip !== undefined) transformedRecord.readings.precipitation = record.precip;
    if (record.rain !== undefined) transformedRecord.readings.precipitation = record.rain;

    // Ensure timestamp is in ISO format
    transformedRecord.timestamp = this.parseTimestamp(transformedRecord.timestamp);

    return transformedRecord;
  }

  async processDataset(dataset) {
    try {
      // Apply data processing operations to the dataset
      const processed = this.processor.process(dataset, {
        operations: [
          // Clean the data (remove outliers, fill missing values)
          { type: 'clean', options: { fillMissingValues: true, removeOutliers: true } },

          // Normalize temperature data to Celsius if needed
          { type: 'normalize', field: 'temperature', options: { unit: 'celsius' } },

          // Smooth the data to reduce noise
          {
            type: 'smooth',
            fields: ['temperature', 'pressure', 'humidity'],
            options: { windowSize: 3 },
          },

          // Calculate derived values
          {
            type: 'derive',
            derivations: [
              {
                name: 'heatIndex',
                formula: 'calculateHeatIndex',
                inputs: ['temperature', 'humidity'],
              },
              {
                name: 'dewPoint',
                formula: 'calculateDewPoint',
                inputs: ['temperature', 'humidity'],
              },
            ],
          },
        ],
      });

      return processed;
    } catch (error) {
      console.error('Error processing dataset:', error);
      throw error;
    }
  }

  async exportToJSON(data, filePath) {
    return new Promise((resolve, reject) => {
      try {
        fs.writeFileSync(filePath, JSON.stringify(data, null, 2));
        resolve({ success: true, filePath });
      } catch (error) {
        console.error('Error exporting to JSON:', error);
        reject(error);
      }
    });
  }

  async exportToCSV(data, filePath, options = {}) {
    return new Promise((resolve, reject) => {
      try {
        // Flatten the data structure to make it suitable for CSV
        const flattenedData = data.map(record => {
          const flatRecord = {
            timestamp: record.timestamp,
            latitude: record.location.latitude,
            longitude: record.location.longitude,
            location: record.location.name,
          };

          // Add readings fields
          for (const key in record.readings) {
            flatRecord[key] = record.readings[key];
          }

          return flatRecord;
        });

        // Generate CSV header
        const headers = Object.keys(flattenedData[0]).join(',');

        // Generate CSV rows
        const rows = flattenedData.map(record => {
          return Object.values(record)
            .map(value => {
              // Quote strings that contain commas
              if (typeof value === 'string' && value.includes(',')) {
                return `"${value}"`;
              }
              return value;
            })
            .join(',');
        });

        // Combine header and rows
        const csvContent = [headers, ...rows].join('\n');

        // Write to file
        fs.writeFileSync(filePath, csvContent);

        resolve({ success: true, filePath });
      } catch (error) {
        console.error('Error exporting to CSV:', error);
        reject(error);
      }
    });
  }
}

module.exports = WeatherDataHandler;
```

### Step 3: Create Example Usage Script

Create a file `process-weather-data.js`:

```javascript
const WeatherDataHandler = require('./weather-data-handler');
const fs = require('fs');

async function main() {
  try {
    // Create weather data handler
    const handler = new WeatherDataHandler({
      // Optional configuration options
      temperatureUnits: 'celsius',
      validateRanges: true,
    });

    // Create a sample CSV file for testing
    const sampleCSVPath = './sample-weather-data.csv';

    // Check if we need to create the sample file
    if (!fs.existsSync(sampleCSVPath)) {
      console.log('Creating sample weather data CSV file...');

      const csvHeader =
        'timestamp,temperature,humidity,pressure,wind_speed,wind_direction,precipitation\n';
      const csvRows = [
        '2025-05-01T00:00:00Z,22.5,65.0,1013.2,5.7,270,0.0',
        '2025-05-01T01:00:00Z,22.1,66.0,1012.9,5.5,275,0.0',
        '2025-05-01T02:00:00Z,21.8,67.0,1012.7,5.2,280,0.0',
        '2025-05-01T03:00:00Z,21.5,68.0,1012.5,4.8,285,0.0',
        '2025-05-01T04:00:00Z,21.2,69.0,1012.3,4.5,290,0.0',
        '2025-05-01T05:00:00Z,20.9,70.0,1012.1,4.2,295,0.2',
        '2025-05-01T06:00:00Z,20.6,71.0,1011.9,3.9,300,0.5',
        '2025-05-01T07:00:00Z,20.3,72.0,1011.7,3.6,305,0.8',
        '2025-05-01T08:00:00Z,20.0,73.0,1011.5,3.3,310,1.2',
        '2025-05-01T09:00:00Z,19.7,74.0,1011.3,3.0,315,1.5',
      ];

      fs.writeFileSync(sampleCSVPath, csvHeader + csvRows.join('\n'));
      console.log('Sample CSV file created successfully.');
    }

    // Process the CSV file
    console.log('Processing weather data from CSV file...');
    const csvResult = await handler.processCSVFile(sampleCSVPath, {
      // CSV parsing options
      headers: true,
      skipLines: 0,

      // Field mappings
      mappings: {
        timestamp: 'timestamp',
        temperature: 'temperature',
        humidity: 'humidity',
        pressure: 'pressure',
        windSpeed: 'wind_speed',
        windDirection: 'wind_direction',
        precipitation: 'precipitation',
      },

      // Location information
      location: {
        name: 'New York City',
        latitude: 40.7128,
        longitude: -74.006,
      },
    });

    console.log(`Processed ${csvResult.transformedData.length} records from CSV file.`);
    console.log('Validation Results:');
    console.log(`- Total Records: ${csvResult.validationResults.validationSummary.totalRecords}`);
    console.log(`- Valid Records: ${csvResult.validationResults.validationSummary.validRecords}`);
    console.log(
      `- Invalid Records: ${csvResult.validationResults.validationSummary.invalidRecords}`
    );

    if (csvResult.validationResults.invalidRecords.length > 0) {
      console.log('\nIssues found in invalid records:');
      csvResult.validationResults.invalidRecords.forEach(invalidRecord => {
        console.log(`Record at index ${invalidRecord.index}:`);
        invalidRecord.errors.forEach(error => console.log(`- ${error}`));
      });
    }

    // Further process the data
    console.log('\nApplying additional processing to the dataset...');
    const processedData = await handler.processDataset(csvResult.transformedData);

    console.log('Processing completed. Sample processed record:');
    console.log(JSON.stringify(processedData[0], null, 2));

    // Export the processed data
    console.log('\nExporting processed data to JSON and CSV...');

    await handler.exportToJSON(processedData, './processed-weather-data.json');
    console.log('Exported to JSON successfully.');

    await handler.exportToCSV(processedData, './processed-weather-data.csv');
    console.log('Exported to CSV successfully.');

    // Fetch data from an API (simulated)
    console.log('\nFetching weather data from API would look like this:');
    console.log('(Not executing actual API call in this example)');

    console.log(`
    const apiResult = await handler.fetchWeatherDataFromAPI(
      'https://api.weather-provider.com/data',
      'your-api-key-here',
      {
        lat: 40.7128,
        lon: -74.0060,
        start: '2025-05-01',
        end: '2025-05-10'
      }
    );
    
    console.log(\`Processed \${apiResult.transformedData.length} records from API.\`);
    `);
  } catch (error) {
    console.error('An error occurred:', error);
  }
}

main();
```

### Step 4: Run the Script

```bash
node process-weather-data.js
```

---

This comprehensive guide provides a solid foundation for developers looking to integrate with the
Wrfcoin platform. By following these tutorials, you'll be able to implement a wide range of
features, from basic wallet functionality to complex cross-chain communication and computational
NFTs.

For more advanced topics and detailed API references, please refer to the official Wrfcoin
documentation at https://docs.wrfcoin.network.
