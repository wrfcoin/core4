# Wrfcoin Integration Guide

This guide provides information on how to integrate with the Wrfcoin blockchain platform and its
various services.

## Table of Contents

1. [Introduction](#introduction)
2. [API Integration](#api-integration)
3. [WebSocket Integration](#websocket-integration)
4. [Smart Contract Integration](#smart-contract-integration)
5. [SDK Integration](#sdk-integration)
6. [Authentication & Wallets](#authentication--wallets)
7. [Weather Data Integration](#weather-data-integration)
8. [Example Integration Flows](#example-integration-flows)

## Introduction

Wrfcoin is a specialized blockchain platform focused on weather, research, and forecasting (WRF)
data. It offers various integration points for applications to interact with its services:

- **REST APIs**: For querying blockchain data, wallet management, and other services
- **WebSockets**: For real-time blockchain updates
- **SDKs**: For programmatic access to platform features
- **Smart Contracts**: For custom on-chain logic and data processing

## API Integration

### Base URL

```
https://api.wrfcoin.network/v1
```

For development and testing, use the testnet URL:

```
https://testnet-api.wrfcoin.network/v1
```

### Authentication

Most API endpoints require authentication. Wrfcoin supports:

1. **API Key Authentication**: For server-to-server communications
2. **Wallet Authentication**: For user-specific operations
3. **Anonymous Access**: For public blockchain data

#### API Key Authentication

Request an API key through the developer portal, then include it in the request headers:

```javascript
const response = await fetch('https://api.wrfcoin.network/v1/blockchain/status', {
  headers: {
    'X-API-Key': 'your-api-key-here',
  },
});
```

#### Wallet Authentication

For user-centric operations, use wallet-based authentication:

```javascript
// Sign message with user's wallet
const signature = await wallet.signMessage('Authentication request for Wrfcoin API');

const response = await fetch('https://api.wrfcoin.network/v1/wallet/transactions', {
  headers: {
    'X-Wallet-Address': wallet.address,
    'X-Wallet-Signature': signature,
  },
});
```

### Key Endpoints

| Endpoint                          | Method | Description               |
| --------------------------------- | ------ | ------------------------- |
| `/blockchain/status`              | GET    | Current blockchain status |
| `/blockchain/blocks`              | GET    | List of recent blocks     |
| `/blockchain/blocks/{hash}`       | GET    | Block details by hash     |
| `/blockchain/transactions/{txid}` | GET    | Transaction details by ID |
| `/wallet/{address}`               | GET    | Wallet information        |
| `/wallet/{address}/transactions`  | GET    | Wallet transactions       |
| `/wallet/{address}/balance`       | GET    | Wallet balance            |
| `/weather/data`                   | GET    | Query weather data        |
| `/weather/stations`               | GET    | List weather stations     |
| `/nft/marketplace`                | GET    | NFT marketplace listings  |

### Error Handling

The API uses standard HTTP status codes. Errors return a JSON structure:

```json
{
  "error": true,
  "code": "INVALID_PARAMETER",
  "message": "The provided address is not valid",
  "details": {
    "parameter": "address"
  }
}
```

## WebSocket Integration

For real-time updates, use the WebSocket API:

```javascript
// Using standard WebSocket
const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
const wsUrl = `${protocol}//${window.location.host}/ws`;
const socket = new WebSocket(wsUrl);

// Event listeners
socket.onopen = () => {
  console.log('Connected to Wrfcoin WebSocket');

  // Subscribe to channels
  socket.send(
    JSON.stringify({
      action: 'subscribe',
      channels: ['blockchain', 'transactions'],
    })
  );
};

socket.onmessage = event => {
  const data = JSON.parse(event.data);
  console.log('Received update:', data);

  // Handle different event types
  if (data.event === 'BLOCK_CREATED') {
    updateBlockchainUI(data.data);
  } else if (data.event === 'TRANSACTION_CREATED') {
    notifyNewTransaction(data.data);
  }
};

socket.onclose = () => {
  console.log('Disconnected from WebSocket');
};
```

### Available Channels

| Channel        | Events                                         | Description              |
| -------------- | ---------------------------------------------- | ------------------------ |
| `blockchain`   | `BLOCK_CREATED`, `STATE_UPDATE`                | Blockchain state updates |
| `transactions` | `TRANSACTION_CREATED`, `TRANSACTION_CONFIRMED` | Transaction events       |
| `governance`   | `PROPOSAL_CREATED`, `VOTE_CAST`                | Governance events        |
| `weather`      | `DATA_SUBMITTED`, `STATION_ADDED`              | Weather data events      |
| `marketplace`  | `NFT_LISTED`, `NFT_SOLD`                       | NFT marketplace events   |

## Smart Contract Integration

Wrfcoin supports custom smart contracts for specialized logic, particularly for weather data
processing and validation.

### Contract Development

Smart contracts can be written in Solidity or using Wrfcoin's specialized Weather Contract Format
(WCF).

Example weather data processing contract:

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import "@wrfcoin/contracts/interfaces/IWeatherData.sol";
import "@wrfcoin/contracts/interfaces/IOracle.sol";

contract TemperatureAlert {
    IWeatherData public weatherData;
    IOracle public oracle;

    address public owner;
    string public location;
    int public temperatureThreshold;

    event TemperatureExceeded(string location, int temperature, uint timestamp);

    constructor(address _weatherData, address _oracle, string memory _location, int _threshold) {
        weatherData = IWeatherData(_weatherData);
        oracle = IOracle(_oracle);
        owner = msg.sender;
        location = _location;
        temperatureThreshold = _threshold;
    }

    function checkTemperature() external {
        (int temperature, uint timestamp) = weatherData.getLatestTemperature(location);

        // Verify data against oracle
        require(oracle.verifyTemperature(location, temperature, timestamp), "Data not verified by oracle");

        if (temperature > temperatureThreshold) {
            emit TemperatureExceeded(location, temperature, timestamp);
        }
    }
}
```

### Deploying Contracts

Contracts can be deployed using the Wrfcoin Web UI or programmatically via the SDK:

```javascript
const { WrfcoinSDK } = require('@wrfcoin/sdk');

async function deployContract() {
  const sdk = new WrfcoinSDK({
    network: 'testnet',
    apiKey: 'your-api-key',
  });

  // Connect to wallet
  await sdk.connectWallet(privateKey);

  // Deploy contract
  const contract = await sdk.deployContract({
    name: 'TemperatureAlert',
    source: contractSource,
    args: [weatherDataAddress, oracleAddress, 'New York', 95],
  });

  console.log('Contract deployed:', contract.address);
}
```

## SDK Integration

Wrfcoin provides SDKs for different languages:

- JavaScript/TypeScript
- Python
- Rust
- Java

### JavaScript SDK Example

```bash
# Install the SDK
npm install @wrfcoin/sdk
```

```javascript
const { WrfcoinSDK } = require('@wrfcoin/sdk');

// Initialize SDK
const sdk = new WrfcoinSDK({
  network: 'mainnet', // or 'testnet'
  apiKey: 'your-api-key-here',
});

// Query blockchain status
async function getBlockchainStatus() {
  const status = await sdk.blockchain.getStatus();
  console.log('Current block height:', status.blockHeight);
  console.log('Network hashrate:', status.hashrate);
}

// Create a wallet
async function createWallet() {
  const wallet = await sdk.wallet.create();
  console.log('New wallet address:', wallet.address);
  console.log('Private key (keep safe!):', wallet.privateKey);
}

// Submit weather data
async function submitWeatherData(stationId, data) {
  // Connect to wallet for signing
  await sdk.connectWallet(privateKey);

  const result = await sdk.weather.submitData({
    stationId,
    temperature: data.temperature,
    humidity: data.humidity,
    pressure: data.pressure,
    timestamp: Date.now(),
  });

  console.log('Weather data submitted:', result.txid);
}
```

### Python SDK Example

```bash
# Install the SDK
pip install wrfcoin-sdk
```

```python
from wrfcoin_sdk import WrfcoinSDK

# Initialize SDK
sdk = WrfcoinSDK(
    network='mainnet',  # or 'testnet'
    api_key='your-api-key-here'
)

# Connect wallet
sdk.connect_wallet(private_key='your-private-key')

# Query blockchain data
block = sdk.blockchain.get_block(height=1000)
print(f"Block hash: {block.hash}")
print(f"Transactions: {len(block.transactions)}")

# Create and send transaction
tx = sdk.wallet.create_transaction(
    recipient='WRF1AbCdEfGhIjKlMnOpQrStUvWxYz12345678',
    amount=10.5,
    memo='Test transaction'
)

tx_result = sdk.wallet.send_transaction(tx)
print(f"Transaction sent: {tx_result.txid}")
```

## Authentication & Wallets

Wrfcoin supports two authentication methods:

1. **Anonymous Access**: Create and use wallets without account association
2. **Account Association**: Link wallets to user accounts for enhanced features

### Creating a Wallet

```javascript
const { WrfcoinSDK } = require('@wrfcoin/sdk');

const sdk = new WrfcoinSDK({ network: 'testnet' });

// Create new wallet
const wallet = await sdk.wallet.create();

// Or import existing wallet from private key
const importedWallet = await sdk.wallet.import(privateKey);

// Or import from mnemonic
const mnemonicWallet = await sdk.wallet.importFromMnemonic(mnemonic);
```

### Account Association (Optional)

To link a wallet with a user account:

```javascript
// User must be authenticated first
await sdk.auth.loginWithEmail(email, password);

// Associate wallet
await sdk.wallet.associate(wallet.address, {
  label: 'My Main Wallet',
});

// Get all associated wallets
const wallets = await sdk.wallet.getAssociatedWallets();
```

## Weather Data Integration

Weather data can be queried or submitted through various APIs.

### Querying Weather Data

```javascript
// Initialize SDK
const sdk = new WrfcoinSDK({ network: 'mainnet', apiKey: 'your-api-key' });

// Query historical temperature data for a location
const tempData = await sdk.weather.getTemperature({
  location: 'New York',
  startTime: new Date('2025-01-01'),
  endTime: new Date('2025-01-31'),
});

// Query data from a specific station
const stationData = await sdk.weather.getStationData({
  stationId: 'WS-NYC-001',
  metrics: ['temperature', 'humidity', 'pressure'],
  limit: 100,
});

// Get aggregated data for a region
const regionalData = await sdk.weather.getRegionalData({
  region: 'Northeast US',
  metric: 'temperature',
  aggregation: 'average',
  interval: 'daily',
  startDate: new Date('2025-01-01'),
  endDate: new Date('2025-01-31'),
});
```

### Submitting Weather Data

Weather stations can submit data to the blockchain:

```javascript
// Connect to wallet for signing
await sdk.connectWallet(stationPrivateKey);

// Submit data
const result = await sdk.weather.submitData({
  stationId: 'WS-NYC-001',
  temperature: 22.5,
  humidity: 65,
  pressure: 1013.2,
  windSpeed: 12.3,
  windDirection: 270,
  precipitation: 0,
  timestamp: Date.now(),
  // Optional metadata
  metadata: {
    instrumentType: 'Davis Vantage Pro2',
    calibrationDate: '2025-01-15',
  },
});

console.log('Data submitted in transaction:', result.txid);
```

## Example Integration Flows

### Building a Weather Dashboard App

1. **Initialize SDK and fetch data**:

```javascript
const { WrfcoinSDK } = require('@wrfcoin/sdk');

const sdk = new WrfcoinSDK({
  network: 'mainnet',
  apiKey: 'your-api-key',
});

async function fetchDashboardData() {
  // Get blockchain status
  const blockchainStatus = await sdk.blockchain.getStatus();

  // Get weather data for multiple cities
  const weatherData = await Promise.all([
    sdk.weather.getLatestData({ location: 'New York' }),
    sdk.weather.getLatestData({ location: 'London' }),
    sdk.weather.getLatestData({ location: 'Tokyo' }),
  ]);

  // Get forecast data
  const forecast = await sdk.weather.getForecast({
    location: 'New York',
    days: 5,
  });

  return {
    blockchainStatus,
    weatherData,
    forecast,
  };
}
```

2. **Set up real-time updates**:

```javascript
function setupRealtimeUpdates() {
  const socket = sdk.createWebSocket();

  socket.onmessage = event => {
    const data = JSON.parse(event.data);

    // Update UI based on event type
    if (data.channel === 'weather' && data.event === 'DATA_SUBMITTED') {
      updateWeatherDisplay(data.data);
    } else if (data.channel === 'blockchain' && data.event === 'BLOCK_CREATED') {
      updateBlockchainStats(data.data);
    }
  };

  // Subscribe to channels
  socket.send(
    JSON.stringify({
      action: 'subscribe',
      channels: ['weather', 'blockchain'],
    })
  );
}
```

### Building a Weather Station Integration

1. **Set up station credentials**:

```javascript
const { WrfcoinSDK } = require('@wrfcoin/sdk');

const sdk = new WrfcoinSDK({
  network: 'mainnet',
  apiKey: 'your-api-key',
});

// Load station private key from secure storage
const stationPrivateKey = process.env.STATION_PRIVATE_KEY;
const stationId = process.env.STATION_ID;

// Connect wallet
await sdk.connectWallet(stationPrivateKey);
```

2. **Implement data submission loop**:

```javascript
// Function to read from weather sensors
async function readSensorData() {
  // Implement your sensor reading logic here
  return {
    temperature: 22.5,
    humidity: 65,
    pressure: 1013.2,
    windSpeed: 12.3,
    windDirection: 270,
    precipitation: 0,
  };
}

// Periodic data submission
async function submitDataPeriodically() {
  try {
    // Read sensor data
    const sensorData = await readSensorData();

    // Submit to blockchain
    const result = await sdk.weather.submitData({
      stationId,
      ...sensorData,
      timestamp: Date.now(),
    });

    console.log(`Data submitted successfully: ${result.txid}`);
  } catch (error) {
    console.error('Error submitting data:', error);
  }

  // Schedule next submission
  setTimeout(submitDataPeriodically, 5 * 60 * 1000); // Every 5 minutes
}

// Start the submission loop
submitDataPeriodically();
```

These examples should help you get started with integrating your applications with the Wrfcoin
platform. For more detailed information, refer to the API documentation and SDK references.
