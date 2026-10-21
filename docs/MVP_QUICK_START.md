# WRFCoin MVP Quick Start Guide

Get up and running with the WRFCoin MVP in under 5 minutes.

## What is WRFCoin?

WRFCoin is a weather forecasting blockchain platform. The core value loop is simple:

1. **Contribute** environmental data from weather stations or donate compute resources
2. **Validate** through Proof-of-Prediction (PoP) consensus -- the network verifies data quality and forecast accuracy
3. **Earn** WRF tokens as rewards for accurate, consistent contributions

The MVP delivers the essential end-to-end experience: submit weather data, participate in consensus, manage your wallet, vote on governance proposals, and explore the chain.

## Prerequisites

- **Node.js 20+** with npm
- **Rust 1.70+** with cargo
- **Docker** (optional, for containerized setup)
- **PostgreSQL 15+** and **Redis 7+** (or use Docker)

## Quick Start

```bash
# 1. Clone and install
git clone https://github.com/your-org/wrfcoin.git
cd wrfcoin
npm install

# 2. Start the MVP
npm run dev:mvp

# 3. Open the app
open http://localhost:5000
```

That's it. `dev:mvp` starts:
- frontend Vite app on `:5000`
- simple backend API on `:3001`
- MVP feature flags (`VITE_MVP_MODE=true` and `MVP_MODE=true`)

If you need the native-chain launch protocol API for commit/reveal + oracle-quorum testing, run:

```bash
cargo run -p wrfcoin-native-chain --bin launch_api
```

## MVP Architecture

```
+-------------------+       +---------------------+       +------------------+
|                   |       |                     |       |                  |
|  React Frontend   +------>+  Backend API        +------>+  Blockchain Core |
|  (Vite :5173)     |       |  (Express :4001)    |       |  (Rust)          |
|                   |       |                     |       |                  |
+-------------------+       +----------+----------+       +------------------+
                                       |
                                       v
                            +----------+----------+
                            |                     |
                            |  PostgreSQL + Redis  |
                            |  (:5432)    (:6379) |
                            |                     |
                            +---------------------+
```

- **Frontend** -- React/TypeScript SPA served by Vite on port 5000 in MVP mode
- **Backend API** -- lightweight Node server on port 3001 in MVP mode
- **Native-chain launch API** -- Rust runtime on port 8556 when `launch_api` is started

## What's in the MVP

| Feature | Description |
|---------|-------------|
| **Dashboard** | Overview of network stats, your balance, and recent activity |
| **Wallet** | Create wallets, send/receive WRF tokens, view transaction history |
| **Explorer** | Browse blocks, transactions, and addresses on the chain |
| **Mining** | Start/stop mining, view hash rate, and monitor rewards |
| **Weather/PoP** | Submit weather station data, participate in Proof-of-Prediction consensus |
| **Governance** | Create proposals, vote on network changes, view results |
| **NFTs** | Mint and view computational weather NFTs |
| **Settings** | Configure node, network preferences, and display options |
| **Resources** | Documentation links, tutorials, and help guides |

## Running Tests

```bash
# Run the MVP test suite
npm run test:mvp

# Run specific test categories
npm run test:unit          # Unit tests
npm run test:e2e           # End-to-end tests
npm run test:integration   # Integration tests
```

## Docker Alternative

If you prefer a containerized setup:

```bash
docker-compose -f docker-compose.mvp.yml up -d
```

This starts the four core services (frontend, backend, blockchain node, database) with no additional dependencies required on your host machine.

## Where to Contribute

See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines on setting up your development environment, the PR process, code style, and good first issues.
