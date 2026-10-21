# Developer Guide

**Date Created:** May 17, 2025  
**Last Edited:** May 26, 2025

Comprehensive resources for building on the Wrfcoin blockchain

## API Reference

### REST API

The Wrfcoin REST API provides programmatic access to the blockchain's core functionality through
standard HTTP methods.

```
# Example: Get blockchain status
curl -X GET https://api.wrfcoin.network/v1/status
```

## Current API Endpoints

### Core Blockchain APIs

- `/api/blockchain/status` - Get real-time blockchain status
- `/api/blockchain/blocks` - Query block data and computational blocks
- `/api/blockchain/transactions` - Submit and track transactions
- `/api/blockchain/validators` - View validator network status

### Cross-Chain Integration APIs

- `/api/cross-chain/bridges` - Manage 13 bridge protocol connections
- `/api/cross-chain/assets` - Cross-chain asset transfers and wrapping
- `/api/cross-chain/messages` - Inter-chain messaging services

### Weather & Environmental Data

- `/api/weather/stations` - IoT sensor network data
- `/api/weather/forecasts` - AI-generated weather predictions
- `/api/weather/validation` - Data quality verification services

### Computational Services

- `/api/compute/nodes` - Distributed computing resource management
- `/api/compute/jobs` - Submit computational workloads
- `/api/vm/execute` - WVM (Wrfcoin Virtual Machine) execution

### Governance & Treasury

- `/api/governance/proposals` - View and submit governance proposals
- `/api/governance/voting` - Participate in network governance
- `/api/treasury/allocations` - Treasury fund management

### IoT & Sensor Networks

- `/api/iot/devices` - Register and manage IoT devices
- `/api/sensors/readings` - Real-time environmental data
- `/api/sensors/networks` - Global sensor network status

### GraphQL API

For more complex queries, Wrfcoin offers a powerful GraphQL API that allows you to request exactly
the data you need.

```graphql
# Example GraphQL query
query {
  blocks(last: 5) {
    height
    timestamp
    transactions {
      id
      type
      amount
    }
  }
}
```

## SDKs & Tools

### JavaScript SDK

The JavaScript SDK enables web and Node.js applications to interact with the Wrfcoin blockchain.

```javascript
// Installation
npm install @wrfcoin/sdk

// Usage
import { WrfcoinClient } from '@wrfcoin/sdk';

const client = new WrfcoinClient({
  network: 'testnet',
  apiKey: 'your-api-key'
});

// Get account balance
const balance = await client.getBalance('wrf1abc123def456');
console.log(`Balance: ${balance} WRF`);
```

### Rust SDK

The native Rust SDK provides low-level access to blockchain functionality with maximum performance.

```rust
// In Cargo.toml
[dependencies]
wrfcoin_sdk = "0.5.2"

// In your code
use wrfcoin_sdk::Client;

fn main() {
    let client = Client::new("testnet", Some("your-api-key"));
    let account = "wrf1abc123def456";

    match client.get_balance(account) {
        Ok(balance) => println!("Balance: {} WRF", balance),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

### Python SDK

For data science, machine learning, and general scripting, the Python SDK provides convenient access
to Wrfcoin.

```python
# Installation
pip install wrfcoin-sdk

# Usage
from wrfcoin import Client

client = Client(network='testnet', api_key='your-api-key')
account = 'wrf1abc123def456'

# Get account balance
balance = client.get_balance(account)
print(f"Balance: {balance} WRF")
```

## Smart Contract Development

Wrfcoin supports smart contracts in multiple languages:

1. **Rust** - Using the native smart contract framework
2. **WebAssembly** - For portable cross-chain contracts
3. **Solidity** - Through the EVM compatibility layer

### Contract Example

```rust
use wrfcoin_contract::prelude::*;

#[contract]
struct WeatherDataContract {
    owner: Address,
    data_providers: Map<Address, bool>,
    temperature_records: Vec<TemperatureRecord>,
}

#[derive(Serialize, Deserialize)]
struct TemperatureRecord {
    timestamp: u64,
    latitude: f64,
    longitude: f64,
    temperature: f64,
    provider: Address,
}

#[contractimpl]
impl WeatherDataContract {
    pub fn new(ctx: InitContext) -> Self {
        Self {
            owner: ctx.sender(),
            data_providers: Map::new(),
            temperature_records: Vec::new(),
        }
    }

    pub fn add_provider(&mut self, ctx: TxContext, provider: Address) -> Result<(), ContractError> {
        self.only_owner(&ctx)?;
        self.data_providers.insert(provider, true);
        Ok(())
    }

    pub fn submit_data(&mut self, ctx: TxContext, lat: f64, lng: f64, temp: f64) -> Result<(), ContractError> {
        let sender = ctx.sender();
        if !self.data_providers.get(&sender).unwrap_or(false) {
            return Err(ContractError::NotAuthorized);
        }

        self.temperature_records.push(TemperatureRecord {
            timestamp: ctx.block_time(),
            latitude: lat,
            longitude: lng,
            temperature: temp,
            provider: sender,
        });

        Ok(())
    }

    fn only_owner(&self, ctx: &TxContext) -> Result<(), ContractError> {
        if ctx.sender() != self.owner {
            return Err(ContractError::NotAuthorized);
        }
        Ok(())
    }
}
```
