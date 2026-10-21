# Interactive API Explorer - Wrfcoin Climate Platform

## 🚀 **Live API Testing Environment**

### **Getting Started with API Explorer**

Access the interactive API explorer at: `https://api.wrfcoin.org/explorer`

```bash
# Quick authentication
curl -X POST https://api.wrfcoin.org/auth/token \
  -H "Content-Type: application/json" \
  -d '{"api_key": "your_api_key_here"}'
```

### **Climate Data Endpoints**

#### **GET /api/climate/current**

Get real-time climate data for any location

**Parameters:**

- `location` (string, required): Location identifier
- `metrics` (array, optional): Specific metrics to retrieve
- `format` (string, optional): Response format (json, csv, xml)

**Example Request:**

```javascript
const response = await fetch('https://api.wrfcoin.org/api/climate/current', {
  method: 'GET',
  headers: {
    Authorization: 'Bearer your_token_here',
    'Content-Type': 'application/json',
  },
  params: {
    location: 'New York, NY',
    metrics: ['temperature', 'humidity', 'pressure'],
    format: 'json',
  },
});

const data = await response.json();
```

**Example Response:**

```json
{
  "location": {
    "name": "New York, NY",
    "coordinates": {
      "latitude": 40.7128,
      "longitude": -74.006
    }
  },
  "timestamp": "2025-05-24T01:35:00Z",
  "data": {
    "temperature": {
      "value": 22.5,
      "unit": "celsius",
      "quality_score": 0.98
    },
    "humidity": {
      "value": 65.2,
      "unit": "percent",
      "quality_score": 0.95
    },
    "pressure": {
      "value": 1013.25,
      "unit": "hPa",
      "quality_score": 0.97
    }
  },
  "metadata": {
    "data_source": "wrfcoin_network",
    "verification_hash": "0x1a2b3c4d...",
    "blockchain_height": 1234567
  }
}
```

#### **POST /api/climate/submit**

Submit climate data to the network

**Request Body:**

```json
{
  "sensor_id": "WRF_SENSOR_001",
  "location": {
    "latitude": 40.7128,
    "longitude": -74.006
  },
  "measurements": [
    {
      "type": "temperature",
      "value": 23.1,
      "unit": "celsius",
      "timestamp": "2025-05-24T01:35:00Z"
    },
    {
      "type": "humidity",
      "value": 67.3,
      "unit": "percent",
      "timestamp": "2025-05-24T01:35:00Z"
    }
  ],
  "quality_metrics": {
    "calibration_date": "2025-05-01",
    "accuracy_rating": 0.98,
    "maintenance_status": "good"
  }
}
```

### **Carbon Credit Trading Endpoints**

#### **GET /api/carbon/credits**

Retrieve available carbon credits

**Query Parameters:**

- `project_type` (string): Type of carbon credit project
- `vintage` (integer): Year of carbon credit vintage
- `min_price` (number): Minimum price per credit
- `max_price` (number): Maximum price per credit
- `region` (string): Geographic region

**Example:**

```bash
curl "https://api.wrfcoin.org/api/carbon/credits?project_type=renewable_energy&vintage=2024&region=north_america" \
  -H "Authorization: Bearer your_token"
```

#### **POST /api/carbon/purchase**

Purchase carbon credits

**Request:**

```json
{
  "credit_id": "CC_RE_2024_001",
  "quantity": 100,
  "price_per_credit": 25.5,
  "buyer_wallet": "0x742d35Cc6634C0532925a3b8D2F09E9a23E4c",
  "payment_method": "USDC"
}
```

### Cross-chain bridge endpoints

`NOT_AVAILABLE` in the public release. The API explorer intentionally exposes
no discovery, transfer, or transfer-status operation.

### **Real-Time WebSocket Streams**

#### **Climate Data Stream**

```javascript
const ws = new WebSocket('wss://api.wrfcoin.org/ws/climate');

ws.on('open', () => {
  // Subscribe to climate updates
  ws.send(
    JSON.stringify({
      type: 'subscribe',
      channels: ['temperature', 'weather_alerts'],
      regions: ['north_america', 'europe'],
    })
  );
});

ws.on('message', data => {
  const update = JSON.parse(data);
  console.log('Climate update:', update);
});
```

#### **Market Data Stream**

```javascript
const marketWs = new WebSocket('wss://api.wrfcoin.org/ws/market');

marketWs.on('message', data => {
  const marketData = JSON.parse(data);
  // Handle carbon credit price updates
  if (marketData.type === 'price_update') {
    updateCarbonCreditPrices(marketData.prices);
  }
});
```

### **GraphQL Schema Explorer**

#### **Climate Data Query**

```graphql
query GetClimateData($location: String!, $timeRange: TimeRange!) {
  climateData(location: $location, timeRange: $timeRange) {
    location {
      name
      coordinates {
        latitude
        longitude
      }
    }
    measurements {
      timestamp
      temperature {
        value
        unit
        qualityScore
      }
      humidity {
        value
        unit
        qualityScore
      }
      airQuality {
        aqi
        pollutants {
          type
          concentration
          unit
        }
      }
    }
    trends {
      temperatureTrend
      precipitationTrend
      airQualityTrend
    }
  }
}
```

#### **Carbon Credit Marketplace Query**

```graphql
query CarbonMarketplace($filters: CarbonCreditFilters) {
  carbonCredits(filters: $filters) {
    id
    projectName
    projectType
    vintage
    pricePerCredit
    availableQuantity
    certification {
      standard
      registry
      verifier
    }
    location {
      country
      region
    }
    environmental_impact {
      co2Reduced
      biodiversityScore
      communityBenefit
    }
  }
}
```

### **Authentication & Rate Limiting**

#### **API Key Authentication**

```bash
# Include API key in header
curl -H "X-API-Key: your_api_key_here" \
     -H "Content-Type: application/json" \
     https://api.wrfcoin.org/api/climate/current
```

#### **OAuth 2.0 Flow**

```javascript
// Step 1: Redirect to authorization
const authUrl =
  'https://api.wrfcoin.org/oauth/authorize?' +
  'client_id=your_client_id&' +
  'redirect_uri=https://yourapp.com/callback&' +
  'response_type=code&' +
  'scope=read:climate write:carbon_credits';

// Step 2: Exchange code for token
const tokenResponse = await fetch('https://api.wrfcoin.org/oauth/token', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    grant_type: 'authorization_code',
    code: authorization_code,
    client_id: 'your_client_id',
    client_secret: 'your_client_secret',
  }),
});
```

#### **Rate Limiting**

- **Free Tier**: 1,000 requests/hour
- **Developer Tier**: 10,000 requests/hour
- **Enterprise Tier**: 100,000 requests/hour
- **Custom Enterprise**: Unlimited with SLA

**Rate Limit Headers:**

```
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 999
X-RateLimit-Reset: 1640995200
```

### **Error Handling**

#### **Standard Error Response**

```json
{
  "error": {
    "code": "INVALID_LOCATION",
    "message": "The specified location could not be found",
    "details": {
      "provided_location": "Invalid Location",
      "suggestions": ["New York, NY", "London, UK", "Tokyo, JP"]
    },
    "documentation_url": "https://docs.wrfcoin.org/errors/invalid-location"
  },
  "request_id": "req_1a2b3c4d5e6f",
  "timestamp": "2025-05-24T01:35:00Z"
}
```

#### **Common Error Codes**

- `400` - Bad Request: Invalid parameters
- `401` - Unauthorized: Invalid or missing API key
- `403` - Forbidden: Insufficient permissions
- `404` - Not Found: Resource doesn't exist
- `429` - Rate Limited: Too many requests
- `500` - Internal Error: Server error

### **SDK Integration Examples**

#### **JavaScript/Node.js**

```javascript
import { WrfcoinAPI } from 'wrfcoin-sdk';

const api = new WrfcoinAPI({
  apiKey: 'your_api_key',
  environment: 'production',
});

// Get current climate data
const climate = await api.climate.getCurrent({
  location: 'San Francisco, CA',
  metrics: ['temperature', 'air_quality'],
});
```

#### **Python**

```python
from wrfcoin import WrfcoinClient

client = WrfcoinClient(api_key='your_api_key')

# Submit climate data
result = client.climate.submit({
    'sensor_id': 'WRF_001',
    'measurements': [
        {'type': 'temperature', 'value': 25.3, 'unit': 'celsius'}
    ]
})
```

#### **cURL Examples**

```bash
# Get blockchain status
curl -X GET "https://api.wrfcoin.org/api/blockchain/status" \
  -H "Authorization: Bearer $WRFCOIN_TOKEN"

# Submit weather data
curl -X POST "https://api.wrfcoin.org/api/climate/submit" \
  -H "Authorization: Bearer $WRFCOIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "sensor_id": "WEATHER_001",
    "location": {"lat": 40.7128, "lon": -74.0060},
    "measurements": [
      {"type": "temperature", "value": 22.5, "unit": "celsius"}
    ]
  }'
```

---

**🔧 The Interactive API Explorer provides comprehensive testing capabilities for all Wrfcoin
climate APIs with real-time data validation and enterprise-grade authentication.**
