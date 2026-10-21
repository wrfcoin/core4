# Wrfcoin Project Structure

Last Updated: July 2, 2025

This document outlines the current structure of the Wrfcoin blockchain platform - a comprehensive
climate-focused blockchain ecosystem with advanced governance, cross-chain interoperability, and
environmental data integration.

## Directory Structure

```
wrfcoin/
├── frontend/                    # React Dashboard & User Interface
│   ├── src/                    # React application source code
│   │   ├── components/         # Reusable React components
│   │   ├── pages/             # Application routes and pages
│   │   ├── services/          # API service layer
│   │   └── hooks/             # Custom React hooks
│   ├── static/                # Static assets and public files
│   ├── test-results/          # E2E test results
│   └── vite.config.js         # Vite build configuration
│
├── backend/                     # Node.js Microservices Architecture
│   ├── server/                 # Microservice implementations
│   │   ├── enhanced_unified_api.ts          # Port 3001 - Main API gateway
│   │   ├── cross_chain_protocol_server.js   # Port 3000 - Cross-chain protocols
│   │   ├── cross_chain_websocket_server.js  # Port 5001 - WebSocket services
│   │   ├── iot_sensor_networks_server_fixed.ts # Port 3002 - IoT sensor management
│   │   ├── wallet_server.js                 # Port 4000 - Wallet operations
│   │   └── governance_server_stable.js      # Port 8000 - DAO governance
│   ├── shared/                 # Shared schemas and utilities
│   ├── drizzle/               # Database migrations and configurations
│   ├── config/                # Service configurations
│   ├── middleware/            # Express middleware components
│   ├── routes/                # API route handlers
│   ├── services/              # Business logic services
│   └── utils/                 # Backend utilities
│
├── blockchain-core/             # Core Blockchain Implementation (Rust)
│   ├── src/                   # Main Rust source code
│   ├── consensus/             # Consensus mechanism implementations
│   ├── crypto/                # Cryptographic functions
│   ├── network/               # P2P networking layer
│   ├── mining/                # Mining and validation logic
│   ├── storage/               # Blockchain data storage
│   ├── cross_chain/           # Cross-chain bridge implementations
│   ├── governance/            # On-chain governance protocols
│   ├── layer2/                # Layer 2 scaling solutions
│   ├── iso_compliance/        # ISO standards compliance
│   └── target/                # Rust build artifacts
│
├── cli/                        # Command-Line Interface Tools
│   └── WrfcoinCLI/            # Rust-based CLI application
│
├── contracts/                  # Solidity Smart Contracts
│   ├── CarbonCredits.sol      # Carbon credit management
│   ├── ComputationalNFT.sol   # Computational NFT contracts
│   ├── WRFToken.sol           # Native token implementation
│   ├── WeatherDataValidator.sol # Weather data validation
│   └── integrations/          # Third-party integrations
│
├── infrastructure/             # DevOps & Deployment
│   ├── docker/                # Docker containerization
│   ├── kubernetes/            # Kubernetes orchestration
│   ├── terraform/             # Infrastructure as Code
│   ├── monitoring/            # Prometheus/Grafana configs
│   ├── unraid/                # Unraid server deployment
│   └── disaster-recovery/     # Backup and recovery procedures
│
├── consolidated-tests/          # Unified Testing Framework
│   ├── unit/                  # Unit tests
│   ├── integration/           # Integration tests
│   ├── e2e/                   # End-to-end tests
│   ├── security/              # Security testing
│   ├── performance/           # Load and performance tests
│   └── smart-contracts/       # Contract testing
│
├── docs/                       # Comprehensive Documentation
│   ├── api/                   # API documentation
│   ├── architecture/          # System architecture guides
│   ├── guides/                # User and developer guides
│   ├── reports/               # Analysis and performance reports
│   ├── security/              # Security documentation
│   └── sdk/                   # SDK documentation
│
├── fuzz-testing/               # Security Fuzzing Framework
├── langchain-integration/      # AI/ML Integration Layer
├── archive/                    # Archived legacy code
├── assets/                     # Project media and resources
├── attached_assets/            # User-uploaded attachments
├── artifacts/                  # Hardhat compilation artifacts
├── cache/                      # Build and runtime cache
├── certs/                      # SSL/TLS certificates
├── config/                     # Global configuration files
├── logs/                       # Application and system logs
│
└── Root Configuration Files
    ├── package.json           # Node.js dependencies and scripts
    ├── jest.config.js         # Jest testing configuration
    ├── playwright.config.js   # E2E testing configuration
    ├── hardhat.config.js      # Smart contract development
    ├── docker-compose.yml     # Local development environment
    └── replit.md              # Project context and preferences
```

## Component Overview

### Frontend Application (`frontend/`)

- **Technology**: React 18 with Vite, Material-UI, TypeScript
- **Port**: 5000 (Development server)
- **Purpose**: Comprehensive web dashboard for blockchain platform interaction
- **Key Features**:
  - Real-time blockchain monitoring and analytics
  - DAO governance interface with proposal management
  - Cross-chain asset management and bridging
  - Weather station integration and data visualization
  - NFT marketplace and computational NFT creation
  - Mining dashboard with performance metrics
  - Wallet operations and multi-signature support

### Backend Microservices (`backend/server/`)

#### Enhanced Unified API Server (`enhanced_unified_api.ts`)

- **Port**: 3001
- **Purpose**: Main API gateway and request routing
- **Features**: Authentication, rate limiting, response aggregation, 47+ endpoints

#### Cross-Chain Protocol Server (`cross_chain_protocol_server.js`)

- **Port**: 3000
- **Purpose**: Multi-blockchain interoperability
- **Features**: Bridge protocols, asset transfers, cross-chain transaction monitoring

#### Cross-Chain WebSocket Server (`cross_chain_websocket_server.js`)

- **Port**: 5001
- **Purpose**: Real-time cross-chain communication
- **Features**: Live transaction updates, bridge status monitoring, event streaming

#### IoT Sensor Networks Server (`iot_sensor_networks_server_fixed.ts`)

- **Port**: 3002
- **Purpose**: Weather station and IoT device management
- **Features**: 740,000 device management across 10 networks, data validation

#### Wallet Server (`wallet_server.js`)

- **Port**: 4000
- **Purpose**: Secure wallet operations and key management
- **Features**: BIP-39/44 support, multi-signature wallets, transaction signing

#### Governance Server (`governance_server_stable.js`)

- **Port**: 8000
- **Purpose**: Decentralized governance and DAO operations
- **Features**: Proposal creation, voting mechanisms, delegation, treasury management

### Blockchain Core (`blockchain-core/`)

- **Technology**: Rust with advanced cryptographic libraries
- **Purpose**: Core blockchain infrastructure implementation
- **Key Components**:
  - **Consensus**: Multiple algorithms (PoW, PoS, PoP, DPoS)
  - **Network**: P2P communication with peer discovery
  - **Mining**: Advanced mining algorithms with difficulty adjustment
  - **Cross-Chain**: 13+ bridge protocols for multi-chain support
  - **Layer 2**: ZK-rollups, optimistic rollups, state channels
  - **Governance**: On-chain voting and proposal execution
  - **ISO Compliance**: Enterprise-grade standards implementation

### Smart Contracts (`contracts/`)

- **Technology**: Solidity with OpenZeppelin libraries
- **Purpose**: On-chain logic and asset management
- **Key Contracts**:
  - **WRFToken.sol**: Native token with advanced tokenomics
  - **CarbonCredits.sol**: Environmental credit tracking
  - **ComputationalNFT.sol**: Dynamic NFTs with computational proofs
  - **WeatherDataValidator.sol**: Physics-based weather validation

### Infrastructure (`infrastructure/`)

- **Technology**: Docker, Kubernetes, Terraform, Prometheus/Grafana
- **Purpose**: Production deployment and monitoring
- **Features**:
  - **Container Orchestration**: Multi-service Docker Compose setup
  - **Kubernetes**: Scalable production deployment
  - **Monitoring**: Comprehensive observability stack
  - **Security**: Enterprise-grade security hardening
  - **Disaster Recovery**: Automated backup and recovery procedures

### Testing Framework (`consolidated-tests/`)

- **Technology**: Jest, Playwright, Artillery, Cypress
- **Purpose**: Comprehensive quality assurance
- **Coverage**:
  - **Unit Tests**: Component-level testing with 80%+ coverage
  - **Integration Tests**: Service interaction validation
  - **E2E Tests**: Full user journey automation
  - **Security Tests**: Vulnerability and penetration testing
  - **Performance Tests**: Load testing up to 20,000+ TPS

## Development Workflow

### Local Development Environment

All services run concurrently through Replit workflows with the following port allocation:

| Service                      | Port | Purpose           | Technology           |
| ---------------------------- | ---- | ----------------- | -------------------- |
| Frontend Application         | 5000 | Web dashboard     | React + Vite         |
| Cross-Chain Protocol Server  | 3000 | Bridge protocols  | Node.js + Express    |
| Enhanced Unified API Server  | 3001 | Main API gateway  | TypeScript + Express |
| IoT Sensor Networks Server   | 3002 | Device management | TypeScript + Express |
| Wallet Server                | 4000 | Wallet operations | Node.js + Express    |
| Cross-Chain WebSocket Server | 5001 | Real-time updates | WebSocket + Node.js  |
| Governance Server            | 8000 | DAO operations    | Node.js + Express    |

### Development Tasks

1. **Frontend Development**

   - Work in `frontend/src/` directory
   - Hot reload available on port 5000
   - Use Material-UI components and React hooks
   - API calls route through service layer in `frontend/src/services/`

2. **Backend API Development**

   - Modify microservices in `backend/server/`
   - Each service has dedicated port and purpose
   - Shared utilities in `backend/shared/`
   - Database schemas managed through Drizzle ORM

3. **Blockchain Core Development**

   - Rust development in `blockchain-core/src/`
   - Consensus mechanisms in `blockchain-core/consensus/`
   - Cross-chain bridges in `blockchain-core/cross_chain/`
   - Build with `cargo build --release`

4. **Smart Contract Development**

   - Solidity contracts in `contracts/` directory
   - Deploy using Hardhat: `npx hardhat deploy`
   - Test with `npx hardhat test`

5. **Database Management**

   - Schema definitions in `backend/shared/schema*.ts`
   - Migrations via Drizzle: `npm run db:push`
   - Never manually write SQL migrations

6. **Testing & Quality Assurance**

   - Unit tests: `npm run test:unit`
   - Integration tests: `npm run test:integration`
   - E2E tests: `npm run test:e2e`
   - Security tests: `npm run test:security`
   - Load testing with Artillery.io

7. **Deployment**
   - Local development: `docker-compose up`
   - Production: Use Kubernetes manifests in `infrastructure/kubernetes/`
   - Infrastructure as Code: Terraform configs in `infrastructure/terraform/`

## Key Configuration Files

### Core Configuration

- `package.json` - Node.js dependencies and scripts for entire platform
- `replit.md` - Project context, user preferences, and architectural decisions
- `docker-compose.yml` - Multi-service local development environment
- `jest.config.js` - Unified testing configuration
- `playwright.config.js` - End-to-end testing setup
- `hardhat.config.js` - Smart contract development and deployment

### Frontend Configuration

- `frontend/vite.config.js` - Vite build configuration with React + TypeScript
- `frontend/package.json` - Frontend-specific dependencies

### Backend Configuration

- `backend/shared/schema*.ts` - Database schemas using Drizzle ORM
- `backend/config/` - Service-specific configuration files
- `backend/drizzle/` - Database migration configurations

### Blockchain Configuration

- `blockchain-core/Cargo.toml` - Rust blockchain core dependencies and features
- `blockchain-core/src/` - Core blockchain source code and modules

### Infrastructure Configuration

- `infrastructure/docker/` - Docker containerization configs
- `infrastructure/kubernetes/` - Production Kubernetes manifests
- `infrastructure/terraform/` - Infrastructure as Code definitions
- `infrastructure/monitoring/` - Prometheus/Grafana monitoring setup

### Security & Compliance

- `certs/` - SSL/TLS certificates for secure communications
- `blockchain-core/iso_compliance/` - Enterprise compliance implementations

## Current Platform Status

### Microservices Health

✅ **All 7 microservices running successfully**

- Cross-Chain Protocol Server (Port 3000)
- Enhanced Unified API Server (Port 3001)
- IoT Sensor Networks Server (Port 3002)
- Wallet Server (Port 4000)
- Cross-Chain WebSocket Server (Port 5001)
- Frontend Application (Port 5000)
- Governance Server (Port 8000)

### Technology Stack

- **Frontend**: React 18 + TypeScript + Material-UI + Vite
- **Backend**: Node.js + Express + TypeScript (99% migrated)
- **Blockchain**: Rust with advanced cryptographic libraries
- **Database**: PostgreSQL with Drizzle ORM
- **Infrastructure**: Docker + Kubernetes + Terraform
- **Testing**: Jest + Playwright + Artillery + Cypress

### Architecture Highlights

- **Climate Focus**: Weather data integration with 740,000+ IoT devices
- **Cross-Chain**: 13+ bridge protocols for multi-blockchain support
- **Governance**: Complete DAO implementation with liquid democracy
- **Security**: Enterprise-grade A+ (99/100) security rating
- **Scalability**: 20,000+ TPS capability with Layer 2 solutions
- **Compliance**: ISO standards implementation for enterprise adoption

This structure represents a comprehensive blockchain platform optimized for climate applications,
governance, and enterprise deployment.

## File Organization (Updated)

### Collectors (`scripts/collectors/`)

Auto-collector scripts for different data sources:

- `acurite-auto-collector.js` - Weather station auto-collection
- `acurite-enhanced-collector.js` - Enhanced weather collection with IPFS
- `acurite-working-collector.js` - Working weather station collector
- `compute-auto-collector.js` - Computational power contribution
- `nas-auto-collector.js` - NAS storage auto-discovery and management

### Testing Scripts (`testing/scripts/`)

Organized testing and validation scripts:

- `test-*.js` - Various system integration tests
- `demos/` - Demonstration scripts for features
- `validate-*.js` - Security and system validation

### Reports (`docs/reports/`)

Analysis reports and documentation:

- Database structure and migration reports
- Performance analysis reports
- Security analysis reports
- System optimization reports

### Temporary Files (`scripts/temp/`)

Temporary development files and work-in-progress scripts.
