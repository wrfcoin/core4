# WRFCoin - Weather Forecasting Blockchain Platform

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Node.js](https://img.shields.io/badge/Node.js-20%2B-green.svg)](https://nodejs.org/)
[![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5%2B-blue.svg)](https://www.typescriptlang.org/)

> **Incentivizing global environmental data contribution through blockchain technology**

WRFCoin is a comprehensive weather forecasting blockchain platform that rewards users for contributing environmental data and forecasts. By combining climate data collection, carbon credit trading, and DeFi features, WRFCoin creates a decentralized ecosystem for weather intelligence.

---

## Key Features

### Weather & IoT Integration
- **20+ Sensor Platforms**: Integration with The Things Network, Helium, IoTeX, PurpleAir, Ambient Weather, and more
- **Quality Scoring**: Physics-based validation with 0-100 quality scores
- **Multi-Source Consensus**: Aggregation and validation across multiple data sources
- **Achievement System**: Tiered rewards for Data Collector, Consistency Champion, and Quality Pioneer

### Cross-Chain Interoperability
- **13+ Blockchain Networks**: Ethereum, Polygon, BSC, Avalanche, Arbitrum, Cosmos, Polkadot, Solana, and more
- **7+ Bridge Protocols**: Axelar, LayerZero, Wormhole, Stargate, IBC, XCMP
- **Atomic Swaps**: Trustless cross-chain token exchanges

### DeFi & Carbon Credits
- **AMM Pools**: Automated market making for WRF token
- **Yield Farming**: Staking rewards for liquidity providers
- **Quadratic Voting Governance**: Fair DAO proposal and voting system
- **Computational NFTs**: Weather data NFTs with on-chain computation
- **Carbon Credit Trading**: Tokenized carbon credits with verification

### Enterprise Security
- **Quantum-Resistant Cryptography**: CRYSTALS-KYBER, DILITHIUM, FALCON, SPHINCS+, XMSS
- **HSM Integration**: AWS CloudHSM, Azure Key Vault, HashiCorp Vault, YubiKey
- **Compliance**: SOC2 Type II, ISO 27001, PCI-DSS, GDPR, ISO 14064

---

## Quick Start

### Prerequisites

- **Node.js 20+** with npm
- **Rust 1.70+** with Cargo
- **PostgreSQL 15+**
- **Docker & Docker Compose**
- **Redis 7+**

### Installation

```bash
# Clone the repository
git clone https://github.com/wrfcoin/core4.git
cd wrfcoin

# Install dependencies
npm install

# Set up environment variables
cp .env.example .env
# Edit .env with your configuration

# Start infrastructure services
docker-compose up -d

# Run database migrations
npm run db:migrate

# Start all services
npm run start:all
```

### Verify Installation

```bash
# Check service health
curl http://localhost:4001/health

# View microservice status
node scripts/service-control-panel.js
```

---

## Technology Stack

| Layer | Technologies |
|-------|--------------|
| **Blockchain Core** | Rust, CRYSTALS-KYBER, DILITHIUM |
| **Backend Services** | Node.js, TypeScript, Express, GraphQL |
| **Frontend** | React 18+, TypeScript, Vite |
| **Smart Contracts** | Solidity, Hardhat (348 contracts) |
| **Database** | PostgreSQL 15, Drizzle ORM (21 tables) |
| **Cache & Messaging** | Redis 7, WebSockets |
| **Infrastructure** | Docker, Kubernetes, Prometheus, Grafana |
| **Desktop App** | Tauri |

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        WRFCoin Platform                          │
├─────────────────────────────────────────────────────────────────┤
│  Frontend (425+ components)     │  Desktop App (Tauri)          │
├─────────────────────────────────────────────────────────────────┤
│                    15+ Microservices                             │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐   │
│  │Main API │ │Cross-   │ │ Wallet  │ │  IoT    │ │   AI/ML │   │
│  │ :4001   │ │Chain    │ │  :4000  │ │ :3002   │ │  :6001  │   │
│  │         │ │  :3000  │ │         │ │         │ │         │   │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘   │
├─────────────────────────────────────────────────────────────────┤
│  Blockchain Core (Rust)         │  Smart Contracts (348)        │
├─────────────────────────────────────────────────────────────────┤
│  PostgreSQL  │  Redis  │  IPFS  │  Filecoin  │  Arweave        │
└─────────────────────────────────────────────────────────────────┘
```

### Core Services

| Port | Service | Description |
|------|---------|-------------|
| 4001 | Main API | Primary application endpoints |
| 3000 | Cross-Chain | Multi-blockchain bridges |
| 4000 | Wallet | BIP-39/44 wallet operations |
| 6001 | AI/ML | Model deployment and inference |
| 3002 | IoT Networks | Sensor data management |
| 8000 | Governance | DAO proposals and voting |
| 8080 | Weather Earnings | Reward distribution |

---

## Documentation

### Core Guides


### Technical Documentation

| Category | Documentation |
|----------|---------------|
| **Architecture** | [System Architecture](./architecture/), [Database Schema](./architecture/database-tables.md) |
| **Security** | [Security Audit](./security/SECURITY_AUDIT_REPORT.md), [XMSS Implementation](./security/XMSS_IMPLEMENTATION_PLAN.md) |
| **Development** | [TypeScript Roadmap](./development/TYPESCRIPT_ROADMAP_2025.md) |
| **API Reference** | [Complete API Docs](./api-reference/api_reference_complete.md), [Smart Contracts API](./api/smart-contracts-api.md) |
| **User Guides** | [Terminal Guide](./user-guides/WRFCOIN_TERMINAL_GUIDE.md), [Quick Reference](./user-guides/QUICK_REFERENCE.md) |

### Additional Resources

- [Business Plan](./business_plan/) - Strategy and economics
- [Tutorials](./tutorials/) - Step-by-step guides
- [Examples](./examples/) - Code samples
- [SDK Reference](./sdk_reference_complete.md) - Developer kits

---

## Development

### Running Tests

```bash
# Run all tests
npm run test:all

# Unit tests
npm run test:unit

# Integration tests
npm run test:integration

# E2E tests
npm run test:e2e

# Blockchain core tests
cd blockchain-core && cargo test
```

### Code Quality

```bash
# TypeScript type checking
npm run type-check

# Linting
npm run lint:all

# Security audit
npm audit
```

---

## Contributing

We welcome contributions! Please see our [Contribution Guide](./testing/CONTRIBUTION_GUIDE.md) for guidelines.

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

---

## License

This project is licensed under the MIT License - see the [LICENSE](../LICENSE) file for details.

---

## Links

- **Repository**: [github.com/wrfcoin/core4](https://github.com/wrfcoin/core4)
- **Documentation**: [docs.wrfcoin.org](https://docs.wrfcoin.org)
- **Community**: [forum.wrfcoin.org](https://forum.wrfcoin.org)

---

_Last updated: January 2026_
