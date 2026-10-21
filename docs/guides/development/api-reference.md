# Wrfcoin API Reference

This comprehensive reference documents the APIs available in the Wrfcoin platform.

## Table of Contents

1. [Introduction](#introduction)
2. [Authentication](#authentication)
3. [Core API Endpoints](#core-api-endpoints)
4. [Blockchain API](#blockchain-api)
5. [Wallet API](#wallet-api)
6. [Weather Data API](#weather-data-api)
7. [Governance API](#governance-api)
8. [Cross-Chain API](#cross-chain-api)
9. [Computational API](#computational-api)
10. [WebSocket API](#websocket-api)
11. [Rate Limits and Quotas](#rate-limits-and-quotas)
12. [Error Handling](#error-handling)
13. [Versioning](#versioning)

## Introduction

The Wrfcoin API provides programmatic access to the Wrfcoin blockchain and related services. It
follows REST principles and returns responses in JSON format. The base URL for all API endpoints is:

```
https://api.wrfcoin.network/v1
```

For testnet, use:

```
https://testnet-api.wrfcoin.network/v1
```

## Authentication

The API supports several authentication methods:

### API Key Authentication

For server-to-server communications, include your API key in the request header:

```http
GET /v1/blockchain/status HTTP/1.1
Host: api.wrfcoin.network
X-API-Key: your-api-key-here
```

You can request an API key through the developer portal at
[https://developers.wrfcoin.network](https://developers.wrfcoin.network).

### Wallet Authentication

For user-specific operations, authenticate using a wallet signature:

```http
GET /v1/wallet/transactions HTTP/1.1
Host: api.wrfcoin.network
X-Wallet-Address: WRF1AbCdEfGhIjKlMnOpQrStUvWxYz12345678
X-Wallet-Signature: 0x7b2276616c7565223a22576562417574686e222c227...
X-Wallet-Message: Authentication request for Wrfcoin API - 1683112800
```

The signature should be created by signing the message with the user's private key.

### JWT Authentication

For multi-request sessions, obtain a JWT token:

```http
POST /v1/auth/token HTTP/1.1
Host: api.wrfcoin.network
Content-Type: application/json

{
  "wallet_address": "WRF1AbCdEfGhIjKlMnOpQrStUvWxYz12345678",
  "signature": "0x7b2276616c7565223a22576562417574686e222c227...",
  "message": "Authentication request for Wrfcoin API - 1683112800"
}
```

Then use the token in subsequent requests:

```http
GET /v1/wallet/transactions HTTP/1.1
Host: api.wrfcoin.network
Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
```

### Anonymous Access

Some endpoints don't require authentication and can be accessed anonymously. These include public
blockchain data and general information.

## Core API Endpoints

### Get API Status

```http
GET /v1/status
```

Response:

```json
{
  "status": "healthy",
  "version": "1.5.2",
  "uptime": 1209600,
  "timestamp": 1683112800,
  "network": "mainnet",
  "chain_id": "wrfcoin-1"
}
```

### Get API Version

```http
GET /v1/version
```

Response:

```json
{
  "version": "1.5.2",
  "build": "20250418-a7f9b2c",
  "supported_versions": ["1.4.x", "1.5.x"],
  "deprecated_versions": ["1.3.x"],
  "latest_version": "1.5.2"
}
```

## Blockchain API

### Get Blockchain Status

```http
GET /v1/blockchain/status
```

Response:

```json
{
  "height": 1023456,
  "hash": "0x7a9b4f2c8d1e0f3a4b5c6d7e8f9a0b1c2d3e4f5a",
  "time": 1683112800,
  "validators_count": 50,
  "active_validators": 48,
  "average_block_time": 5.2,
  "total_transactions": 15783421,
  "pending_transactions": 56,
  "tps": 12.3,
  "current_difficulty": 45723840,
  "network_hashrate": "1.23 PH/s",
  "staked_tokens": 526483219
}
```

### Get Block Details

```http
GET /v1/blockchain/blocks/{height|hash}
```

Response:

```json
{
  "height": 1023456,
  "hash": "0x7a9b4f2c8d1e0f3a4b5c6d7e8f9a0b1c2d3e4f5a",
  "previous_hash": "0x8b9c5f3d9e2f1g4b5c6d7e8f9a0b1c2d3e4f5a6",
  "timestamp": 1683112800,
  "validator": "WRF1ValidatorAddress123456789",
  "size": 24680,
  "gas_used": 456789,
  "gas_limit": 8000000,
  "transactions_count": 32,
  "transactions": [
    "0x1a2b3c4d5e6f7g8h9i0j1k2l3m4n5o6p7q8r9s0t",
    "0x2b3c4d5e6f7g8h9i0j1k2l3m4n5o6p7q8r9s0t1u"
  ],
  "weather_data": {
    "data_count": 8,
    "data_hash": "0x9c8b7a6f5e4d3c2b1a0f9e8d7c6b5a4f3e2d1c0b",
    "data_types": ["temperature", "humidity", "pressure"]
  },
  "computational_data": {
    "present": true,
    "model": "SimpleWeatherPredictor",
    "version": "1.0.2",
    "hash": "0xa1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9t"
  }
}
```

### Get Latest Blocks

```http
GET /v1/blockchain/blocks?limit={limit}&offset={offset}
```

Parameters:

- `limit`: Number of blocks to return (default: 10, max: 100)
- `offset`: Offset from the latest block (default: 0)

Response:

```json
{
  "blocks": [
    {
      "height": 1023456,
      "hash": "0x7a9b4f2c8d1e0f3a4b5c6d7e8f9a0b1c2d3e4f5a",
      "timestamp": 1683112800,
      "validator": "WRF1ValidatorAddress123456789",
      "transactions_count": 32,
      "size": 24680
    },
    {
      "height": 1023455,
      "hash": "0x8b9c5f3d9e2f1g4b5c6d7e8f9a0b1c2d3e4f5a6",
      "timestamp": 1683112795,
      "validator": "WRF1ValidatorAddress987654321",
      "transactions_count": 28,
      "size": 22340
    }
  ],
  "total": 1023456,
  "limit": 10,
  "offset": 0
}
```

### Get Transaction

```http
GET /v1/blockchain/transactions/{txid}
```

Response:

```json
{
  "txid": "0x1a2b3c4d5e6f7g8h9i0j1k2l3m4n5o6p7q8r9s0t",
  "block_height": 1023456,
  "block_hash": "0x7a9b4f2c8d1e0f3a4b5c6d7e8f9a0b1c2d3e4f5a",
  "timestamp": 1683112800,
  "type": "regular", // regular, weather_data, computational, governance, nft
  "from": "WRF1SenderAddress123456789",
  "to": "WRF1RecipientAddress123456789",
  "amount": "123.456",
  "fee": "0.0123",
  "gas_used": 21000,
  "gas_price": "0.0000001",
  "nonce": 42,
  "status": "confirmed",
  "confirmations": 10,
  "data": {}, // Additional data based on transaction type
  "signature": "0x2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9t0u1v"
}
```

### Get Pending Transactions

```http
GET /v1/blockchain/pending-transactions
```

Response:

```json
{
  "transactions": [
    {
      "txid": "0x3c4d5e6f7g8h9i0j1k2l3m4n5o6p7q8r9s0t1u2v",
      "type": "regular",
      "from": "WRF1SenderAddress123456789",
      "to": "WRF1RecipientAddress123456789",
      "amount": "45.678",
      "fee": "0.0234",
      "timestamp": 1683112830,
      "status": "pending"
    }
  ],
  "count": 56,
  "limit": 10,
  "offset": 0
}
```

### Submit Transaction

```http
POST /v1/blockchain/transactions
Content-Type: application/json

{
  "type": "regular",
  "from": "WRF1SenderAddress123456789",
  "to": "WRF1RecipientAddress123456789",
  "amount": "10.5",
  "fee": "0.001",
  "nonce": 43,
  "data": {},
  "signature": "0x4d5e6f7g8h9i0j1k2l3m4n5o6p7q8r9s0t1u2v3w"
}
```

Response:

```json
{
  "txid": "0x5e6f7g8h9i0j1k2l3m4n5o6p7q8r9s0t1u2v3w4x",
  "status": "pending",
  "timestamp": 1683112850,
  "estimated_confirmation_time": 1683112860
}
```

### Get Network Stats

```http
GET /v1/blockchain/stats
```

Response:

```json
{
  "height": 1023456,
  "transactions_total": 15783421,
  "transactions_per_second": 12.3,
  "unique_addresses": 389562,
  "active_validators": 48,
  "total_validators": 50,
  "average_block_time": 5.2,
  "total_supply": "1000000000",
  "circulating_supply": "750000000",
  "staked_tokens": "526483219",
  "governance_tokens": "125000000",
  "reward_pool": "230000000",
  "inflation_rate": 0.03,
  "network_hashrate": "1.23 PH/s",
  "weather_stations": 3892,
  "computational_nodes": 567
}
```

## Wallet API

### Get Wallet Information

```http
GET /v1/wallet/{address}
```

Response:

```json
{
  "address": "WRF1AbCdEfGhIjKlMnOpQrStUvWxYz12345678",
  "balance": "1234.5678",
  "staked": "500.0000",
  "delegated": "200.0000",
  "rewards_pending": "12.3456",
  "transaction_count": 157,
  "first_activity": 1672531200,
  "last_activity": 1683112800,
  "tokens": [
    {
      "symbol": "WRF",
      "name": "Wrfcoin",
      "balance": "1234.5678"
    },
    {
      "symbol": "WETH",
      "name": "Wrapped Ethereum",
      "balance": "10.5"
    }
  ],
  "nfts_count": 3,
  "validator_status": "active", // active, inactive, jailed, none
  "governance_power": 1734.5678
}
```

### Get Wallet Transactions

```http
GET /v1/wallet/{address}/transactions?limit={limit}&offset={offset}&type={type}
```

Parameters:

- `limit`: Number of transactions to return (default: 10, max: 100)
- `offset`: Pagination offset (default: 0)
- `type`: Filter by transaction type (optional)

Response:

```json
{
  "transactions": [
    {
      "txid": "0x1a2b3c4d5e6f7g8h9i0j1k2l3m4n5o6p7q8r9s0t",
      "type": "regular",
      "from": "WRF1SenderAddress123456789",
      "to": "WRF1RecipientAddress123456789",
      "amount": "123.456",
      "fee": "0.0123",
      "timestamp": 1683112800,
      "status": "confirmed",
      "block_height": 1023456
    }
  ],
  "total": 157,
  "limit": 10,
  "offset": 0
}
```

### Create Wallet

```http
POST /v1/wallet/create
Content-Type: application/json

{
  "encryption_password": "optional-encryption-password"
}
```

Response:

```json
{
  "address": "WRF1NewlyCreatedWalletAddress12345678",
  "private_key": "0x7b2276616c7565223a22576562417574686e222c227...",
  "mnemonic": "word1 word2 word3 word4 ... word12",
  "encrypted": false,
  "created_at": 1683112800
}
```

### Import Wallet

```http
POST /v1/wallet/import
Content-Type: application/json

{
  "private_key": "0x7b2276616c7565223a22576562417574686e222c227...",
  "encryption_password": "optional-encryption-password"
}
```

Response:

```json
{
  "address": "WRF1ImportedWalletAddress12345678",
  "imported": true,
  "encrypted": false,
  "created_at": 1683112800
}
```

### Get Wallet NFTs

```http
GET /v1/wallet/{address}/nfts
```

Response:

```json
{
  "nfts": [
    {
      "id": "NFT-123456",
      "name": "Weather Station #42",
      "type": "weather_station",
      "image_url": "https://assets.wrfcoin.network/nfts/123456.png",
      "metadata": {
        "location": "New York City",
        "coordinates": {
          "latitude": 40.7128,
          "longitude": -74.006
        },
        "active_since": 1672531200
      },
      "owner": "WRF1AbCdEfGhIjKlMnOpQrStUvWxYz12345678",
      "created_at": 1672531200,
      "transferable": true
    }
  ],
  "count": 3,
  "limit": 10,
  "offset": 0
}
```

### Get Wallet Weather Stations

```http
GET /v1/wallet/{address}/weather-stations
```

Response:

```json
{
  "stations": [
    {
      "id": "WS-NYC-001",
      "name": "Central Park Weather Station",
      "type": "official",
      "location": {
        "latitude": 40.7829,
        "longitude": -73.9654,
        "elevation": 42.7,
        "name": "Central Park"
      },
      "status": "active",
      "nft_id": "NFT-123456",
      "last_report": 1683112700,
      "data_points_submitted": 15783,
      "rewards_earned": "123.456",
      "reputation_score": 98.7
    }
  ],
  "count": 2,
  "limit": 10,
  "offset": 0
}
```

## Weather Data API

### Get Latest Weather Data

```http
GET /v1/weather/latest?location={location}&coordinates={lat,lon}
```

Parameters:

- `location`: Location name (optional if coordinates provided)
- `coordinates`: Latitude,longitude (optional if location provided)

Response:

```json
{
  "location": {
    "name": "New York City",
    "coordinates": {
      "latitude": 40.7128,
      "longitude": -74.006
    }
  },
  "timestamp": 1683112800,
  "data": {
    "temperature": 22.5,
    "humidity": 65,
    "pressure": 1013.2,
    "wind_speed": 12.3,
    "wind_direction": 270,
    "precipitation": 0,
    "cloud_cover": 30,
    "visibility": 10000,
    "uv_index": 6.5
  },
  "source": {
    "station_id": "WS-NYC-001",
    "type": "official",
    "distance": 1.2 // km
  },
  "quality": {
    "confidence": 98,
    "verified": true,
    "consensus_level": "high"
  }
}
```

### Get Historical Weather Data

```http
GET /v1/weather/historical?location={location}&start_time={start}&end_time={end}&metrics={metrics}
```

Parameters:

- `location`: Location name or ID
- `start_time`: Start timestamp (UNIX)
- `end_time`: End timestamp (UNIX)
- `metrics`: Comma-separated list of metrics (optional)

Response:

```json
{
  "location": {
    "name": "New York City",
    "coordinates": {
      "latitude": 40.7128,
      "longitude": -74.006
    }
  },
  "timeframe": {
    "start": 1682726400,
    "end": 1683112800
  },
  "interval": "hourly",
  "data_points": [
    {
      "timestamp": 1682726400,
      "temperature": 21.3,
      "humidity": 68,
      "pressure": 1012.4
    },
    {
      "timestamp": 1682730000,
      "temperature": 22.1,
      "humidity": 67,
      "pressure": 1012.6
    }
  ],
  "sources": [
    {
      "station_id": "WS-NYC-001",
      "type": "official",
      "contribution": 0.8
    },
    {
      "station_id": "WS-NYC-002",
      "type": "amateur",
      "contribution": 0.2
    }
  ],
  "quality": {
    "average_confidence": 95,
    "verified_percentage": 98
  }
}
```

### Get Weather Forecast

```http
GET /v1/weather/forecast?location={location}&days={days}
```

Parameters:

- `location`: Location name or ID
- `days`: Number of days to forecast (default: 7, max: 14)

Response:

```json
{
  "location": {
    "name": "New York City",
    "coordinates": {
      "latitude": 40.7128,
      "longitude": -74.006
    }
  },
  "generated_at": 1683112800,
  "model": {
    "name": "WrfGlobal",
    "version": "3.2.1",
    "resolution": "0.25deg"
  },
  "forecast": [
    {
      "date": "2025-05-20",
      "hourly": [
        {
          "time": "00:00",
          "temperature": 18.5,
          "humidity": 70,
          "precipitation_chance": 10,
          "wind_speed": 8.2,
          "wind_direction": 280
        },
        {
          "time": "01:00",
          "temperature": 18.1,
          "humidity": 71,
          "precipitation_chance": 5,
          "wind_speed": 7.8,
          "wind_direction": 275
        }
      ],
      "daily": {
        "temperature_min": 18.1,
        "temperature_max": 26.3,
        "humidity_avg": 65,
        "precipitation_chance": 15,
        "precipitation_amount": 0,
        "wind_speed_max": 12.5,
        "condition": "partly_cloudy"
      }
    }
  ],
  "confidence": 0.85,
  "verification_data": {
    "past_accuracy": 0.87,
    "consensus_level": "high",
    "verification_nodes": 12
  }
}
```

### Get Weather Stations

```http
GET /v1/weather/stations?near={location}&distance={km}&status={status}
```

Parameters:

- `near`: Location or coordinates (optional)
- `distance`: Search radius in km (default: 10, max: 100)
- `status`: Filter by status (active, maintenance, inactive)

Response:

```json
{
  "stations": [
    {
      "id": "WS-NYC-001",
      "name": "Central Park Weather Station",
      "type": "official",
      "location": {
        "latitude": 40.7829,
        "longitude": -73.9654,
        "elevation": 42.7,
        "name": "Central Park"
      },
      "status": "active",
      "owner": "WRF1StationOwnerAddress12345678",
      "instruments": ["temperature", "humidity", "pressure", "wind", "precipitation"],
      "reporting_interval": 300,
      "last_report": 1683112700,
      "reliability_score": 98.7,
      "distance": 1.2 // km from search point
    }
  ],
  "count": 12,
  "limit": 10,
  "offset": 0
}
```

### Submit Weather Data

```http
POST /v1/weather/data
Content-Type: application/json

{
  "station_id": "WS-NYC-001",
  "timestamp": 1683112800,
  "data": {
    "temperature": 22.5,
    "humidity": 65,
    "pressure": 1013.2,
    "wind_speed": 12.3,
    "wind_direction": 270,
    "precipitation": 0,
    "cloud_cover": 30,
    "visibility": 10000,
    "uv_index": 6.5
  },
  "signature": "0x6f7g8h9i0j1k2l3m4n5o6p7q8r9s0t1u2v3w4x5y"
}
```

Response:

```json
{
  "success": true,
  "txid": "0x7g8h9i0j1k2l3m4n5o6p7q8r9s0t1u2v3w4x5y6z",
  "timestamp": 1683112800,
  "verification_status": "pending",
  "estimated_rewards": "0.0123"
}
```

### Register Weather Station

```http
POST /v1/weather/stations
Content-Type: application/json

{
  "name": "Backyard Weather Station",
  "location": {
    "latitude": 40.7128,
    "longitude": -74.0060,
    "elevation": 10,
    "name": "Downtown NYC"
  },
  "type": "amateur",
  "instruments": [
    {
      "type": "temperature",
      "model": "Davis VP2 Temp-Hum",
      "accuracy": 0.5,
      "last_calibration": 1672531200
    },
    {
      "type": "humidity",
      "model": "Davis VP2 Temp-Hum",
      "accuracy": 3.0,
      "last_calibration": 1672531200
    }
  ],
  "reporting_interval": 300,
  "owner": "WRF1StationOwnerAddress12345678",
  "signature": "0x8h9i0j1k2l3m4n5o6p7q8r9s0t1u2v3w4x5y6z7a"
}
```

Response:

```json
{
  "success": true,
  "station_id": "WS-NYC-123",
  "registration_txid": "0x9i0j1k2l3m4n5o6p7q8r9s0t1u2v3w4x5y6z7a8b",
  "nft_id": "NFT-789012",
  "timestamp": 1683112800,
  "verification_required": true,
  "next_steps": [
    "Complete verification by submitting test data",
    "Install station at registered location",
    "Begin regular data submissions"
  ]
}
```

## Governance API

### Get Governance Proposals

```http
GET /v1/governance/proposals?status={status}&limit={limit}&offset={offset}
```

Parameters:

- `status`: Filter by status (active, passed, rejected, all)
- `limit`: Number of proposals to return (default: 10, max: 100)
- `offset`: Pagination offset (default: 0)

Response:

```json
{
  "proposals": [
    {
      "id": "PROP-12345",
      "title": "Increase Validator Set to 100",
      "description": "This proposal aims to increase the maximum number of validators from 50 to 100 to improve network decentralization.",
      "type": "parameter_change",
      "status": "active",
      "proposer": "WRF1ProposerAddress123456789",
      "created_at": 1682726400,
      "voting_start": 1682812800,
      "voting_end": 1683417600,
      "parameter_changes": [
        {
          "parameter": "max_validators",
          "current_value": "50",
          "proposed_value": "100"
        }
      ],
      "votes": {
        "yes": "15000000",
        "no": "5000000",
        "abstain": "1000000",
        "veto": "500000"
      },
      "threshold": {
        "quorum": 0.4,
        "pass": 0.5,
        "veto": 0.334
      },
      "status_details": {
        "quorum_reached": true,
        "current_result": "passing",
        "estimated_result": "pass"
      }
    }
  ],
  "count": 34,
  "limit": 10,
  "offset": 0
}
```

### Get Governance Proposal

```http
GET /v1/governance/proposals/{id}
```

Response:

```json
{
  "id": "PROP-12345",
  "title": "Increase Validator Set to 100",
  "description": "This proposal aims to increase the maximum number of validators from 50 to 100 to improve network decentralization.",
  "type": "parameter_change",
  "status": "active",
  "proposer": "WRF1ProposerAddress123456789",
  "created_at": 1682726400,
  "voting_start": 1682812800,
  "voting_end": 1683417600,
  "parameter_changes": [
    {
      "parameter": "max_validators",
      "current_value": "50",
      "proposed_value": "100"
    }
  ],
  "votes": {
    "yes": "15000000",
    "no": "5000000",
    "abstain": "1000000",
    "veto": "500000"
  },
  "threshold": {
    "quorum": 0.4,
    "pass": 0.5,
    "veto": 0.334
  },
  "status_details": {
    "quorum_reached": true,
    "current_result": "passing",
    "estimated_result": "pass"
  },
  "vote_distribution": {
    "validators": {
      "yes": 28,
      "no": 12,
      "abstain": 5,
      "veto": 2,
      "not_voted": 3
    },
    "top_voters": [
      {
        "address": "WRF1LargeVoterAddress123456789",
        "vote": "yes",
        "voting_power": "5000000"
      }
    ]
  },
  "discussions": [
    {
      "author": "WRF1DiscussionAuthorAddress12345",
      "content": "I believe this change will help increase network security.",
      "timestamp": 1682900000
    }
  ]
}
```

### Submit Governance Proposal

```http
POST /v1/governance/proposals
Content-Type: application/json

{
  "title": "Increase Weather Data Rewards",
  "description": "This proposal aims to increase the rewards for weather data submissions by 10% to incentivize more data collection.",
  "type": "parameter_change",
  "parameter_changes": [
    {
      "parameter": "weather_data_reward_factor",
      "current_value": "1.0",
      "proposed_value": "1.1"
    }
  ],
  "deposit": "1000", // WRF tokens
  "proposer": "WRF1ProposerAddress123456789",
  "signature": "0xj1k2l3m4n5o6p7q8r9s0t1u2v3w4x5y6z7a8b9c"
}
```

Response:

```json
{
  "success": true,
  "proposal_id": "PROP-67890",
  "txid": "0xk2l3m4n5o6p7q8r9s0t1u2v3w4x5y6z7a8b9c0d",
  "voting_start": 1683112900,
  "voting_end": 1683717600,
  "proposal_url": "https://governance.wrfcoin.network/proposals/PROP-67890"
}
```

### Vote on Proposal

```http
POST /v1/governance/proposals/{id}/vote
Content-Type: application/json

{
  "voter": "WRF1VoterAddress123456789",
  "vote": "yes", // yes, no, abstain, veto
  "memo": "I support this proposal because it will improve data quality.",
  "signature": "0xl3m4n5o6p7q8r9s0t1u2v3w4x5y6z7a8b9c0d1e"
}
```

Response:

```json
{
  "success": true,
  "proposal_id": "PROP-12345",
  "vote": "yes",
  "voting_power": "500000",
  "txid": "0xm4n5o6p7q8r9s0t1u2v3w4x5y6z7a8b9c0d1e2f",
  "timestamp": 1683112800,
  "current_result": "passing"
}
```

### Get Governance Parameters

```http
GET /v1/governance/parameters
```

Response:

```json
{
  "parameters": {
    "max_validators": "50",
    "blocks_per_year": "6311520",
    "inflation_rate": "0.03",
    "voting_period": "604800", // seconds
    "minimum_proposal_deposit": "1000",
    "quorum": "0.4",
    "threshold": "0.5",
    "veto_threshold": "0.334",
    "weather_data_reward_factor": "1.0",
    "validator_commission_max": "0.2",
    "slashing_downtime": "0.01",
    "slashing_double_sign": "0.05"
  },
  "parameter_changes": [
    {
      "parameter": "max_validators",
      "current_value": "50",
      "future_value": "100",
      "effective_height": 1050000
    }
  ]
}
```

## Cross-Chain API

### Get Cross-Chain Status

```http
GET /v1/cross-chain/status
```

Response:

```json
{
  "connected_chains": [
    {
      "name": "Ethereum",
      "id": "ethereum",
      "status": "active",
      "bridge_address": "0x1a2b3c4d5e6f7g8h9i0j1k2l3m4n5o6p7q8r9s0t",
      "block_height": 17250000,
      "last_sync": 1683112790
    },
    {
      "name": "Solana",
      "id": "solana",
      "status": "active",
      "bridge_address": "So1AQZGJQZNsFwt84rEXXfNWzYBxG1wZTSrRs3J2xh9",
      "block_height": 198342000,
      "last_sync": 1683112795
    }
  ],
  "bridge_operations": {
    "pending": 12,
    "completed_24h": 142,
    "volume_24h": "123456.78"
  },
  "health": {
    "status": "healthy",
    "average_confirmation_time": 180, // seconds
    "available_liquidity": {
      "ethereum": "10000.00",
      "solana": "20000.00"
    }
  }
}
```

### Get Cross-Chain Transactions

```http
GET /v1/cross-chain/transactions?status={status}&chain={chain}&limit={limit}&offset={offset}
```

Parameters:

- `status`: Filter by status (pending, completed, failed, all)
- `chain`: Filter by chain ID
- `limit`: Number of transactions to return (default: 10, max: 100)
- `offset`: Pagination offset (default: 0)

Response:

```json
{
  "transactions": [
    {
      "id": "XCTX-12345",
      "type": "token_transfer",
      "source_chain": "wrfcoin",
      "destination_chain": "ethereum",
      "sender": "WRF1SenderAddress123456789",
      "recipient": "0xRecipientAddress123456789",
      "token": "WRF",
      "amount": "100.0",
      "fee": "0.5",
      "status": "completed",
      "created_at": 1683112700,
      "completed_at": 1683112900,
      "source_txid": "0xn5o6p7q8r9s0t1u2v3w4x5y6z7a8b9c0d1e2f3g",
      "destination_txid": "0xSourceChainTransactionId12345689",
      "confirmation_blocks": 30,
      "required_confirmations": 30
    }
  ],
  "count": 142,
  "limit": 10,
  "offset": 0
}
```

### Initiate Cross-Chain Transfer

```http
POST /v1/cross-chain/transfer
Content-Type: application/json

{
  "source_chain": "wrfcoin",
  "destination_chain": "ethereum",
  "token": "WRF",
  "amount": "100.0",
  "sender": "WRF1SenderAddress123456789",
  "recipient": "0xRecipientAddress123456789",
  "fee": "0.5",
  "signature": "0xo6p7q8r9s0t1u2v3w4x5y6z7a8b9c0d1e2f3g4h"
}
```

Response:

```json
{
  "success": true,
  "transfer_id": "XCTX-67890",
  "source_txid": "0xp7q8r9s0t1u2v3w4x5y6z7a8b9c0d1e2f3g4h5i",
  "status": "pending",
  "estimated_completion_time": 1683113100,
  "track_url": "https://bridge.wrfcoin.network/transfers/XCTX-67890"
}
```

## Computational API

### Get Computational Jobs

```http
GET /v1/computational/jobs?status={status}&type={type}&limit={limit}&offset={offset}
```

Parameters:

- `status`: Filter by status (pending, running, completed, failed, all)
- `type`: Filter by job type (forecast, analysis, simulation)
- `limit`: Number of jobs to return (default: 10, max: 100)
- `offset`: Pagination offset (default: 0)

Response:

```json
{
  "jobs": [
    {
      "id": "COMP-12345",
      "type": "forecast",
      "status": "completed",
      "submitter": "WRF1SubmitterAddress123456789",
      "model": {
        "name": "WrfGlobal",
        "version": "3.2.1"
      },
      "input_data": {
        "type": "weather_observations",
        "count": 15320,
        "timeframe": {
          "start": 1682726400,
          "end": 1683112800
        },
        "locations": ["Global"]
      },
      "computation_details": {
        "nodes": 12,
        "cpu_hours": 36.5,
        "memory_gb_hours": 192.0
      },
      "output": {
        "type": "forecast_data",
        "size_mb": 450,
        "data_points": 12960000,
        "reference": "ipfs://QmOutputDataReferenceHash12345"
      },
      "created_at": 1683112800,
      "started_at": 1683112900,
      "completed_at": 1683116400,
      "cost": "25.75",
      "txid": "0xq8r9s0t1u2v3w4x5y6z7a8b9c0d1e2f3g4h5i6j"
    }
  ],
  "count": 56,
  "limit": 10,
  "offset": 0
}
```

### Submit Computational Job

```http
POST /v1/computational/jobs
Content-Type: application/json

{
  "type": "forecast",
  "model": {
    "name": "WrfGlobal",
    "version": "3.2.1"
  },
  "input_data": {
    "type": "weather_observations",
    "source": "recent_observations",
    "parameters": {
      "timeframe": {
        "start": 1682726400,
        "end": 1683112800
      },
      "locations": ["Global"]
    }
  },
  "computation_parameters": {
    "resolution": "0.25deg",
    "forecast_hours": 240,
    "output_interval": 1
  },
  "output_format": "netcdf",
  "max_budget": "50.0",
  "priority": "standard",
  "submitter": "WRF1SubmitterAddress123456789",
  "signature": "0xr9s0t1u2v3w4x5y6z7a8b9c0d1e2f3g4h5i6j7k"
}
```

Response:

```json
{
  "success": true,
  "job_id": "COMP-67890",
  "txid": "0xs0t1u2v3w4x5y6z7a8b9c0d1e2f3g4h5i6j7k8l",
  "status": "pending",
  "estimated_cost": "32.15",
  "estimated_completion_time": 1683117600,
  "track_url": "https://compute.wrfcoin.network/jobs/COMP-67890"
}
```

### Get Computational Job Details

```http
GET /v1/computational/jobs/{id}
```

Response:

```json
{
  "id": "COMP-67890",
  "type": "forecast",
  "status": "running",
  "submitter": "WRF1SubmitterAddress123456789",
  "model": {
    "name": "WrfGlobal",
    "version": "3.2.1"
  },
  "input_data": {
    "type": "weather_observations",
    "count": 15320,
    "timeframe": {
      "start": 1682726400,
      "end": 1683112800
    },
    "locations": ["Global"]
  },
  "computation_parameters": {
    "resolution": "0.25deg",
    "forecast_hours": 240,
    "output_interval": 1
  },
  "computation_details": {
    "nodes": 15,
    "cpu_hours": 45.0,
    "memory_gb_hours": 240.0,
    "progress": 0.35, // 35% complete
    "eta": 1683117600
  },
  "created_at": 1683112800,
  "started_at": 1683113100,
  "estimated_completion": 1683117600,
  "current_cost": "11.25",
  "estimated_final_cost": "32.15",
  "txid": "0xs0t1u2v3w4x5y6z7a8b9c0d1e2f3g4h5i6j7k8l",
  "logs": [
    {
      "timestamp": 1683113100,
      "message": "Job started",
      "level": "info"
    },
    {
      "timestamp": 1683114000,
      "message": "Input data processing complete",
      "level": "info"
    }
  ]
}
```

### Get Available Computation Models

```http
GET /v1/computational/models
```

Response:

```json
{
  "models": [
    {
      "name": "WrfGlobal",
      "version": "3.2.1",
      "type": "forecast",
      "description": "Global weather forecast model",
      "parameters": {
        "resolutions": ["0.25deg", "0.5deg", "1.0deg"],
        "max_forecast_hours": 240,
        "min_output_interval": 1
      },
      "pricing": {
        "base_cost": "10.0",
        "per_hour_forecast": "0.1",
        "resolution_multipliers": {
          "0.25deg": 2.0,
          "0.5deg": 1.5,
          "1.0deg": 1.0
        }
      },
      "available_nodes": 32,
      "average_queue_time": 300, // seconds
      "average_runtime": 3600 // seconds
    }
  ],
  "count": 8,
  "limit": 10,
  "offset": 0
}
```

## WebSocket API

The WebSocket API provides real-time updates for blockchain events, weather data, and more.

### WebSocket Endpoint

```
wss://ws.wrfcoin.network/v1
```

For testnet:

```
wss://testnet-ws.wrfcoin.network/v1
```

### Authentication

Authentication can be done during the connection by providing a token:

```javascript
const ws = new WebSocket('wss://ws.wrfcoin.network/v1?token=your-api-key-here');
```

Or by sending an authentication message:

```javascript
ws.onopen = () => {
  ws.send(
    JSON.stringify({
      action: 'authenticate',
      token: 'your-api-key-here',
    })
  );
};
```

### Available Channels

Subscribe to channels for specific types of updates:

```javascript
ws.send(
  JSON.stringify({
    action: 'subscribe',
    channels: ['blockchain', 'transactions', 'weather'],
  })
);
```

Available channels:

- `blockchain`: Block creation and chain state updates
- `transactions`: Transaction events (creation, confirmation)
- `weather`: Weather data events (new data, station updates)
- `governance`: Governance events (proposals, votes)
- `cross-chain`: Cross-chain transaction events
- `computational`: Computational job events

### Channel: blockchain

Events:

- `BLOCK_CREATED`: New block added to the chain
- `STATE_UPDATE`: Updates to blockchain state (hashrate, difficulty, etc.)

Example message:

```json
{
  "channel": "blockchain",
  "event": "BLOCK_CREATED",
  "data": {
    "height": 1023456,
    "hash": "0x7a9b4f2c8d1e0f3a4b5c6d7e8f9a0b1c2d3e4f5a",
    "transactions_count": 32,
    "timestamp": 1683112800,
    "validator": "WRF1ValidatorAddress123456789"
  }
}
```

### Channel: transactions

Events:

- `TRANSACTION_CREATED`: New transaction added to the mempool
- `TRANSACTION_CONFIRMED`: Transaction included in a block
- `TRANSACTION_FAILED`: Transaction failed to process

Example message:

```json
{
  "channel": "transactions",
  "event": "TRANSACTION_CONFIRMED",
  "data": {
    "txid": "0x1a2b3c4d5e6f7g8h9i0j1k2l3m4n5o6p7q8r9s0t",
    "type": "regular",
    "from": "WRF1SenderAddress123456789",
    "to": "WRF1RecipientAddress123456789",
    "amount": "123.456",
    "block_height": 1023456,
    "confirmations": 1,
    "timestamp": 1683112800
  }
}
```

### Channel: weather

Events:

- `DATA_SUBMITTED`: New weather data submitted
- `STATION_ADDED`: New weather station registered
- `STATION_UPDATED`: Weather station information updated

Example message:

```json
{
  "channel": "weather",
  "event": "DATA_SUBMITTED",
  "data": {
    "station_id": "WS-NYC-001",
    "location": {
      "name": "Central Park",
      "coordinates": {
        "latitude": 40.7829,
        "longitude": -73.9654
      }
    },
    "timestamp": 1683112800,
    "metrics": ["temperature", "humidity", "pressure"],
    "txid": "0x7g8h9i0j1k2l3m4n5o6p7q8r9s0t1u2v3w4x5y6z"
  }
}
```

## Rate Limits and Quotas

The API enforces rate limits to ensure fair usage:

### Default Rate Limits

- **Anonymous access**: 100 requests per minute
- **API key access**: 1,000 requests per minute
- **Premium API key access**: 10,000 requests per minute

### Endpoint-Specific Limits

Some endpoints have specific rate limits:

- `POST /v1/blockchain/transactions`: 10 requests per minute
- `POST /v1/weather/data`: 60 requests per minute (1 per second)

### Rate Limit Headers

Rate limit information is included in response headers:

```
X-Rate-Limit-Limit: 1000
X-Rate-Limit-Remaining: 995
X-Rate-Limit-Reset: 1683112860
```

### Quota System

Some operations have daily or monthly quotas:

- **Weather data submissions**: 1,440 per day (1 per minute)
- **Computational jobs**: 50 per month for standard tier

## Error Handling

The API uses standard HTTP status codes and returns detailed error information:

### Error Response Format

```json
{
  "error": true,
  "code": "INVALID_PARAMETER",
  "message": "The provided address is not valid",
  "details": {
    "parameter": "address",
    "constraint": "Must be a valid Wrfcoin address starting with WRF1"
  },
  "request_id": "req_7a9b4f2c8d1e0f3a"
}
```

### Common Error Codes

- `INVALID_PARAMETER`: Invalid parameter value
- `MISSING_PARAMETER`: Required parameter missing
- `AUTHENTICATION_FAILED`: Authentication failed
- `AUTHORIZATION_FAILED`: Action not authorized
- `RATE_LIMIT_EXCEEDED`: Rate limit exceeded
- `QUOTA_EXCEEDED`: Usage quota exceeded
- `RESOURCE_NOT_FOUND`: Requested resource not found
- `INTERNAL_ERROR`: Internal server error

## Versioning

The API uses versioning to maintain compatibility:

### Current Versions

- **v1**: Current stable version
- **v2-beta**: Beta version with additional features

### Version Specification

You can specify the API version in three ways:

1. **URL path**: `https://api.wrfcoin.network/v1/status`
2. **Query parameter**: `https://api.wrfcoin.network/status?version=v1`
3. **Header**: `X-API-Version: v1`

### Version Deprecation

Deprecated versions will be announced at least 6 months before being removed. You can check the
deprecated versions in the API status endpoint:

```json
{
  "version": "1.5.2",
  "supported_versions": ["1.4.x", "1.5.x"],
  "deprecated_versions": ["1.3.x", "1.2.x"],
  "deprecated_endpoints": [
    {
      "path": "/v1/old-endpoint",
      "replacement": "/v1/new-endpoint",
      "deprecation_date": "2024-12-31"
    }
  ]
}
```

For additional information or API support, visit the
[developer portal](https://developers.wrfcoin.network) or contact the API team at
api-support@wrfcoin.network.
