# WRFCoin Developer Getting Started Guide

**Complete guide to building on the WRFCoin climate-focused blockchain platform**

## Table of Contents

1. [Quick Start](#quick-start)
2. [Authentication & API Keys](#authentication--api-keys)
3. [SDKs & Integration Options](#sdks--integration-options)
4. [Core Concepts](#core-concepts)
5. [Your First API Call](#your-first-api-call)
6. [Weather Data Integration](#weather-data-integration)
7. [Cross-Chain Availability](#cross-chain-operations)
8. [Computational NFTs](#computational-nfts)
9. [Testing & Development](#testing--development)
10. [Production Deployment](#production-deployment)
11. [Support & Resources](#support--resources)

## Quick Reference

### Essential npm Scripts

| Command | Description |
|---------|-------------|
| `npm run start:all` | Start frontend, backend, and GraphQL servers |
| `npm run start:backend` | Start backend API only (port 4001) |
| `npm run start:frontend` | Start frontend dev server (port 3000) |
| `npm run test:unit` | Run unit tests |
| `npm run test:all` | Run all test suites |
| `npm run build` | Build all packages |

### Database Commands

| Command | Description |
|---------|-------------|
| `npm run db:status` | Show database connection and tables |
| `npm run db:setup` | Push schema + seed all data (fresh start) |
| `npm run db:push` | Push schema changes to database |
| `npm run db:migrate` | Apply database migrations |
| `npm run db:seed` | Seed all test/demo data |

### Docker Commands

| Command | Description |
|---------|-------------|
| `npm run docker:up` | Start all containers (Postgres, Redis, IPFS) |
| `npm run docker:down` | Stop all containers |
| `npm run docker:logs` | Follow container logs |
| `npm run docker:ps` | Show running containers |

### Service Ports

| Port | Service | Health Check |
|------|---------|--------------|
| 3000 | Frontend | `http://localhost:3000` |
| 4001 | Main API | `curl http://localhost:4001/health` |
| 4000 | Wallet/GraphQL | `curl http://localhost:4000/health` |
| 3002 | IoT Networks | `curl http://localhost:3002/health` |
| 6001 | AI/ML | `curl http://localhost:6001/health` |
| 8000 | Governance | `curl http://localhost:8000/health` |
| 5432 | PostgreSQL | `pg_isready -h localhost` |
| 6379 | Redis | `redis-cli ping` |

---

## Quick Start

Get up and running with WRFCoin in under 5 minutes:

### 1. Get Your API Key

Visit the [Developer Portal](http://localhost:5000/developer) and create a new API key:

```bash
curl -X POST http://localhost:3001/api/developer/api-keys \
  -H "Content-Type: application/json" \
  -d '{"name": "My First App", "permissions": ["read", "write"]}'
```

### 2. Install the SDK

#### JavaScript/TypeScript

```bash
npm install @wrfcoin/sdk
# or
yarn add @wrfcoin/sdk
```

#### Python

```bash
pip install wrfcoin-sdk
```

#### Rust

```toml
[dependencies]
wrfcoin-sdk = "0.3.0"
```

#### Go

```bash
go get github.com/wrfcoin/sdk-go
```

### 3. Initialize the SDK

#### JavaScript/TypeScript

```javascript
import { WRFCoinSDK } from '@wrfcoin/sdk';

const sdk = new WRFCoinSDK({
  apiKey: 'your-api-key',
  network: 'mainnet', // or 'testnet'
  endpoint: 'http://localhost:3001',
});
```

#### Python

```python
from wrfcoin_sdk import WRFCoinClient

client = WRFCoinClient(
    api_key='your-api-key',
    network='mainnet',
    endpoint='http://localhost:3001'
)
```

#### Rust

```rust
use wrfcoin_sdk::WRFCoinClient;

let client = WRFCoinClient::new(
    "your-api-key",
    "mainnet",
    "http://localhost:3001"
).await?;
```

#### Go

```go
import "github.com/wrfcoin/sdk-go"

client := wrfcoin.NewClient(&wrfcoin.Config{
    APIKey:   "your-api-key",
    Network:  "mainnet",
    Endpoint: "http://localhost:3001",
})
```

### 4. Make Your First Call

#### JavaScript/TypeScript

```javascript
async function getBlockchainStatus() {
  try {
    const status = await sdk.blockchain.getStatus();
    console.log('Blockchain Status:', status);
  } catch (error) {
    console.error('Error:', error);
  }
}

getBlockchainStatus();
```

#### Python

```python
import asyncio

async def get_blockchain_status():
    try:
        status = await client.blockchain.get_status()
        print("Blockchain Status:", status)
    except Exception as error:
        print("Error:", error)

asyncio.run(get_blockchain_status())
```

#### Rust

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let status = client.blockchain().get_status().await?;
    println!("Blockchain Status: {:?}", status);
    Ok(())
}
```

#### Go

```go
func main() {
    ctx := context.Background()
    status, err := client.Blockchain.GetStatus(ctx)
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Blockchain Status: %+v\n", status)
}
```

## Authentication & API Keys

WRFCoin uses API key authentication for secure access to the platform.

### Creating API Keys

1. **Via Developer Portal**: Visit [localhost:5000/developer](http://localhost:5000/developer)
2. **Via API**: Use the API key management endpoints

#### API Key Management

**Create a New API Key:**

```bash
curl -X POST http://localhost:3001/api/developer/api-keys \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Production App",
    "permissions": ["read", "write"],
    "rateLimitTier": "premium"
  }'
```

**List Your API Keys:**

```bash
curl -X GET http://localhost:3001/api/developer/api-keys \
  -H "Authorization: Bearer your-jwt-token"
```

**Delete an API Key:**

```bash
curl -X DELETE http://localhost:3001/api/developer/api-keys/{key-id} \
  -H "Authorization: Bearer your-jwt-token"
```

### API Key Security

⚠️ **Security Best Practices:**

1. **Never commit API keys to version control**
2. **Use environment variables**
3. **Rotate keys regularly**
4. **Use least privilege permissions**
5. **Monitor key usage**

#### Environment Variables Setup

Create a `.env` file:

```bash
# .env
WRFCOIN_API_KEY=your-api-key-here
WRFCOIN_NETWORK=mainnet
WRFCOIN_ENDPOINT=http://localhost:3001
```

#### Loading Environment Variables

**JavaScript/TypeScript:**

```javascript
import dotenv from 'dotenv';
dotenv.config();

const sdk = new WRFCoinSDK({
  apiKey: process.env.WRFCOIN_API_KEY,
  network: process.env.WRFCOIN_NETWORK,
  endpoint: process.env.WRFCOIN_ENDPOINT,
});
```

**Python:**

```python
import os
from dotenv import load_dotenv

load_dotenv()

client = WRFCoinClient(
    api_key=os.getenv('WRFCOIN_API_KEY'),
    network=os.getenv('WRFCOIN_NETWORK'),
    endpoint=os.getenv('WRFCOIN_ENDPOINT')
)
```

## SDKs & Integration Options

WRFCoin provides SDKs for all major programming languages and frameworks.

### Available SDKs

| Language              | Status            | Version | Package                     |
| --------------------- | ----------------- | ------- | --------------------------- |
| JavaScript/TypeScript | ✅ Available      | 1.2.3   | `@wrfcoin/sdk`              |
| React/Vue Components  | Retired examples  | -       | Not published               |
| Python                | ✅ Available      | 1.0.5   | `wrfcoin-sdk`               |
| Rust                  | ✅ Available      | 0.3.0   | `wrfcoin-sdk`               |
| Go                    | ✅ Available      | 1.0.2   | `github.com/wrfcoin/sdk-go` |
| Java                  | 🚧 Coming Q4 2024 | -       | -                           |
| C#                    | 🚧 Coming Q1 2025 | -       | -                           |
| Swift                 | 🚧 Coming Q2 2025 | -       | -                           |

### SDK Downloads

Download complete SDK packages from the Developer Portal:

- **JavaScript/TypeScript**:
  [Download SDK](http://localhost:3001/api/developer/sdk/download/typescript)
- **Python**: [Download SDK](http://localhost:3001/api/developer/sdk/download/python)
- **Rust**: [Download SDK](http://localhost:3001/api/developer/sdk/download/rust)
- **Go**: [Download SDK](http://localhost:3001/api/developer/sdk/download/go)

The historical React and Vue component trees are unsupported source examples.
They are not published or downloadable SDK packages and are excluded from
release artifacts.

### Direct API Integration

If you prefer not to use an SDK, you can integrate directly with the REST API:

```bash
# Basic API call
curl -X GET http://localhost:3001/api/blockchain/status \
  -H "Authorization: Bearer your-api-key" \
  -H "Content-Type: application/json"
```

## Core Concepts

Understanding these core concepts will help you build better applications on WRFCoin.

### 1. Climate-Focused Blockchain

WRFCoin is specifically designed for environmental and climate applications:

- **Weather Data Integration**: Real-time weather station data validation
- **Carbon Credit Tracking**: Automated carbon footprint calculation and credits
- **Environmental NFTs**: Computational NFTs for climate modeling
- **Proof-of-Publishing**: Energy-efficient consensus mechanism

### 2. Native-chain architecture

The public release exposes the WRFCoin native chain. Cross-chain transfers are
`NOT_AVAILABLE`.

### 3. Weather Data Economy

Contribute weather data and earn WRF tokens:

```javascript
// Submit weather data
const submission = await sdk.weather.submit({
  stationId: 'my-station-001',
  temperature: 25.3,
  humidity: 65,
  pressure: 1013.25,
  timestamp: new Date().toISOString(),
});
```

### 4. Computational NFTs

NFTs that perform actual computations:

```javascript
// Create computational NFT
const nft = await sdk.nft.createComputational({
  name: 'Weather Prediction Model',
  description: 'ML model for local weather forecasting',
  computationType: 'machine_learning',
  modelData: wasmBytes,
  inputSchema: {
    temperature: 'number',
    humidity: 'number',
    pressure: 'number',
  },
});
```

## Your First API Call

Let's create a comprehensive example that demonstrates the basic API workflow.

### Complete Example: Weather Station Monitor

This example shows how to:

1. Connect to the API
2. Get blockchain status
3. Fetch weather station data
4. Submit new weather data
5. Handle errors properly

#### JavaScript/TypeScript Implementation

```javascript
import { WRFCoinSDK } from '@wrfcoin/sdk';
import dotenv from 'dotenv';

dotenv.config();

class WeatherStationMonitor {
  constructor() {
    this.sdk = new WRFCoinSDK({
      apiKey: process.env.WRFCOIN_API_KEY,
      network: 'mainnet',
      endpoint: 'http://localhost:3001',
    });
  }

  async initialize() {
    try {
      // Check blockchain status
      const status = await this.sdk.blockchain.getStatus();
      console.log('🟢 Connected to WRFCoin blockchain');
      console.log(`Block Height: ${status.blockHeight}`);
      console.log(`Network: ${status.network}`);

      return true;
    } catch (error) {
      console.error('❌ Failed to connect:', error.message);
      return false;
    }
  }

  async getWeatherStations() {
    try {
      const stations = await this.sdk.weather.getStations();
      console.log(`\n📡 Found ${stations.length} weather stations:`);

      stations.forEach(station => {
        console.log(`  - ${station.name} (${station.location.lat}, ${station.location.lon})`);
      });

      return stations;
    } catch (error) {
      console.error('❌ Failed to fetch stations:', error.message);
      return [];
    }
  }

  async submitWeatherData(stationId, weatherData) {
    try {
      const submission = await this.sdk.weather.submit({
        stationId,
        ...weatherData,
        timestamp: new Date().toISOString(),
      });

      console.log(`\n✅ Weather data submitted successfully`);
      console.log(`Transaction ID: ${submission.transactionId}`);
      console.log(`Reward: ${submission.reward} WRF`);

      return submission;
    } catch (error) {
      console.error('❌ Failed to submit weather data:', error.message);
      throw error;
    }
  }

  async monitorStation(stationId) {
    console.log(`\n🔄 Monitoring station: ${stationId}`);

    // Simulate weather data collection every 30 seconds
    setInterval(async () => {
      const weatherData = {
        temperature: 20 + Math.random() * 15, // 20-35°C
        humidity: 40 + Math.random() * 40, // 40-80%
        pressure: 1000 + Math.random() * 50, // 1000-1050 hPa
      };

      try {
        await this.submitWeatherData(stationId, weatherData);
      } catch (error) {
        console.error('❌ Failed to submit data:', error.message);
      }
    }, 30000);
  }
}

// Main execution
async function main() {
  const monitor = new WeatherStationMonitor();

  // Initialize connection
  const connected = await monitor.initialize();
  if (!connected) {
    process.exit(1);
  }

  // Get available stations
  const stations = await monitor.getWeatherStations();
  if (stations.length === 0) {
    console.log('No weather stations available');
    return;
  }

  // Start monitoring the first station
  await monitor.monitorStation(stations[0].id);

  console.log('\n🏃 Monitor running... Press Ctrl+C to stop');
}

main().catch(console.error);
```

#### Python Implementation

```python
import asyncio
import os
import time
import random
from datetime import datetime
from wrfcoin_sdk import WRFCoinClient
from dotenv import load_dotenv

load_dotenv()

class WeatherStationMonitor:
    def __init__(self):
        self.client = WRFCoinClient(
            api_key=os.getenv('WRFCOIN_API_KEY'),
            network=os.getenv('WRFCOIN_NETWORK', 'mainnet'),
            endpoint=os.getenv('WRFCOIN_ENDPOINT', 'http://localhost:3001')
        )

    async def initialize(self):
        try:
            status = await self.client.blockchain.get_status()
            print("🟢 Connected to WRFCoin blockchain")
            print(f"Block Height: {status['blockHeight']}")
            print(f"Network: {status['network']}")
            return True
        except Exception as error:
            print(f"❌ Failed to connect: {error}")
            return False

    async def get_weather_stations(self):
        try:
            stations = await self.client.weather.get_stations()
            print(f"\n📡 Found {len(stations)} weather stations:")

            for station in stations:
                location = station['location']
                print(f"  - {station['name']} ({location['lat']}, {location['lon']})")

            return stations
        except Exception as error:
            print(f"❌ Failed to fetch stations: {error}")
            return []

    async def submit_weather_data(self, station_id, weather_data):
        try:
            submission = await self.client.weather.submit({
                'stationId': station_id,
                **weather_data,
                'timestamp': datetime.now().isoformat()
            })

            print(f"\n✅ Weather data submitted successfully")
            print(f"Transaction ID: {submission['transactionId']}")
            print(f"Reward: {submission['reward']} WRF")

            return submission
        except Exception as error:
            print(f"❌ Failed to submit weather data: {error}")
            raise error

    async def monitor_station(self, station_id):
        print(f"\n🔄 Monitoring station: {station_id}")

        while True:
            weather_data = {
                'temperature': 20 + random.random() * 15,  # 20-35°C
                'humidity': 40 + random.random() * 40,     # 40-80%
                'pressure': 1000 + random.random() * 50    # 1000-1050 hPa
            }

            try:
                await self.submit_weather_data(station_id, weather_data)
            except Exception as error:
                print(f"❌ Failed to submit data: {error}")

            await asyncio.sleep(30)  # Wait 30 seconds

async def main():
    monitor = WeatherStationMonitor()

    # Initialize connection
    connected = await monitor.initialize()
    if not connected:
        return

    # Get available stations
    stations = await monitor.get_weather_stations()
    if not stations:
        print("No weather stations available")
        return

    # Start monitoring the first station
    print("\n🏃 Monitor running... Press Ctrl+C to stop")
    try:
        await monitor.monitor_station(stations[0]['id'])
    except KeyboardInterrupt:
        print("\n👋 Monitor stopped")

if __name__ == "__main__":
    asyncio.run(main())
```

#### Rust Implementation

```rust
use wrfcoin_sdk::{WRFCoinClient, WeatherData, Result};
use tokio::time::{sleep, Duration};
use rand::Rng;
use chrono::Utc;

struct WeatherStationMonitor {
    client: WRFCoinClient,
}

impl WeatherStationMonitor {
    pub fn new() -> Result<Self> {
        let api_key = std::env::var("WRFCOIN_API_KEY")
            .expect("WRFCOIN_API_KEY environment variable required");

        let client = WRFCoinClient::new(
            &api_key,
            "mainnet",
            "http://localhost:3001"
        )?;

        Ok(Self { client })
    }

    pub async fn initialize(&self) -> Result<bool> {
        match self.client.blockchain().get_status().await {
            Ok(status) => {
                println!("🟢 Connected to WRFCoin blockchain");
                println!("Block Height: {}", status.block_height);
                println!("Network: {}", status.network);
                Ok(true)
            }
            Err(error) => {
                eprintln!("❌ Failed to connect: {}", error);
                Ok(false)
            }
        }
    }

    pub async fn get_weather_stations(&self) -> Result<Vec<WeatherStation>> {
        match self.client.weather().get_stations().await {
            Ok(stations) => {
                println!("\n📡 Found {} weather stations:", stations.len());

                for station in &stations {
                    println!(
                        "  - {} ({}, {})",
                        station.name,
                        station.location.lat,
                        station.location.lon
                    );
                }

                Ok(stations)
            }
            Err(error) => {
                eprintln!("❌ Failed to fetch stations: {}", error);
                Ok(Vec::new())
            }
        }
    }

    pub async fn submit_weather_data(
        &self,
        station_id: &str,
        weather_data: WeatherData,
    ) -> Result<WeatherSubmission> {
        let submission = self.client.weather().submit(
            station_id,
            weather_data,
            Utc::now(),
        ).await?;

        println!("\n✅ Weather data submitted successfully");
        println!("Transaction ID: {}", submission.transaction_id);
        println!("Reward: {} WRF", submission.reward);

        Ok(submission)
    }

    pub async fn monitor_station(&self, station_id: &str) -> Result<()> {
        println!("\n🔄 Monitoring station: {}", station_id);

        let mut rng = rand::thread_rng();

        loop {
            let weather_data = WeatherData {
                temperature: 20.0 + rng.gen::<f64>() * 15.0, // 20-35°C
                humidity: 40.0 + rng.gen::<f64>() * 40.0,    // 40-80%
                pressure: 1000.0 + rng.gen::<f64>() * 50.0,  // 1000-1050 hPa
            };

            if let Err(error) = self.submit_weather_data(station_id, weather_data).await {
                eprintln!("❌ Failed to submit data: {}", error);
            }

            sleep(Duration::from_secs(30)).await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let monitor = WeatherStationMonitor::new()?;

    // Initialize connection
    if !monitor.initialize().await? {
        return Ok(());
    }

    // Get available stations
    let stations = monitor.get_weather_stations().await?;
    if stations.is_empty() {
        println!("No weather stations available");
        return Ok(());
    }

    // Start monitoring the first station
    println!("\n🏃 Monitor running... Press Ctrl+C to stop");
    monitor.monitor_station(&stations[0].id).await?;

    Ok(())
}
```

## Weather Data Integration

Weather data is at the heart of WRFCoin's climate-focused approach. Here's how to integrate weather
stations and contribute data to earn rewards.

### Weather Station Registration

Before submitting data, register your weather station:

```javascript
const station = await sdk.weather.registerStation({
  name: 'My Home Weather Station',
  location: {
    latitude: 30.628,
    longitude: -96.3344,
    elevation: 104, // meters above sea level
  },
  equipment: {
    temperatureSensor: 'AcuRite 5-in-1',
    humiditySensor: 'Built-in',
    pressureSensor: 'Barometric',
    windSensor: 'Anemometer',
    rainGauge: 'Digital',
  },
  dataSubmissionInterval: 300, // seconds (5 minutes)
});

console.log(`Station registered with ID: ${station.id}`);
```

### Data Validation & Quality Scoring

WRFCoin uses physics-based validation to ensure data quality:

```javascript
// Submit high-quality weather data
const submission = await sdk.weather.submit({
  stationId: 'station-001',
  temperature: 25.3, // °C
  humidity: 65, // %
  pressure: 1013.25, // hPa
  windSpeed: 5.2, // m/s
  windDirection: 180, // degrees
  rainfall: 0.0, // mm
  timestamp: new Date().toISOString(),

  // Optional: Include sensor metadata for higher quality score
  sensorData: {
    temperatureAccuracy: 0.1,
    humidityAccuracy: 2,
    calibrationDate: '2024-01-15T00:00:00Z',
  },
});

console.log(`Quality Score: ${submission.qualityScore}/100`);
console.log(`Reward: ${submission.reward} WRF`);
```

### Quality Factors

Data quality affects your rewards. Higher quality data earns more WRF:

- **Sensor Accuracy**: Better sensors = higher rewards
- **Calibration Status**: Recently calibrated equipment
- **Data Consistency**: Consistent reporting intervals
- **Physics Validation**: Data that passes atmospheric science checks
- **Cross-Validation**: Agreement with nearby stations

### Weather Data Types

WRFCoin supports various weather measurements:

```javascript
// Comprehensive weather reading
const fullReading = {
  // Basic measurements
  temperature: 25.3, // Air temperature (°C)
  humidity: 65, // Relative humidity (%)
  pressure: 1013.25, // Atmospheric pressure (hPa)

  // Wind measurements
  windSpeed: 5.2, // Wind speed (m/s)
  windDirection: 180, // Wind direction (degrees)
  windGust: 8.1, // Wind gust speed (m/s)

  // Precipitation
  rainfall: 0.0, // Rainfall amount (mm)
  rainfallRate: 0.0, // Current rain rate (mm/hr)

  // Solar and UV
  solarRadiation: 850, // Solar radiation (W/m²)
  uvIndex: 6, // UV index

  // Advanced measurements
  dewPoint: 18.2, // Dew point (°C)
  heatIndex: 27.8, // Heat index (°C)
  windChill: 25.3, // Wind chill (°C)

  // Air quality (if available)
  pm25: 12.5, // PM2.5 (μg/m³)
  pm10: 18.3, // PM10 (μg/m³)
  ozone: 45, // Ozone (ppb)

  timestamp: new Date().toISOString(),
};
```

### Historical Data Access

Access historical weather data for analysis:

```javascript
// Get historical data for a region
const historicalData = await sdk.weather.getHistorical({
  region: {
    latitude: 30.628,
    longitude: -96.3344,
    radius: 50, // km
  },
  startDate: '2024-01-01T00:00:00Z',
  endDate: '2024-01-31T23:59:59Z',
  measurements: ['temperature', 'humidity', 'pressure'],
  aggregation: 'hourly', // 'raw', 'hourly', 'daily'
});

// Analyze temperature trends
const avgTemp =
  historicalData.reduce((sum, reading) => sum + reading.temperature, 0) / historicalData.length;

console.log(`Average temperature in January: ${avgTemp.toFixed(1)}°C`);
```

## Cross-Chain Operations

`NOT_AVAILABLE` in the public release. The SDK deliberately exports no bridge
client or provider implementation.

## Computational NFTs

Computational NFTs execute real computations and can be used for weather modeling, climate analysis,
and environmental monitoring.

### Creating Computational NFTs

```javascript
// Create a weather forecasting model NFT
const nft = await sdk.nft.createComputational({
  metadata: {
    name: 'Texas Weather Prediction Model v2.1',
    description: 'Machine learning model for 7-day weather forecasting in Texas',
    image: 'ipfs://QmHash...of_model_visualization',
    attributes: [
      { trait_type: 'Model Type', value: 'Neural Network' },
      { trait_type: 'Accuracy', value: '94.2%' },
      { trait_type: 'Training Data', value: '5 years' },
      { trait_type: 'Update Frequency', value: 'Daily' },
    ],
  },

  // Computational specification
  computation: {
    type: 'weather_forecasting',
    runtime: 'wasm', // WebAssembly for cross-platform execution
    code: modelWasmBytes,

    // Input schema
    inputSchema: {
      temperature: { type: 'number', min: -50, max: 60 },
      humidity: { type: 'number', min: 0, max: 100 },
      pressure: { type: 'number', min: 800, max: 1200 },
      windSpeed: { type: 'number', min: 0, max: 100 },
      historicalData: { type: 'array', items: 'weatherReading' },
    },

    // Output schema
    outputSchema: {
      forecast: {
        type: 'array',
        items: {
          date: 'string',
          temperature: 'number',
          humidity: 'number',
          precipitation: 'number',
          confidence: 'number',
        },
      },
    },

    // Resource requirements
    resources: {
      cpu: 2, // CPU cores
      memory: 512, // MB
      storage: 100, // MB
      maxRuntime: 300, // seconds
    },
  },

  // Economic model
  pricing: {
    executionFee: 0.1, // WRF per execution
    royalty: 2.5, // % to creator on resale
    stakingReward: 1.0, // WRF per day for stakers
  },
});

console.log(`Computational NFT created: ${nft.tokenId}`);
console.log(`Contract address: ${nft.contractAddress}`);
```

### Executing Computational NFTs

```javascript
// Execute the weather prediction model
const execution = await sdk.nft.execute({
  tokenId: nft.tokenId,

  // Input data for the model
  inputs: {
    temperature: 28.5,
    humidity: 70,
    pressure: 1015.2,
    windSpeed: 12.3,
    historicalData: await sdk.weather.getHistorical({
      region: { lat: 30.628, lon: -96.3344, radius: 10 },
      startDate: '2024-07-07T00:00:00Z',
      endDate: '2024-07-14T00:00:00Z',
    }),
  },

  // Execution preferences
  priority: 'high', // Execution priority
  maxFee: 0.15, // Maximum fee willing to pay
  callback: 'https://my-app.com/webhook/nft-result',
});

console.log(`Execution started: ${execution.executionId}`);
```

### Monitoring Execution

```javascript
// Monitor execution progress
const result = await sdk.nft.getExecutionResult(execution.executionId);

if (result.status === 'completed') {
  console.log('📊 7-Day Weather Forecast:');
  result.output.forecast.forEach(day => {
    console.log(
      `${day.date}: ${day.temperature}°C, ${day.precipitation}mm rain (${day.confidence}% confidence)`
    );
  });
} else if (result.status === 'failed') {
  console.error(`Execution failed: ${result.error}`);
}
```

### NFT Marketplace Integration

```javascript
// List computational NFT for sale
const listing = await sdk.nft.marketplace.list({
  tokenId: nft.tokenId,
  price: 50.0, // WRF
  duration: 30, // days

  // Include execution examples
  demoInputs: [
    {
      name: 'Hurricane Season Forecast',
      description: 'Predict hurricane activity for Gulf Coast',
      inputs: {
        /* demo weather data */
      },
    },
  ],
});

// Purchase and execute an NFT
const purchase = await sdk.nft.marketplace.buy({
  listingId: listing.id,
  executeImmediately: true,
  executionInputs: {
    /* your weather data */
  },
});
```

## Testing & Development

### Development Environment Setup

Set up a local development environment for testing:

```bash
# Clone the WRFCoin development environment
git clone https://github.com/wrfcoin/dev-environment
cd dev-environment

# Start local blockchain and services
docker-compose up -d

# Services will be available at:
# - Blockchain RPC: http://localhost:8545
# - API Gateway: http://localhost:3001
# - Frontend: http://localhost:5000
# - Documentation: http://localhost:3003
```

### Test Network Configuration

```javascript
// Configure SDK for test network
const testSdk = new WRFCoinSDK({
  apiKey: 'test-api-key',
  network: 'testnet',
  endpoint: 'http://localhost:3001',

  // Test network specific settings
  gasPrice: '1', // Lower gas prices for testing
  confirmations: 1, // Faster confirmations
  timeout: 10000, // Shorter timeouts
});
```

### Unit Testing

#### JavaScript/TypeScript Testing

```javascript
import { describe, it, expect, beforeEach } from 'jest';
import { WRFCoinSDK } from '@wrfcoin/sdk';

describe('WRFCoin SDK', () => {
  let sdk;

  beforeEach(() => {
    sdk = new WRFCoinSDK({
      apiKey: 'test-key',
      network: 'testnet',
      endpoint: 'http://localhost:3001',
    });
  });

  it('should get blockchain status', async () => {
    const status = await sdk.blockchain.getStatus();
    expect(status).toHaveProperty('blockHeight');
    expect(status).toHaveProperty('network');
    expect(status.network).toBe('testnet');
  });

  it('should validate weather data', async () => {
    const validData = {
      temperature: 25,
      humidity: 60,
      pressure: 1013,
    };

    const result = await sdk.weather.validate(validData);
    expect(result.isValid).toBe(true);
    expect(result.qualityScore).toBeGreaterThan(80);
  });

  it('should handle API errors gracefully', async () => {
    const invalidSdk = new WRFCoinSDK({
      apiKey: 'invalid-key',
      network: 'testnet',
      endpoint: 'http://localhost:3001',
    });

    await expect(invalidSdk.blockchain.getStatus()).rejects.toThrow('Authentication failed');
  });
});
```

#### Python Testing

```python
import pytest
import asyncio
from wrfcoin_sdk import WRFCoinClient

class TestWRFCoinSDK:
    @pytest.fixture
    def client(self):
        return WRFCoinClient(
            api_key='test-key',
            network='testnet',
            endpoint='http://localhost:3001'
        )

    @pytest.mark.asyncio
    async def test_blockchain_status(self, client):
        status = await client.blockchain.get_status()
        assert 'blockHeight' in status
        assert 'network' in status
        assert status['network'] == 'testnet'

    @pytest.mark.asyncio
    async def test_weather_validation(self, client):
        valid_data = {
            'temperature': 25,
            'humidity': 60,
            'pressure': 1013
        }

        result = await client.weather.validate(valid_data)
        assert result['isValid'] is True
        assert result['qualityScore'] > 80

    @pytest.mark.asyncio
    async def test_api_error_handling(self):
        invalid_client = WRFCoinClient(
            api_key='invalid-key',
            network='testnet',
            endpoint='http://localhost:3001'
        )

        with pytest.raises(Exception) as exc_info:
            await invalid_client.blockchain.get_status()

        assert 'Authentication failed' in str(exc_info.value)
```

### Integration Testing

```javascript
// Integration test example
describe('Weather Station Integration', () => {
  it('should complete full weather submission workflow', async () => {
    // 1. Register weather station
    const station = await sdk.weather.registerStation({
      name: 'Test Station',
      location: { latitude: 30.0, longitude: -95.0 },
    });

    // 2. Submit weather data
    const submission = await sdk.weather.submit({
      stationId: station.id,
      temperature: 25,
      humidity: 60,
      pressure: 1013,
      timestamp: new Date().toISOString(),
    });

    // 3. Verify submission
    expect(submission).toHaveProperty('transactionId');
    expect(submission.qualityScore).toBeGreaterThan(0);

    // 4. Check station data
    const stationData = await sdk.weather.getStationData(station.id);
    expect(stationData.readings).toHaveLength(1);
    expect(stationData.readings[0].temperature).toBe(25);
  });
});
```

## Production Deployment

### Environment Configuration

Create production environment configuration:

```bash
# .env.production
NODE_ENV=production
WRFCOIN_API_KEY=prod_your_production_api_key
WRFCOIN_NETWORK=mainnet
WRFCOIN_ENDPOINT=https://api.wrfcoin.org

# Security
SSL_ENABLED=true
RATE_LIMIT_ENABLED=true
CORS_ORIGIN=https://your-app.com

# Monitoring
LOG_LEVEL=info
METRICS_ENABLED=true
HEALTH_CHECK_INTERVAL=30000

# Performance
CONNECTION_POOL_SIZE=20
CACHE_TTL=300
TIMEOUT=30000
```

### Security Best Practices

1. **API Key Security**

```javascript
// Use environment variables
const apiKey = process.env.WRFCOIN_API_KEY;
if (!apiKey) {
  throw new Error('WRFCOIN_API_KEY environment variable is required');
}

// Implement key rotation
const rotateApiKey = async () => {
  const newKey = await sdk.auth.rotateApiKey();
  // Update environment variable in your deployment system
  updateEnvironmentVariable('WRFCOIN_API_KEY', newKey);
};
```

2. **Rate Limiting**

```javascript
// Implement client-side rate limiting
import { RateLimiter } from 'limiter';

const limiter = new RateLimiter(100, 'minute'); // 100 requests per minute

const makeApiCall = async (endpoint, data) => {
  await limiter.removeTokens(1);
  return sdk.api.call(endpoint, data);
};
```

3. **Error Handling & Monitoring**

```javascript
// Comprehensive error handling
const handleApiError = error => {
  // Log error for monitoring
  console.error('API Error:', {
    message: error.message,
    endpoint: error.endpoint,
    statusCode: error.statusCode,
    timestamp: new Date().toISOString(),
  });

  // Handle different error types
  switch (error.code) {
    case 'RATE_LIMIT_EXCEEDED':
      // Implement exponential backoff
      return retryWithBackoff(() => makeApiCall(endpoint, data));

    case 'AUTHENTICATION_FAILED':
      // Refresh API key
      return refreshApiKey();

    case 'NETWORK_ERROR':
      // Switch to backup endpoint
      return useBackupEndpoint();

    default:
      throw error;
  }
};
```

### Performance Optimization

1. **Connection Pooling**

```javascript
// Configure connection pooling
const sdk = new WRFCoinSDK({
  apiKey: process.env.WRFCOIN_API_KEY,
  network: 'mainnet',

  // Connection pool settings
  pool: {
    maxConnections: 20,
    idleTimeout: 30000,
    keepAlive: true,
  },

  // Retry configuration
  retry: {
    attempts: 3,
    delay: 1000,
    backoff: 'exponential',
  },
});
```

2. **Caching Strategy**

```javascript
import Redis from 'ioredis';

const redis = new Redis(process.env.REDIS_URL);

// Cache frequently accessed data
const getCachedWeatherData = async stationId => {
  const cacheKey = `weather:${stationId}`;

  // Try cache first
  const cached = await redis.get(cacheKey);
  if (cached) {
    return JSON.parse(cached);
  }

  // Fetch from API
  const data = await sdk.weather.getStationData(stationId);

  // Cache for 5 minutes
  await redis.setex(cacheKey, 300, JSON.stringify(data));

  return data;
};
```

3. **Batch Operations**

```javascript
// Batch multiple operations
const batchWeatherSubmissions = async submissions => {
  const batchSize = 10;
  const results = [];

  for (let i = 0; i < submissions.length; i += batchSize) {
    const batch = submissions.slice(i, i + batchSize);
    const batchResults = await Promise.all(batch.map(submission => sdk.weather.submit(submission)));
    results.push(...batchResults);
  }

  return results;
};
```

### Monitoring & Alerting

```javascript
// Health check endpoint
app.get('/health', async (req, res) => {
  try {
    // Check API connectivity
    const status = await sdk.blockchain.getStatus();

    // Check database connectivity
    await database.ping();

    // Check external services
    const serviceChecks = await Promise.all([
      checkRedis(),
      checkWeatherAPI(),
      checkBlockchainNode(),
    ]);

    res.json({
      status: 'healthy',
      services: {
        blockchain: 'up',
        database: 'up',
        redis: serviceChecks[0] ? 'up' : 'down',
        weatherAPI: serviceChecks[1] ? 'up' : 'down',
        blockchainNode: serviceChecks[2] ? 'up' : 'down',
      },
      uptime: process.uptime(),
      timestamp: new Date().toISOString(),
    });
  } catch (error) {
    res.status(503).json({
      status: 'unhealthy',
      error: error.message,
      timestamp: new Date().toISOString(),
    });
  }
});
```

## Support & Resources

### Official Resources

- **Developer Portal**: [localhost:5000/developer](http://localhost:5000/developer)
- **API Documentation**: [localhost:3003](http://localhost:3003)
- **SDK Downloads**: Available in Developer Portal
- **Interactive API Explorer**: [localhost:3003/api-docs](http://localhost:3003/api-docs)

### Community & Support

- **GitHub Repository**: [github.com/wrfcoin/sdk](https://github.com/wrfcoin/sdk)
- **Discord Community**: [discord.gg/wrfcoin](https://discord.gg/wrfcoin)
- **Developer Forum**: [forum.wrfcoin.org](https://forum.wrfcoin.org)
- **Stack Overflow**: Tag questions with `wrfcoin`

### Getting Help

1. **Check the Documentation**: Start with the comprehensive API docs
2. **Search Existing Issues**: Check GitHub issues for similar problems
3. **Join Discord**: Get real-time help from the community
4. **Contact Support**: Email developers@wrfcoin.org for technical issues

### Stay Updated

- **Developer Newsletter**: Subscribe for SDK updates and new features
- **Release Notes**: Follow GitHub releases for changelog
- **Blog**: [blog.wrfcoin.org](https://blog.wrfcoin.org) for technical articles
- **Twitter**: [@wrfcoin_dev](https://twitter.com/wrfcoin_dev) for announcements

### Contributing

We welcome contributions to the WRFCoin ecosystem:

1. **SDK Improvements**: Submit PRs to enhance SDK functionality
2. **Documentation**: Help improve guides and tutorials
3. **Bug Reports**: Report issues to help improve platform stability
4. **Feature Requests**: Suggest new features for the platform

---

**Ready to build on WRFCoin?** Start with the [Developer Portal](http://localhost:5000/developer)
and create your first API key today!
