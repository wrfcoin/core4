# Smart Contracts API Documentation

**Implements Task 5.1 from Feature Completion Roadmap**

## Overview

The Wrfcoin Smart Contracts API provides comprehensive endpoints for deploying, interacting with,
and managing smart contracts on the Wrfcoin blockchain. The API supports both WASM and
EVM-compatible contracts with built-in climate data validation capabilities.

## Base URL

```
https://api.wrfcoin.network/api/contracts
```

## Authentication

All API requests require authentication via API key or wallet signature:

```http
Authorization: Bearer YOUR_API_KEY
```

Or wallet signature authentication:

```http
X-Wallet-Address: 0x1234...
X-Signature: 0xabcd...
X-Message: timestamp_nonce
```

## Endpoints

### Deploy Contract

Deploy a new smart contract to the Wrfcoin blockchain.

```http
POST /deploy
```

**Parameters:**

- `bytecode` (string, required): Contract bytecode in hex format
- `constructorArgs` (array, optional): Constructor arguments
- `gasLimit` (number, optional): Gas limit for deployment (default: 500000)

**Example Request:**

```json
{
  "bytecode": "0x608060405234801561001057600080fd5b50...",
  "constructorArgs": ["Weather Oracle", "WO", 1000000],
  "gasLimit": 800000
}
```

**Example Response:**

```json
{
  "success": true,
  "data": {
    "contractAddress": "0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234",
    "transactionHash": "0x1234567890abcdef...",
    "gasUsed": 567890,
    "blockNumber": 1234567
  },
  "timestamp": "2025-06-23T01:00:00.000Z"
}
```

### Call Contract Function

Execute a function on a deployed contract.

```http
POST /call
```

**Parameters:**

- `contractAddress` (string, required): Contract address
- `functionName` (string, required): Function to call
- `args` (array, optional): Function arguments
- `gasLimit` (number, optional): Gas limit (default: 100000)

**Example Request:**

```json
{
  "contractAddress": "0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234",
  "functionName": "submitWeatherData",
  "args": [22.5, 65, "New York", 1703721600],
  "gasLimit": 150000
}
```

**Example Response:**

```json
{
  "success": true,
  "data": {
    "result": {
      "dataId": "weather_12345",
      "rewardAmount": "0.125 WRF",
      "validationScore": 95
    },
    "gasUsed": 89456,
    "transactionHash": "0xabcdef1234567890...",
    "events": [
      {
        "address": "0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234",
        "topics": ["0x..."],
        "data": "0x...",
        "event": "WeatherDataSubmitted"
      }
    ]
  },
  "timestamp": "2025-06-23T01:00:00.000Z"
}
```

### Query Contract State

Read contract state without executing a transaction.

```http
GET /query/:contractAddress
```

**Parameters:**

- `contractAddress` (string, required): Contract address
- `functionName` (string, optional): Function to query
- `args` (string, optional): JSON-encoded function arguments

**Example Request:**

```http
GET /query/0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234?functionName=getWeatherData&args=["New York"]
```

**Example Response:**

```json
{
  "success": true,
  "data": {
    "temperature": 22.5,
    "humidity": 65,
    "timestamp": 1703721600,
    "location": "New York",
    "validationScore": 95
  },
  "timestamp": "2025-06-23T01:00:00.000Z"
}
```

### Get Contract Events

Retrieve events emitted by a contract.

```http
GET /events/:contractAddress
```

**Parameters:**

- `contractAddress` (string, required): Contract address
- `fromBlock` (number, optional): Starting block number (default: 0)
- `toBlock` (string|number, optional): Ending block number (default: "latest")
- `eventName` (string, optional): Filter by event name

**Example Request:**

```http
GET /events/0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234?eventName=WeatherDataSubmitted&fromBlock=1000000
```

**Example Response:**

```json
{
  "success": true,
  "data": [
    {
      "address": "0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234",
      "topics": ["0x..."],
      "data": "0x...",
      "blockNumber": 1234567,
      "transactionHash": "0x...",
      "event": "WeatherDataSubmitted",
      "timestamp": "2025-06-23T01:00:00.000Z"
    }
  ],
  "count": 1,
  "timestamp": "2025-06-23T01:00:00.000Z"
}
```

### Estimate Gas

Estimate gas costs for a transaction.

```http
POST /estimate-gas
```

**Parameters:**

- `bytecode` (string, optional): Contract bytecode for deployment
- `functionName` (string, optional): Function name for contract call
- `args` (array, optional): Function arguments

**Example Request:**

```json
{
  "functionName": "submitWeatherData",
  "args": [22.5, 65, "New York", 1703721600]
}
```

**Example Response:**

```json
{
  "success": true,
  "data": {
    "gasEstimate": 125000,
    "gasPrice": 20,
    "estimatedCost": 2500000
  },
  "timestamp": "2025-06-23T01:00:00.000Z"
}
```

### Verify Contract

Verify contract source code.

```http
POST /verify/:contractAddress
```

**Parameters:**

- `contractAddress` (string, required): Contract address
- `sourceCode` (string, required): Contract source code
- `compilerVersion` (string, optional): Compiler version used

**Example Request:**

```json
{
  "sourceCode": "pragma solidity ^0.8.0;\n\ncontract WeatherOracle {\n    ...\n}",
  "compilerVersion": "0.8.19"
}
```

**Example Response:**

```json
{
  "success": true,
  "data": {
    "verified": true,
    "sourceCode": "pragma solidity ^0.8.0;...",
    "compilerVersion": "0.8.19",
    "verifiedAt": "2025-06-23T01:00:00.000Z",
    "matchesDeployedBytecode": true
  },
  "timestamp": "2025-06-23T01:00:00.000Z"
}
```

### List Contracts

Get a list of all deployed contracts.

```http
GET /list
```

**Example Response:**

```json
{
  "success": true,
  "data": [
    {
      "address": "0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234",
      "deployedAt": "2025-06-23T00:30:00.000Z",
      "gasUsed": 567890
    },
    {
      "address": "0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234",
      "deployedAt": "2025-06-23T00:45:00.000Z",
      "gasUsed": 423156
    }
  ],
  "count": 2,
  "timestamp": "2025-06-23T01:00:00.000Z"
}
```

## Climate-Specific Contract Templates

### Weather Oracle Contract

Pre-built template for weather data oracles with built-in validation.

**Deployment Example:**

```json
{
  "template": "weather_oracle",
  "parameters": {
    "updateInterval": 3600,
    "validationThreshold": 0.95,
    "rewardPerValidation": "0.1 WRF"
  }
}
```

### Carbon Credit Contract

Template for carbon credit tokenization and trading.

**Deployment Example:**

```json
{
  "template": "carbon_credit",
  "parameters": {
    "projectId": "PROJ-001",
    "creditType": "VER",
    "totalCredits": 10000,
    "pricePerCredit": "25.0 USDC"
  }
}
```

### Climate Data Validator

Contract template for validating climate data submissions.

**Deployment Example:**

```json
{
  "template": "climate_validator",
  "parameters": {
    "validatorNodes": ["0x123...", "0x456...", "0x789..."],
    "consensusThreshold": 0.67,
    "validationReward": "0.05 WRF"
  }
}
```

## Error Responses

All error responses follow this format:

```json
{
  "success": false,
  "error": "Error description",
  "code": "ERROR_CODE",
  "timestamp": "2025-06-23T01:00:00.000Z"
}
```

**Common Error Codes:**

- `INVALID_BYTECODE`: Invalid contract bytecode
- `INSUFFICIENT_GAS`: Gas limit too low
- `CONTRACT_NOT_FOUND`: Contract address not found
- `FUNCTION_NOT_FOUND`: Function does not exist
- `INVALID_ARGUMENTS`: Invalid function arguments
- `DEPLOYMENT_FAILED`: Contract deployment failed
- `EXECUTION_REVERTED`: Transaction reverted

## Rate Limits

- **General endpoints**: 100 requests per minute
- **Contract deployment**: 10 requests per minute
- **Contract calls**: 50 requests per minute
- **Queries**: 200 requests per minute

## SDKs and Client Libraries

### JavaScript/TypeScript

```bash
npm install @wrfcoin/smart-contracts-sdk
```

### Python

```bash
pip install wrfcoin-contracts
```

### Rust

```toml
[dependencies]
wrfcoin-contracts = "0.1.0"
```

## Webhooks

Subscribe to contract events via webhooks:

```http
POST /webhooks/subscribe
```

**Parameters:**

- `url` (string): Webhook endpoint URL
- `events` (array): Event types to subscribe to
- `contractAddress` (string, optional): Specific contract to monitor

## Examples

### Complete Weather Oracle Deployment

```javascript
const WrfcoinContracts = require('@wrfcoin/smart-contracts-sdk');

const client = new WrfcoinContracts({
  apiKey: 'your-api-key',
  network: 'mainnet',
});

// Deploy weather oracle
const deployment = await client.deployTemplate('weather_oracle', {
  updateInterval: 3600,
  validationThreshold: 0.95,
});

console.log('Contract deployed:', deployment.contractAddress);

// Submit weather data
const result = await client.call(deployment.contractAddress, 'submitWeatherData', [
  22.5, // temperature
  65, // humidity
  'New York',
  Date.now(),
]);

console.log('Data submitted:', result.transactionHash);
```

### Contract Event Monitoring

```javascript
// Subscribe to weather data events
client.subscribeToEvents(contractAddress, ['WeatherDataSubmitted'], event => {
  console.log('New weather data:', event.data);
});
```

## Support

For API support and questions:

- Documentation: https://docs.wrfcoin.network/api/contracts
- GitHub: https://github.com/wrfcoin/contracts-api
- Discord: https://discord.gg/wrfcoin-dev
