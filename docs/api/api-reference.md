# Wrfcoin API Reference

**Date Created:** May 18, 2025  
**Last Edited:** May 26, 2025

Complete reference for the Wrfcoin Climate Blockchain Platform APIs featuring 13 cross-chain bridge
protocols, AI-enhanced validation systems, and comprehensive IoT sensor network integrations.

## 🌐 Base URLs

**Testnet:** `https://api-testnet.wrfcoin.org/v1`  
**Mainnet:** `https://api.wrfcoin.org/v1`  
**Cross-Chain Bridge:** `https://bridge.wrfcoin.org/v1`  
**IoT Sensor Networks:** `https://iot.wrfcoin.org/v1`

## 🔐 Authentication

All API requests require authentication using your API key:

```bash
curl -H "Authorization: Bearer YOUR_API_KEY" \
     -H "Content-Type: application/json" \
     https://api.wrfcoin.org/v1/weather/current
```

**Get your API key:** [developer.wrfcoin.org](https://developer.wrfcoin.org)

## 🌡️ Weather Data API

### Get Current Weather Conditions

Retrieve real-time weather data for any location.

```http
GET /weather/current
```

**Parameters:**

- `lat` (required) - Latitude (-90 to 90)
- `lng` (required) - Longitude (-180 to 180)
- `units` (optional) - `metric` (default), `imperial`, `kelvin`

**Example Request:**

```bash
curl "https://api.wrfcoin.org/v1/weather/current?lat=40.7128&lng=-74.0060&units=metric" \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Example Response:**

```json
{
  "data": {
    "temperature": 22.5,
    "humidity": 65,
    "pressure": 1013.25,
    "windSpeed": 4.2,
    "windDirection": 180,
    "visibility": 10000,
    "cloudCover": 25,
    "uvIndex": 6,
    "conditions": "partly_cloudy",
    "location": {
      "latitude": 40.7128,
      "longitude": -74.006,
      "city": "New York",
      "country": "US",
      "timezone": "America/New_York"
    }
  },
  "metadata": {
    "source": "NOAA",
    "quality": "high",
    "timestamp": "2024-01-15T14:30:00Z",
    "blockHash": "0x1234...abcd",
    "transactionHash": "0x5678...efgh"
  }
}
```

### Get Historical Weather Data

Retrieve historical weather data for analysis and validation.

```http
GET /weather/historical
```

**Parameters:**

- `lat` (required) - Latitude
- `lng` (required) - Longitude
- `start` (required) - Start date (ISO 8601)
- `end` (required) - End date (ISO 8601)
- `parameters` (optional) - Comma-separated list: `temperature,humidity,pressure`
- `interval` (optional) - `hourly` (default), `daily`

**Example Request:**

```bash
curl "https://api.wrfcoin.org/v1/weather/historical?lat=40.7128&lng=-74.0060&start=2024-01-01T00:00:00Z&end=2024-01-02T00:00:00Z&interval=hourly" \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Example Response:**

```json
{
  "data": [
    {
      "timestamp": "2024-01-01T00:00:00Z",
      "temperature": 2.1,
      "humidity": 78,
      "pressure": 1015.3,
      "blockHeight": 1234567
    },
    {
      "timestamp": "2024-01-01T01:00:00Z",
      "temperature": 1.8,
      "humidity": 80,
      "pressure": 1015.1,
      "blockHeight": 1234568
    }
  ],
  "metadata": {
    "totalRecords": 24,
    "averageTemperature": 2.5,
    "dataQuality": 0.95
  }
}
```

### Submit Weather Data

Submit weather data from your sensors to the blockchain.

```http
POST /weather/submit
```

**Request Body:**

```json
{
  "readings": [
    {
      "latitude": 40.7128,
      "longitude": -74.006,
      "temperature": 22.5,
      "humidity": 65,
      "pressure": 1013.25,
      "timestamp": "2024-01-15T14:30:00Z",
      "sensorId": "sensor-001",
      "quality": "high"
    }
  ],
  "metadata": {
    "source": "IoT Weather Station",
    "calibrationDate": "2024-01-01T00:00:00Z",
    "accuracy": "+/- 0.1°C"
  }
}
```

**Example Response:**

```json
{
  "success": true,
  "transactionHash": "0x1234...abcd",
  "blockHash": "0x5678...efgh",
  "recordsProcessed": 1,
  "gasUsed": 45000,
  "gasOptimization": "15.7% reduction applied"
}
```

## 🧮 Computational API

### Execute Climate Computation

Run climate models and computations on the blockchain.

```http
POST /compute/execute
```

**Request Body:**

```json
{
  "computationType": "weather_prediction",
  "inputData": {
    "location": {
      "latitude": 40.7128,
      "longitude": -74.006
    },
    "historicalPeriod": "30 days",
    "predictionHorizon": "7 days",
    "parameters": ["temperature", "precipitation", "windSpeed"]
  },
  "model": "WRF-4.2.1",
  "priority": "high"
}
```

**Example Response:**

```json
{
  "computationId": "comp_1234567890",
  "status": "processing",
  "estimatedCompletion": "2024-01-15T15:00:00Z",
  "gasEstimate": 150000,
  "queuePosition": 2
}
```

### Get Computation Results

Retrieve results from a climate computation.

```http
GET /compute/results/{computationId}
```

**Example Response:**

```json
{
  "computationId": "comp_1234567890",
  "status": "completed",
  "results": {
    "forecast": [
      {
        "date": "2024-01-16",
        "temperature": {
          "min": 18.5,
          "max": 25.2,
          "confidence": 0.89
        },
        "precipitation": {
          "probability": 0.25,
          "amount": 2.1,
          "confidence": 0.82
        }
      }
    ],
    "accuracy": 0.91,
    "processingTime": 45000,
    "energyUsed": "0.125 kWh"
  },
  "blockchain": {
    "transactionHash": "0x9abc...def0",
    "blockHeight": 1234570,
    "gasUsed": 142000
  }
}
```

## 💳 Carbon Credits API

### Get Carbon Credit Information

```http
GET /carbon/credits/{creditId}
```

**Example Response:**

```json
{
  "creditId": "CC_001234567",
  "project": {
    "name": "Amazon Reforestation Initiative",
    "location": "Brazil",
    "methodology": "VCS",
    "vintage": 2024
  },
  "carbonReduced": 1000,
  "verificationStatus": "verified",
  "owner": "0x1234...abcd",
  "price": 25.5,
  "currency": "USD",
  "blockchain": {
    "tokenId": "12345",
    "contractAddress": "0x5678...efgh",
    "standard": "ERC-721"
  }
}
```

### Trade Carbon Credits

```http
POST /carbon/trade
```

**Request Body:**

```json
{
  "creditId": "CC_001234567",
  "buyer": "0x9abc...def0",
  "seller": "0x1234...abcd",
  "quantity": 100,
  "pricePerCredit": 25.5,
  "currency": "USD"
}
```

## 🔗 Blockchain API

### Get Block Information

```http
GET /blockchain/blocks/{blockNumber}
```

**Example Response:**

```json
{
  "blockNumber": 1234567,
  "blockHash": "0x1234...abcd",
  "timestamp": "2024-01-15T14:30:00Z",
  "transactions": [
    {
      "hash": "0x5678...efgh",
      "type": "weather_data_submission",
      "from": "0x9abc...def0",
      "gasUsed": 45000,
      "climateOptimization": true
    }
  ],
  "climateData": {
    "weatherReadings": 45,
    "computations": 3,
    "carbonCredits": 12
  },
  "gasEfficiency": 0.157
}
```

### Get Transaction Details

```http
GET /blockchain/transactions/{txHash}
```

**Example Response:**

```json
{
  "hash": "0x1234...abcd",
  "blockNumber": 1234567,
  "from": "0x5678...efgh",
  "to": "0x9abc...def0",
  "value": "1000000000000000000",
  "gasUsed": 45000,
  "gasPrice": "20000000000",
  "type": "climate_computation",
  "climateData": {
    "computationType": "weather_prediction",
    "location": "New York City",
    "accuracy": 0.91
  },
  "layer2": {
    "sequencer": "seq_001",
    "batchId": "batch_12345",
    "optimizationApplied": true
  }
}
```

## 📡 WebSocket API

Real-time data streams for live applications.

### Connect to WebSocket

```javascript
const ws = new WebSocket('wss://ws.wrfcoin.org/v1?apikey=YOUR_API_KEY');

// Subscribe to real-time weather updates
ws.send(
  JSON.stringify({
    action: 'subscribe',
    channel: 'weather',
    locations: [
      { lat: 40.7128, lng: -74.006 }, // NYC
      { lat: 51.5074, lng: -0.1278 }, // London
    ],
  })
);

// Handle incoming data
ws.onmessage = event => {
  const data = JSON.parse(event.data);
  console.log('Weather update:', data);
};
```

### Available Channels

- `weather` - Real-time weather data updates
- `blocks` - New block notifications
- `transactions` - Transaction confirmations
- `computations` - Computation status updates
- `carbon` - Carbon credit trading activity

## 🚨 Error Codes

| Code  | Description  | Solution                      |
| ----- | ------------ | ----------------------------- |
| `400` | Bad Request  | Check your request parameters |
| `401` | Unauthorized | Verify your API key           |
| `403` | Forbidden    | Check API key permissions     |
| `404` | Not Found    | Verify the endpoint URL       |
| `429` | Rate Limited | Reduce request frequency      |
| `500` | Server Error | Contact support               |

**Error Response Format:**

```json
{
  "error": {
    "code": "INVALID_COORDINATES",
    "message": "Latitude must be between -90 and 90",
    "details": {
      "field": "lat",
      "value": "91.0",
      "constraint": "range"
    }
  },
  "timestamp": "2024-01-15T14:30:00Z",
  "requestId": "req_1234567890"
}
```

## 📊 Rate Limits

| Plan         | Requests/minute | Burst  | WebSocket Connections |
| ------------ | --------------- | ------ | --------------------- |
| Free         | 100             | 200    | 2                     |
| Developer    | 1,000           | 2,000  | 10                    |
| Professional | 10,000          | 20,000 | 100                   |
| Enterprise   | Custom          | Custom | Custom                |

Rate limit headers included in all responses:

```
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 999
X-RateLimit-Reset: 1705329000
```

## 🌍 GraphQL API

For complex queries and real-time subscriptions.

**Endpoint:** `https://api.wrfcoin.org/graphql`

**Example Query:**

```graphql
query WeatherData($location: LocationInput!, $period: String!) {
  weather(location: $location) {
    current {
      temperature
      humidity
      conditions
      timestamp
    }
    historical(period: $period) {
      temperature
      timestamp
      blockHash
    }
  }
  blockchain {
    latestBlock {
      number
      climateTransactions
    }
  }
}
```

**Variables:**

```json
{
  "location": { "lat": 40.7128, "lng": -74.006 },
  "period": "24h"
}
```

## 🔧 SDK Integration

### JavaScript/TypeScript

```javascript
import { WrfcoinAPI } from '@wrfcoin/sdk';

const api = new WrfcoinAPI({
  apiKey: 'your-api-key',
  network: 'mainnet',
});

const weather = await api.weather.getCurrent({
  lat: 40.7128,
  lng: -74.006,
});
```

### Python

```python
from wrfcoin import WrfcoinAPI

api = WrfcoinAPI(
    api_key='your-api-key',
    network='mainnet'
)

weather = await api.weather.get_current(
    lat=40.7128,
    lng=-74.0060
)
```

### cURL Examples

```bash
# Get current weather
curl -H "Authorization: Bearer YOUR_API_KEY" \
  "https://api.wrfcoin.org/v1/weather/current?lat=40.7128&lng=-74.0060"

# Submit weather data
curl -X POST \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"readings":[{"latitude":40.7128,"longitude":-74.0060,"temperature":22.5}]}' \
  "https://api.wrfcoin.org/v1/weather/submit"
```

## 📞 Support

- **API Status**: [status.wrfcoin.org](https://status.wrfcoin.org)
- **Developer Discord**: [discord.gg/wrfcoin-dev](https://discord.gg/wrfcoin-dev)
- **GitHub Issues**: [github.com/wrfcoin/api](https://github.com/wrfcoin/api)
- **Email Support**: [api-support@wrfcoin.org](mailto:api-support@wrfcoin.org)
