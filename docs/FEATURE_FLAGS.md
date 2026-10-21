# Feature Flags

WRFCoin uses feature flags across all four layers of the stack to separate the MVP from deferred functionality. This keeps the MVP lean while ensuring advanced features can be enabled incrementally.

## Overview

| Layer | Flag Mechanism | MVP Default |
|-------|---------------|-------------|
| Frontend | `VITE_MVP_MODE` env var | `true` |
| Backend | `MVP_MODE` env var | `true` |
| Rust Core | Cargo features | `default = ["mvp"]` |
| Docker | Compose profiles | `docker-compose.mvp.yml` |

## Frontend

The environment variable `VITE_MVP_MODE=true` controls which features are exposed in the UI.

**Configuration source:** `frontend/src/config/features.ts`

This module reads `import.meta.env.VITE_MVP_MODE` and exports a feature map that the rest of the app consumes.

**Route gating:** `App.jsx` conditionally registers routes based on the feature map. Routes for deferred features (cross-chain bridges, advanced DeFi, IoT management) are not mounted when MVP mode is active.

**Navigation gating:** `StartMenu.jsx` reads the same feature map to show or hide navigation items. Users only see menu entries for features that are enabled.

```bash
# .env (frontend)
VITE_MVP_MODE=true
```

## Backend

The environment variable `MVP_MODE=true` controls which API routes and services are registered.

**Configuration source:** `backend/server/main_api_server.ts`

When `MVP_MODE` is set to `true`, the server skips registration of non-MVP route modules. This means endpoints for cross-chain bridges, IoT sensor networks, AI/ML inference, and advanced DeFi protocols are not loaded, reducing startup time and attack surface.

```bash
# .env (backend)
MVP_MODE=true
```

## Rust Blockchain Core

Cargo features in `blockchain-core/Cargo.toml` control which modules are compiled.

```toml
[features]
default = ["mvp"]
mvp = []
full = ["mvp", "cross-chain", "quantum-crypto", "ai-ml", "iot-integration", "hsm"]
cross-chain = []
quantum-crypto = []
ai-ml = []
iot-integration = []
hsm = []
```

The `mvp` feature is the default. It includes core consensus (PoP), wallet operations, block production, transaction processing, and basic mining. The `full` feature enables everything.

```bash
# Build MVP (default)
cargo build --release

# Build with all features
cargo build --release --features full

# Build with specific features
cargo build --release --features "mvp,cross-chain"
```

## Docker

**MVP compose file:** `docker-compose.mvp.yml`

This file defines the four core services needed for the MVP:

1. Frontend (Vite dev server or nginx for production)
2. Backend API (Node.js/Express)
3. Blockchain Core (Rust binary)
4. Database (PostgreSQL + Redis)

```bash
# Start MVP services only
docker-compose -f docker-compose.mvp.yml up -d
```

**Full compose file:** `infra/docker/docker-compose.yml`

Non-MVP services in the full compose file use Docker Compose profiles. Services like cross-chain bridges, IoT ingestion, AI/ML workers, and monitoring are tagged with `profiles: ["full"]` so they do not start by default.

```bash
# Start all services including deferred features
docker compose -f infra/docker/docker-compose.yml --profile full up -d
```

## Enabling Deferred Features

To switch from MVP to the full platform:

1. **Frontend:** Set `VITE_MVP_MODE=false` in your `.env` file and restart the dev server
2. **Backend:** Remove `MVP_MODE` from your environment (or set it to any value other than `true`) and restart the server
3. **Rust Core:** Build with `cargo build --release --features full`
4. **Docker:** Use `docker compose -f infra/docker/docker-compose.yml --profile full up -d`

You can also enable features selectively. For example, enable cross-chain support in Rust without enabling quantum crypto by specifying `--features "mvp,cross-chain"`.

## Phase Definitions

### Phase 1: MVP (Current)

The foundation. Everything needed for the core value loop.

- Dashboard with network overview
- Wallet creation and token transfers
- Block explorer
- Mining with PoP consensus
- Weather data submission and validation
- Basic governance (proposals and voting)
- Computational weather NFTs
- Settings and resources pages

### Phase 2: Carbon, AI/ML, Advanced DeFi

Expanding the platform's capabilities.

- Carbon credit tracking and trading
- AI/ML weather forecasting models
- Advanced DeFi: AMM liquidity pools, yield farming, staking
- Enhanced analytics and reporting
- Multi-model ensemble predictions

### Phase 3: Cross-Chain, IoT, Quantum Crypto, HSM

Enterprise-grade features and interoperability.

- Cross-chain bridges (13+ chains via Axelar, LayerZero, Wormhole, IBC, XCMP)
- IoT sensor network integrations (20+ platforms)
- Quantum-resistant cryptography (CRYSTALS-KYBER, DILITHIUM, FALCON, SPHINCS+)
- Hardware Security Module (HSM) integration
- Full compliance frameworks (SOC2, ISO 27001, GDPR, PCI-DSS)
