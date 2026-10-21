# Wrfcoin SDK Complete Reference

**Date Created:** May 19, 2025  
**Last Edited:** May 26, 2025

## Multi-Language SDK for Climate Blockchain Development

Comprehensive SDK featuring 13 cross-chain bridge protocols, AI-enhanced validation systems, and
comprehensive IoT sensor network integrations.

## JavaScript/TypeScript SDK

### Installation

```bash
npm install @wrfcoin/sdk
```

### Basic Setup

```typescript
import { WrfcoinClient, NetworkConfig } from '@wrfcoin/sdk';

const config: NetworkConfig = {
  network: 'mainnet', // 'testnet' | 'mainnet' | 'devnet'
  apiKey: 'your-api-key',
  endpoint: 'https://api.wrfcoin.network',
  timeout: 30000,
};

const client = new WrfcoinClient(config);
```

### Core Methods

#### Account Management

```typescript
// Get account balance
const balance = await client.account.getBalance('wrf1abc123...');

// Get account info
const accountInfo = await client.account.getInfo('wrf1abc123...');

// Create new account
const newAccount = await client.account.create({
  mnemonic: true, // Generate mnemonic phrase
  keyType: 'secp256k1',
});
```

#### Transaction Methods

```typescript
// Send WRF tokens
const txHash = await client.transaction.send({
  to: 'wrf1def456...',
  amount: '100.5',
  memo: 'Payment for weather data',
});

// Send custom tokens
const customTx = await client.transaction.sendToken({
  to: 'wrf1def456...',
  tokenAddress: 'wrf1token123...',
  amount: '50.0',
});

// Multi-send transaction
const multiTx = await client.transaction.multiSend({
  outputs: [
    { to: 'wrf1addr1...', amount: '10.0' },
    { to: 'wrf1addr2...', amount: '20.0' },
  ],
});
```

#### Weather Data Access

```typescript
// Get current weather
const currentWeather = await client.weather.getCurrent({
  latitude: 40.7128,
  longitude: -74.006,
});

// Get weather forecast
const forecast = await client.weather.getForecast({
  latitude: 40.7128,
  longitude: -74.006,
  days: 7,
});

// Get historical weather
const historical = await client.weather.getHistorical({
  latitude: 40.7128,
  longitude: -74.006,
  startDate: '2024-01-01',
  endDate: '2024-01-31',
});

// Submit weather data (for data contributors)
const submission = await client.weather.submit({
  latitude: 40.7128,
  longitude: -74.006,
  temperature: 22.5,
  humidity: 65,
  pressure: 1013.25,
  timestamp: new Date().toISOString(),
});
```

#### Computational NFTs

```typescript
// Create computational NFT
const nft = await client.nft.createComputational({
  name: 'Climate Model v2.1',
  description: 'Advanced climate prediction model',
  computationType: 'climate_modeling',
  inputSchema: {
    temperature: 'number',
    humidity: 'number',
    pressure: 'number',
  },
  code: wasmByteCode, // WebAssembly bytecode
  resources: {
    cpu: 2,
    memory: 1024, // MB
    storage: 100, // MB
  },
});

// Execute computational NFT
const result = await client.nft.execute({
  nftId: 'nft_123...',
  inputs: {
    temperature: 25.0,
    humidity: 70,
    pressure: 1015.0,
  },
});
```

#### Cross-Chain Operations

```typescript
// Bridge tokens to Ethereum
const bridgeTx = await client.bridge.sendToEthereum({
  amount: '100.0',
  ethereumAddress: '0x742d35Cc6635C0532925a3b8D45C_recipient',
  gasLimit: 21000,
});

// Get bridge status
const bridgeStatus = await client.bridge.getStatus('bridge_tx_123...');

// Bridge weather data
const dataBridge = await client.bridge.bridgeWeatherData({
  targetChain: 'polygon',
  dataType: 'temperature_records',
  timeRange: 'last_24_hours',
  region: 'northeast_us',
});
```

#### Staking & Governance

```typescript
// Delegate tokens to validator
const delegation = await client.staking.delegate({
  validatorAddress: 'wrf1validator123...',
  amount: '1000.0',
});

// Vote on governance proposal
const vote = await client.governance.vote({
  proposalId: 'prop_123',
  option: 'yes', // 'yes' | 'no' | 'abstain'
  amount: '500.0', // voting power
});

// Create governance proposal
const proposal = await client.governance.createProposal({
  title: 'Increase block size limit',
  description: 'Proposal to increase block size from 1MB to 2MB',
  type: 'parameter_change',
  changes: [
    {
      subspace: 'consensus',
      key: 'max_block_size',
      value: '2097152',
    },
  ],
  deposit: '1000.0',
});
```

## Python SDK

### Installation

```bash
pip install wrfcoin-sdk
```

### Basic Usage

```python
from wrfcoin import WrfcoinClient

# Initialize client
client = WrfcoinClient(
    network='mainnet',
    api_key='your-api-key'
)

# Get account balance
balance = client.account.get_balance('wrf1abc123...')
print(f"Balance: {balance} WRF")

# Get weather data
weather = client.weather.get_current(
    latitude=40.7128,
    longitude=-74.0060
)
print(f"Temperature: {weather.temperature}°C")

# Send transaction
tx_hash = client.transaction.send(
    to='wrf1def456...',
    amount=100.5,
    memo='Python SDK transaction'
)
print(f"Transaction hash: {tx_hash}")
```

### Data Science Integration

```python
import pandas as pd
import numpy as np
from wrfcoin import WrfcoinClient

client = WrfcoinClient(network='mainnet', api_key='your-key')

# Get historical weather data for analysis
weather_data = client.weather.get_historical_bulk(
    locations=[
        {'lat': 40.7128, 'lng': -74.0060, 'name': 'NYC'},
        {'lat': 34.0522, 'lng': -118.2437, 'name': 'LA'},
        {'lat': 41.8781, 'lng': -87.6298, 'name': 'Chicago'}
    ],
    start_date='2023-01-01',
    end_date='2023-12-31'
)

# Convert to pandas DataFrame
df = pd.DataFrame(weather_data)

# Perform climate analysis
correlation_matrix = df[['temperature', 'humidity', 'pressure']].corr()
print(correlation_matrix)

# Submit analysis results as computational NFT
analysis_nft = client.nft.create_computational(
    name='Climate Correlation Analysis 2023',
    description='Statistical analysis of weather patterns across major US cities',
    computation_type='statistical_analysis',
    input_data=weather_data,
    results=correlation_matrix.to_dict(),
    methodology='Pearson correlation analysis'
)
```

## Rust SDK

### Cargo.toml

```toml
[dependencies]
wrfcoin-sdk = "0.5.2"
tokio = { version = "1.0", features = ["full"] }
```

### Basic Usage

```rust
use wrfcoin_sdk::{WrfcoinClient, Config, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config {
        network: "mainnet".to_string(),
        api_key: Some("your-api-key".to_string()),
        endpoint: "https://api.wrfcoin.network".to_string(),
        ..Default::default()
    };

    let client = WrfcoinClient::new(config).await?;

    // Get account balance
    let balance = client.account().get_balance("wrf1abc123...").await?;
    println!("Balance: {} WRF", balance);

    // Get weather data
    let weather = client.weather().get_current(40.7128, -74.0060).await?;
    println!("Temperature: {}°C", weather.temperature);

    // Send transaction
    let tx_hash = client.transaction()
        .send("wrf1def456...", 100.5, Some("Rust SDK transaction"))
        .await?;
    println!("Transaction hash: {}", tx_hash);

    Ok(())
}
```

### High-Performance Operations

```rust
use wrfcoin_sdk::{WrfcoinClient, BatchOperation, WeatherQuery};
use std::collections::HashMap;

async fn batch_weather_processing(client: &WrfcoinClient) -> Result<()> {
    // Batch weather data requests for high performance
    let locations = vec![
        (40.7128, -74.0060), // NYC
        (34.0522, -118.2437), // LA
        (41.8781, -87.6298), // Chicago
    ];

    let queries: Vec<WeatherQuery> = locations.iter()
        .map(|(lat, lng)| WeatherQuery::current(*lat, *lng))
        .collect();

    // Execute all queries in parallel
    let results = client.weather().batch_query(queries).await?;

    // Process results
    for (i, weather_data) in results.iter().enumerate() {
        println!("Location {}: {}°C", i, weather_data.temperature);
    }

    Ok(())
}

// Custom validator node operations
async fn validator_operations(client: &WrfcoinClient) -> Result<()> {
    // Check validator status
    let validator_info = client.validator()
        .get_info("wrf1validator123...")
        .await?;

    // Submit new block proposal
    let block_proposal = client.validator()
        .propose_block(validator_info.next_height)
        .await?;

    // Vote on block proposals
    client.validator()
        .vote_block(block_proposal.hash, true)
        .await?;

    Ok(())
}
```

## Go SDK

### Installation

```bash
go get github.com/wrfcoin/go-sdk
```

### Basic Usage

```go
package main

import (
    "context"
    "fmt"
    "log"

    "github.com/wrfcoin/go-sdk/client"
    "github.com/wrfcoin/go-sdk/types"
)

func main() {
    // Initialize client
    config := client.Config{
        Network: "mainnet",
        APIKey:  "your-api-key",
        Endpoint: "https://api.wrfcoin.network",
    }

    c, err := client.New(config)
    if err != nil {
        log.Fatal(err)
    }

    ctx := context.Background()

    // Get account balance
    balance, err := c.Account.GetBalance(ctx, "wrf1abc123...")
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Balance: %s WRF\n", balance.String())

    // Get weather data
    weather, err := c.Weather.GetCurrent(ctx, &types.LocationQuery{
        Latitude:  40.7128,
        Longitude: -74.0060,
    })
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Temperature: %.2f°C\n", weather.Temperature)

    // Send transaction
    txHash, err := c.Transaction.Send(ctx, &types.SendRequest{
        To:     "wrf1def456...",
        Amount: types.NewCoin("wrf", 100500000), // 100.5 WRF
        Memo:   "Go SDK transaction",
    })
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Transaction hash: %s\n", txHash)
}
```

## Error Handling

### JavaScript/TypeScript

```typescript
import { WrfcoinError, ErrorCode } from '@wrfcoin/sdk';

try {
  const balance = await client.account.getBalance('invalid-address');
} catch (error) {
  if (error instanceof WrfcoinError) {
    switch (error.code) {
      case ErrorCode.INVALID_ADDRESS:
        console.error('Invalid address format');
        break;
      case ErrorCode.INSUFFICIENT_FUNDS:
        console.error('Not enough funds');
        break;
      case ErrorCode.NETWORK_ERROR:
        console.error('Network connectivity issue');
        break;
      default:
        console.error('Unknown error:', error.message);
    }
  }
}
```

### Python

```python
from wrfcoin import WrfcoinClient, WrfcoinError, ErrorCode

try:
    balance = client.account.get_balance('invalid-address')
except WrfcoinError as e:
    if e.code == ErrorCode.INVALID_ADDRESS:
        print('Invalid address format')
    elif e.code == ErrorCode.INSUFFICIENT_FUNDS:
        print('Not enough funds')
    elif e.code == ErrorCode.NETWORK_ERROR:
        print('Network connectivity issue')
    else:
        print(f'Unknown error: {e.message}')
```

### Rust

```rust
use wrfcoin_sdk::{WrfcoinError, ErrorCode};

match client.account().get_balance("invalid-address").await {
    Ok(balance) => println!("Balance: {}", balance),
    Err(WrfcoinError::InvalidAddress) => eprintln!("Invalid address format"),
    Err(WrfcoinError::InsufficientFunds) => eprintln!("Not enough funds"),
    Err(WrfcoinError::NetworkError(msg)) => eprintln!("Network error: {}", msg),
    Err(e) => eprintln!("Unknown error: {}", e),
}
```

## Advanced Features

### Real-Time Subscriptions

```typescript
// Subscribe to account updates
const accountSubscription = client.account.subscribe('wrf1abc123...', {
  onUpdate: update => {
    console.log('Account updated:', update);
  },
  onError: error => {
    console.error('Subscription error:', error);
  },
});

// Subscribe to weather alerts
const weatherSubscription = client.weather.subscribe({
  region: 'northeast_us',
  alertTypes: ['severe_weather', 'temperature_anomaly'],
  onAlert: alert => {
    console.log('Weather alert:', alert);
  },
});

// Unsubscribe when done
accountSubscription.unsubscribe();
weatherSubscription.unsubscribe();
```

### Custom Gas Strategies

```typescript
// Advanced transaction with custom gas
const tx = await client.transaction.send({
  to: 'wrf1def456...',
  amount: '100.0',
  gas: {
    strategy: 'fast', // 'slow' | 'standard' | 'fast' | 'custom'
    customGasPrice: '0.001', // Only for 'custom' strategy
    gasLimit: 21000,
  },
  confirmations: 3, // Wait for 3 confirmations
});
```

This comprehensive SDK reference covers all major programming languages and use cases for building
on the Wrfcoin climate blockchain platform.
