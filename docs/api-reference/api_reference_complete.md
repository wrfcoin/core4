# Wrfcoin API Reference

**Date Created:** May 18, 2025  
**Last Edited:** May 26, 2025

## Complete REST API & WebSocket Documentation

**Cross-chain support:** `NOT_AVAILABLE` in the public release
**AI-Enhanced Features:** Advanced validation systems and computational NFT support  
**IoT Integration:** Comprehensive sensor network and environmental data processing

## Base URL

```
Production: https://api.wrfcoin.network/v1
Testnet: https://testnet-api.wrfcoin.network/v1
Cross-Chain: https://bridge.wrfcoin.network/v1
```

## Authentication

```bash
# API Key Authentication
curl -H "Authorization: Bearer YOUR_API_KEY" \
     -H "Content-Type: application/json" \
     https://api.wrfcoin.network/v1/status
```

## Status & Health

### GET /status

Get blockchain network status

```bash
curl https://api.wrfcoin.network/v1/status
```

**Response:**

```json
{
  "status": "online",
  "network": "mainnet",
  "blockHeight": 1545823,
  "lastBlockTime": "2024-05-23T12:34:56Z",
  "averageBlockTime": 2.3,
  "totalTransactions": 45673291,
  "activeValidators": 78,
  "networkUptime": 99.99,
  "version": "v2.1.0"
}
```

### GET /health

Health check endpoint

```bash
curl https://api.wrfcoin.network/v1/health
```

**Response:**

```json
{
  "status": "healthy",
  "timestamp": "2024-05-23T12:34:56Z",
  "services": {
    "blockchain": "healthy",
    "database": "healthy",
    "weather_api": "healthy",
    "cross_chain": "healthy"
  }
}
```

## Account Management

### GET /accounts/{address}

Get account information

```bash
curl https://api.wrfcoin.network/v1/accounts/wrf1abc123def456ghi789
```

**Response:**

```json
{
  "address": "wrf1abc123def456ghi789",
  "balance": {
    "wrf": "1234.567890",
    "cwrf": "89.123456"
  },
  "sequence": 42,
  "accountNumber": 1337,
  "publicKey": "A12B34C56D...",
  "tokens": [
    {
      "denom": "wrf",
      "amount": "1234567890"
    }
  ],
  "delegations": {
    "totalDelegated": "500.000000",
    "validators": [
      {
        "validatorAddress": "wrf1validator123...",
        "amount": "500.000000",
        "rewards": "12.345678"
      }
    ]
  }
}
```

### GET /accounts/{address}/transactions

Get account transaction history

```bash
curl "https://api.wrfcoin.network/v1/accounts/wrf1abc123def456ghi789/transactions?limit=50&offset=0"
```

**Query Parameters:**

- `limit` (optional): Number of transactions to return (default: 50, max: 100)
- `offset` (optional): Number of transactions to skip (default: 0)
- `type` (optional): Filter by transaction type

**Response:**

```json
{
  "transactions": [
    {
      "hash": "ABC123DEF456...",
      "height": 1545820,
      "timestamp": "2024-05-23T12:30:00Z",
      "type": "send",
      "status": "success",
      "fee": "0.001",
      "gasUsed": 21000,
      "gasWanted": 25000,
      "from": "wrf1abc123def456ghi789",
      "to": "wrf1def456ghi789abc123",
      "amount": "100.5",
      "memo": "Payment for weather data"
    }
  ],
  "pagination": {
    "total": 342,
    "limit": 50,
    "offset": 0
  }
}
```

## Transactions

### POST /transactions

Broadcast a transaction

```bash
curl -X POST https://api.wrfcoin.network/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "tx": "base64_encoded_transaction",
    "mode": "sync"
  }'
```

**Request Body:**

```json
{
  "tx": "CowBCocBChwvY29zbW9zLmJhbmsudjFiZXRhMS5Nc2dTZW5kEmcKLGNvc21vczFmeDJkOG...",
  "mode": "sync"
}
```

**Mode Options:**

- `sync`: Wait for transaction to be included in mempool
- `async`: Return immediately
- `block`: Wait for transaction to be included in block

**Response:**

```json
{
  "txhash": "ABC123DEF456GHI789...",
  "code": 0,
  "data": "",
  "raw_log": "[]",
  "logs": [],
  "gas_wanted": "25000",
  "gas_used": "21543"
}
```

### GET /transactions/{hash}

Get transaction details

```bash
curl https://api.wrfcoin.network/v1/transactions/ABC123DEF456GHI789
```

**Response:**

```json
{
  "hash": "ABC123DEF456GHI789...",
  "height": 1545820,
  "timestamp": "2024-05-23T12:30:00Z",
  "tx": {
    "type": "send",
    "from": "wrf1abc123def456ghi789",
    "to": "wrf1def456ghi789abc123",
    "amount": "100.5",
    "fee": "0.001",
    "memo": "Payment for weather data"
  },
  "result": {
    "code": 0,
    "data": "",
    "log": "transaction successful",
    "gasWanted": "25000",
    "gasUsed": "21543"
  },
  "events": [
    {
      "type": "transfer",
      "attributes": [
        {
          "key": "recipient",
          "value": "wrf1def456ghi789abc123"
        },
        {
          "key": "amount",
          "value": "100500000uwrf"
        }
      ]
    }
  ]
}
```

## Blocks

### GET /blocks/latest

Get latest block

```bash
curl https://api.wrfcoin.network/v1/blocks/latest
```

### GET /blocks/{height}

Get block by height

```bash
curl https://api.wrfcoin.network/v1/blocks/1545820
```

**Response:**

```json
{
  "blockId": {
    "hash": "BLOCK123HASH456...",
    "parts": {
      "total": 1,
      "hash": "PARTS123HASH456..."
    }
  },
  "block": {
    "header": {
      "version": {
        "block": "11"
      },
      "chainId": "wrfcoin-mainnet-1",
      "height": "1545820",
      "time": "2024-05-23T12:30:00Z",
      "lastBlockId": {
        "hash": "PREV123HASH456..."
      },
      "proposerAddress": "wrf1validator123...",
      "appHash": "APP123HASH456..."
    },
    "data": {
      "txs": ["base64_encoded_transaction_1", "base64_encoded_transaction_2"]
    },
    "evidence": {
      "evidence": []
    },
    "lastCommit": {
      "height": "1545819",
      "signatures": []
    }
  }
}
```

## Weather Data API

### GET /weather/current

Get current weather conditions

```bash
curl "https://api.wrfcoin.network/v1/weather/current?lat=40.7128&lng=-74.0060"
```

**Query Parameters:**

- `lat` (required): Latitude
- `lng` (required): Longitude
- `radius` (optional): Search radius in kilometers (default: 10)

**Response:**

```json
{
  "location": {
    "latitude": 40.7128,
    "longitude": -74.006,
    "city": "New York",
    "country": "US"
  },
  "timestamp": "2024-05-23T12:30:00Z",
  "temperature": 22.5,
  "humidity": 65,
  "pressure": 1013.25,
  "windSpeed": 5.2,
  "windDirection": 270,
  "visibility": 10.0,
  "uvIndex": 6,
  "cloudCover": 25,
  "precipitationRate": 0.0,
  "dataQuality": "high",
  "sources": [
    {
      "stationId": "NYC_CENTRAL_PARK",
      "distance": 2.1,
      "reliability": 0.95
    }
  ]
}
```

### GET /weather/forecast

Get weather forecast

```bash
curl "https://api.wrfcoin.network/v1/weather/forecast?lat=40.7128&lng=-74.0060&days=7"
```

**Query Parameters:**

- `lat` (required): Latitude
- `lng` (required): Longitude
- `days` (optional): Number of forecast days (default: 7, max: 30)
- `hours` (optional): Hourly forecast if true

**Response:**

```json
{
  "location": {
    "latitude": 40.7128,
    "longitude": -74.006
  },
  "forecast": [
    {
      "date": "2024-05-24",
      "temperatureMax": 25.0,
      "temperatureMin": 18.0,
      "humidity": 70,
      "pressure": 1015.0,
      "windSpeed": 4.8,
      "precipitationProbability": 20,
      "precipitationAmount": 0.5,
      "uvIndex": 7,
      "sunrise": "05:45",
      "sunset": "19:30"
    }
  ],
  "confidence": 0.92,
  "lastUpdated": "2024-05-23T12:00:00Z"
}
```

### GET /weather/historical

Get historical weather data

```bash
curl "https://api.wrfcoin.network/v1/weather/historical?lat=40.7128&lng=-74.0060&start=2024-01-01&end=2024-01-31"
```

**Query Parameters:**

- `lat` (required): Latitude
- `lng` (required): Longitude
- `start` (required): Start date (YYYY-MM-DD)
- `end` (required): End date (YYYY-MM-DD)

**Response:**

```json
{
  "location": {
    "latitude": 40.7128,
    "longitude": -74.006
  },
  "data": [
    {
      "date": "2024-01-01",
      "temperatureMax": 5.0,
      "temperatureMin": -2.0,
      "temperatureMean": 1.5,
      "humidity": 78,
      "pressure": 1020.5,
      "windSpeed": 6.2,
      "precipitationAmount": 2.3,
      "snowAmount": 0.0
    }
  ],
  "statistics": {
    "totalDays": 31,
    "meanTemperature": 2.1,
    "totalPrecipitation": 45.6,
    "maxTemperature": 8.5,
    "minTemperature": -5.2
  }
}
```

### POST /weather/submit

Submit weather data (requires authentication)

```bash
curl -X POST https://api.wrfcoin.network/v1/weather/submit \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "stationId": "PERSONAL_STATION_001",
    "latitude": 40.7128,
    "longitude": -74.0060,
    "timestamp": "2024-05-23T12:30:00Z",
    "temperature": 22.5,
    "humidity": 65,
    "pressure": 1013.25,
    "signature": "weather_data_signature"
  }'
```

## Computational NFTs

### GET /nfts/computational

Get computational NFTs

```bash
curl "https://api.wrfcoin.network/v1/nfts/computational?limit=20&offset=0"
```

**Response:**

```json
{
  "nfts": [
    {
      "id": "nft_comp_123",
      "name": "Climate Model v2.1",
      "description": "Advanced climate prediction model",
      "creator": "wrf1creator123...",
      "computationType": "climate_modeling",
      "inputSchema": {
        "temperature": "number",
        "humidity": "number",
        "pressure": "number"
      },
      "resources": {
        "cpu": 2,
        "memory": 1024,
        "storage": 100
      },
      "executionCount": 1547,
      "averageExecutionTime": 2.3,
      "lastExecuted": "2024-05-23T11:45:00Z",
      "reputation": 0.95
    }
  ]
}
```

### POST /nfts/computational/{id}/execute

Execute computational NFT

```bash
curl -X POST https://api.wrfcoin.network/v1/nfts/computational/nft_comp_123/execute \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "inputs": {
      "temperature": 25.0,
      "humidity": 70,
      "pressure": 1015.0
    },
    "gasLimit": 100000
  }'
```

**Response:**

```json
{
  "executionId": "exec_456",
  "status": "completed",
  "startTime": "2024-05-23T12:30:00Z",
  "endTime": "2024-05-23T12:30:02Z",
  "gasUsed": 75432,
  "results": {
    "prediction": "sunny",
    "confidence": 0.87,
    "precipitationProbability": 15
  },
  "metadata": {
    "version": "2.1",
    "modelType": "ensemble",
    "dataQuality": "high"
  }
}
```

## Cross-Chain Bridge

`NOT_AVAILABLE` in the public release. No discovery, transfer, or status API is
published.

## Staking & Validators

### GET /staking/validators

Get validator list

```bash
curl "https://api.wrfcoin.network/v1/staking/validators?status=active"
```

**Response:**

```json
{
  "validators": [
    {
      "operatorAddress": "wrf1validator123...",
      "consensusPubkey": "wrf1consensuspubkey123...",
      "moniker": "Validator One",
      "details": "Professional validator service",
      "website": "https://validator1.com",
      "commission": {
        "rate": "0.05",
        "maxRate": "0.10",
        "maxChangeRate": "0.01"
      },
      "status": "bonded",
      "tokens": "1000000.0",
      "delegatorShares": "1000000.0",
      "bondHeight": "100000",
      "unbondingHeight": "0",
      "jailed": false,
      "uptime": 0.9987
    }
  ]
}
```

### POST /staking/delegate

Delegate tokens to validator

```bash
curl -X POST https://api.wrfcoin.network/v1/staking/delegate \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "validatorAddress": "wrf1validator123...",
    "amount": "1000.0"
  }'
```

## WebSocket API

### Connection

```javascript
const ws = new WebSocket('wss://api.wrfcoin.network/ws');

ws.onopen = function () {
  // Authenticate
  ws.send(
    JSON.stringify({
      type: 'authenticate',
      apiKey: 'YOUR_API_KEY',
    })
  );
};
```

### Subscribe to Events

```javascript
// Subscribe to new blocks
ws.send(
  JSON.stringify({
    type: 'subscribe',
    channel: 'blocks',
  })
);

// Subscribe to account updates
ws.send(
  JSON.stringify({
    type: 'subscribe',
    channel: 'account',
    address: 'wrf1abc123def456ghi789',
  })
);

// Subscribe to weather alerts
ws.send(
  JSON.stringify({
    type: 'subscribe',
    channel: 'weather_alerts',
    region: 'northeast_us',
  })
);
```

### Event Messages

```javascript
ws.onmessage = function (event) {
  const data = JSON.parse(event.data);

  switch (data.type) {
    case 'new_block':
      console.log('New block:', data.block);
      break;
    case 'account_update':
      console.log('Account updated:', data.account);
      break;
    case 'weather_alert':
      console.log('Weather alert:', data.alert);
      break;
    case 'transaction_confirmed':
      console.log('Transaction confirmed:', data.transaction);
      break;
  }
};
```

## Rate Limits

### Standard Limits

- **Free Tier**: 1,000 requests/hour
- **Basic**: 10,000 requests/hour
- **Professional**: 100,000 requests/hour
- **Enterprise**: 1,000,000 requests/hour

### Rate Limit Headers

```
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 999
X-RateLimit-Reset: 1621507200
```

## Error Codes

### HTTP Status Codes

- `200` - Success
- `400` - Bad Request
- `401` - Unauthorized
- `403` - Forbidden
- `404` - Not Found
- `429` - Too Many Requests
- `500` - Internal Server Error
- `503` - Service Unavailable

### Error Response Format

```json
{
  "error": {
    "code": "INVALID_ADDRESS",
    "message": "The provided address is not valid",
    "details": "Address must start with 'wrf1' and be 42 characters long"
  }
}
```

This comprehensive API reference covers all core functionality needed for enterprise integration and
developer adoption.
