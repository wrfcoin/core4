# WRFCoin Platform Overview

**Last updated:** 2026-04-27 (testnet 2 launch day)
**Status:** Living document — single-page entry point for the WRFCoin platform.

---

## 1. What is WRFCoin?

WRFCoin is a weather-forecasting blockchain that pays contributors for environmental data
collection and climate computation, anchors carbon-credit provenance on-chain, and ships a full
DeFi suite (AMM, staking, governance) on top of the same ledger. By combining sensor incentives,
verifiable forecasting, and tokenized carbon, WRFCoin becomes a decentralized substrate for
climate intelligence — owned and operated by the people who feed it data.

The platform is a Rust-based Layer 1 (`core4`) with a Layer 2 scaling tier, surrounded by 25+
TypeScript microservices, 348+ Solidity contracts, a React frontend, mobile clients, and an
operations toolchain. See [`README.md`](../README.md) for the workspace entry point and
[`docs/README.md`](README.md) for the deeper feature index.

---

## 2. The problem WRFCoin solves

Weather and climate data has three structural problems that WRFCoin addresses simultaneously:

- **Centralization.** Authoritative weather data is locked inside national agencies (NOAA, ECMWF,
  Met Office, JMA) and a small number of commercial providers. Coverage is uneven, refresh
  cadences are political, and access pricing favors institutions over individuals. Anyone with a
  barometer, a GPS, an RTL-SDR, or a weather station can produce useful data — but today there
  is no global market for it.
- **Opaque models.** Modern numerical weather prediction (NWP) runs are reproducible in theory
  but rarely in practice — the input datasets, parameter choices, and post-processing steps are
  hidden behind agency walls. There is no audit trail showing _which inputs produced which
  forecast_, so consumers cannot verify accuracy or assign blame for errors.
- **Carbon-credit fraud.** Carbon-credit markets have repeatedly been embarrassed by
  double-counting, ghost projects, and unverifiable retirement claims. Without a cryptographic
  ledger that ties a credit to its measurement, its issuance, its trade history, and its
  retirement, the market cannot scale beyond voluntary buyers.

WRFCoin makes contribution _incentivizable_ (contributors are paid in WRF), models _verifiable_
(every forecast logs its inputs and computational proof on-chain), and credits _cryptographically
tracked_ (every credit is a token with a full provenance trail from sensor reading to retirement).

---

## 3. Architecture

### 3.1 Four-tier model

WRFCoin layers concerns from raw sensor input up to user-facing applications. See
[`docs/architecture-overview.md`](architecture-overview.md) lines 13–34 for the canonical diagram.

| Tier | Responsibility | Components |
|------|----------------|------------|
| **Application** | Climate dApps, weather APIs, carbon markets, parametric insurance | Frontend, mobile, dApp ecosystem |
| **Node OS** | Container orchestration, AI/ML inference, ETL pipelines | Backend microservices, compute agent |
| **Layer 2** | Sequencer, fraud proofs, state manager, bridge protocol | `core4/layer2/`, smart contracts |
| **Layer 1** | Consensus, data-aware blocks, computational blocks, climate oracles | `core4/native-chain/`, `core4/shared-protocol/` |

### 3.2 Proof of Climate Computation (PoCC)

The signature consensus mechanism. Validators perform real environmental computations (NWP runs,
risk assessments, air-quality forecasts) and earn rewards proportional to **data quality** and
**computational accuracy**, not raw hash rate. PoCC produces two specialized block shapes:

- **Data-aware blocks** carry classified climate observations with quality scores and adaptive
  routing decisions.
- **Computational blocks** carry input data, computation results, and verification proofs for a
  weather model run.

See [`docs/architecture-overview.md`](architecture-overview.md) lines 36–86 for the full struct
definitions and supported computation types (WRF, ICON, GFS, carbon, sea-level, air-quality).

### 3.3 Consensus engines

`core4/consensus-engines/` ships pluggable engines selectable per network. PoCC is the headline,
but the engine factory also supports PoW, PoS, PoI (Proof of Importance), PoP (Proof of
Participation), PoU (Proof of Uniqueness) and DPoS for hybrid deployments. See
[`core4/CLAUDE.md`](../CLAUDE.md) for crate-level details.

### 3.4 Microservices and port map

The Node.js backend is organized as 25+ services using the Strangler Fig pattern. Port map (see
[`backend/README.md`](https://github.com/wrfcoin/backend) lines 20–31 and
[`docs/README.md`](README.md) lines 123–134):

| Port | Service | Purpose |
|------|---------|---------|
| 4000 | API Gateway | Primary entry point and request router |
| 4001 | Main API | Core blockchain, governance, mining logic |
| 3000 | Cross-Chain Bridge | Axelar / LayerZero / Wormhole protocols |
| 3002 | IoT Data Collector | Meshtastic and environmental sensor ingestion |
| 6000 | Compute Service | Decentralized compute contribution |
| 6001 | AI/ML Service | Forecasting and AI verification |
| 7000 | Storage Service | IPFS and decentralized storage adapters |
| 8000 | Governance | DAO voting and proposal management |
| 8080 | Weather Rewards | Reward calculation and distribution |
| 9001 | Node Discovery | P2P coordination |

PostgreSQL (5432) and Redis (6379) back the services with a consolidated 26-table schema.

### 3.5 Cross-chain bridges

WRFCoin bridges to 13+ networks (EVM: Ethereum, Polygon, BSC, Avalanche, Arbitrum, Optimism, Base,
Fantom; non-EVM: Cosmos, Polkadot, Solana, Near, Harmony) over 7+ protocols (Axelar, LayerZero,
Wormhole, Stargate, Synapse, IBC, XCMP). See [`core4/CLAUDE.md`](../CLAUDE.md) for the canonical
list.

### 3.6 Repository layout

The platform is split across nine sibling repositories — see the workspace
[`CLAUDE.md`](../CONTRIBUTING.md) lines 22–37 for the canonical table. Headlines:

- **`core4/`** — Rust workspace (this repo): blockchain runtime, consensus, P2P, privacy
- **`frontend/`** — React 19 + TypeScript dashboard (450+ components)
- **`backend/`** — 25+ Node.js microservices
- **`smart-contracts/`** — 348+ Solidity contracts (DeFi, governance, carbon, NFT, bridge)
- **`infra/`** — Docker, Kubernetes, Terraform, monitoring, deployment scripts
- **`security-testing/`** — Rust attack-simulation and stress-testing suite
- **`mobile/`** — Flutter app (barometer, GPS, wallet)
- **`defi-protocols/`** — DeFi primitives (issue #370, in development)
- **`storage-providers/`** — IPFS / Filecoin / Arweave adapters (issue #371, in development)
- **`blockchain-core/`** — archived original Rust blockchain (frozen, reference only)

---

## 4. Security model

WRFCoin is engineered for institutional deployment from day one.

| Surface | Control |
|---------|---------|
| **Validator signing** | HSM-backed. SoftHSM2 by default; YubiKey via PKCS#11; AWS CloudHSM, Azure Key Vault, Google Cloud HSM, HashiCorp Vault, Thales Luna, and Utimaco supported. See [`security/HSM-SECURITY-GUIDE.md`](../security/HSM-SECURITY-GUIDE.md). |
| **WebSocket access** | Identity-bound ACLs — the token store carries identity and not just role, so a compromised role token cannot impersonate another validator. |
| **Slashing** | On-chain slashing for double-sign and for sustained low-quality contribution streaks (issues #1393, #1425). |
| **Fast-sync trust** | Trusted snapshot signer allow-list for fast-sync clients (issues #1390, #1413) — clients only accept snapshots signed by a known producer. |
| **Cryptography** | ed25519 + Blake3 in the canonical runtime. ZK proofs use ark-groth16 over BN254. Post-quantum stack (KYBER, DILITHIUM, FALCON, SPHINCS+, XMSS) is archived in `blockchain-core/` for future re-integration. |
| **Compliance** | SOC2 Type II, ISO 27001, NIST CSF, GDPR, CCPA, PCI-DSS, ISO 20022, ISO 14064. |
| **L2 arithmetic** | Overflow-safe checked math throughout; supply-chain dep hygiene via SBOM, sigstore, dependabot, and supply-chain-security workflow. |
| **Network** | TLS 1.3, DDoS mitigation, rate limiting, helmet middleware, JWT + MFA. |

For the comprehensive security model and HSM provider matrix, read
[`security/HSM-SECURITY-GUIDE.md`](../security/HSM-SECURITY-GUIDE.md) and the security guide in
[`docs/SECURITY.md`](SECURITY.md).

---

## 5. How users interact

WRFCoin has five distinct user roles, all served by the same protocol.

- **Weather data contributors** — install the mobile app or run a contributor node. The app
  collects barometer pressure and GPS readings in the background; a node can additionally feed in
  RTL-SDR captures of consumer weather-station broadcasts. Contributions follow a commit/reveal
  protocol so submissions cannot be front-run, and rewards land in the user's wallet on every
  block. See [`infra/testnet/GETTING_STARTED.md`](https://github.com/wrfcoin/infra/blob/main/testnet/GETTING_STARTED.md).
- **Validators** — run a `wrfcoin-native-chain` node, stake WRF, and sign blocks with an
  HSM-backed key. PoCC pays for both presence (block production) and quality (forecast accuracy
  scored against ground truth).
- **DeFi users** — open the frontend, connect a wallet (Ethers v6), and access the AMM, yield
  farms, and carbon-credit marketplace. Cross-chain bridges expose WRF to 13+ external networks.
  See [`frontend/README.md`](https://github.com/wrfcoin/frontend) lines 24–33.
- **Governance participants** — DAO proposals and quadratic voting are fully on-chain. The
  governance service (port 8000) handles proposal lifecycle; the frontend handles voting UX.
- **Enterprises and developers** — climate APIs, GraphQL endpoints, multi-language SDKs (TS,
  Python, Rust, Java, Go, C#, Android, iOS), and the WRFCoin CLI. See
  [`backend/README.md`](https://github.com/wrfcoin/backend) lines 93–101 for the weather pipeline endpoints.

---

## 6. Why WRFCoin is useful

The combination — not any one piece — is what makes WRFCoin valuable.

A direct **financial incentive for high-quality environmental data** produces datasets that
agency-funded networks cannot match. NOAA's Cooperative Observer Program is ~8,700 stations
worldwide; a network that pays per quality-scored observation can plausibly grow an order of
magnitude larger and refresh an order of magnitude faster, especially in under-instrumented
regions of the global south. **On-chain provenance for carbon credits** ends the double-counting
fraud that has dogged voluntary carbon markets: every credit is minted from a measured event,
traded with full history, and burned on retirement, all visible to any verifier.

**Decentralized weather forecasts** are politically tamper-resistant in a way agency forecasts
are not — when the inputs and computation are on-chain, no government can quietly suppress an
inconvenient hurricane track or tweak a drought outlook. And the **AMM and tokenomics** turn
sensor data and computation into liquid assets, so contributors can monetize their work
immediately without going through institutional intermediaries. Together, these primitives make
WRFCoin the substrate for a decentralized $100T climate economy — see
[`docs/wvm_18_month_roadmap.md`](wvm_18_month_roadmap.md) for the strategic framing.

---

## 7. Roadmap

**Now (April 2026)** — testnet 2 launches today from clean genesis on the node host reference node.
Testnet 1 has been archived to `wrfcoin/blockchain-core/testnet-1-archive/` for historical
record. All code-level launch blockers are resolved; only human gates remain (key ceremony,
final audit pass, hardware provisioning).

**Next** — mainnet candidate, gated on testnet 2 stability, third-party security audit, and the
post-quantum re-integration from the `blockchain-core/` archive.

**Hardware integration** — see [`docs/hardware-integration-roadmap.md`](hardware-integration-roadmap.md).
Tier 1 (RTL-SDR USB passthrough, testnet-ready today) is live; Tier 2 (LoRaWAN gateway + Zigbee
mesh) and Tier 3 (LTE failover, WiFi mesh, AP mode) are post-launch.

**18-month WVM (WRFCoin Virtual Machine)** — see [`docs/wvm_18_month_roadmap.md`](wvm_18_month_roadmap.md).
Phase 1 (foundation, testnet) is complete; phase 2 (enterprise partnerships, L2 scaling to
100k+ TPS, formal verification) and phase 3 (institutional adoption, $100B-cap target) are the
forward agenda.

**New repos in flight** — `defi-protocols/` (account abstraction, liquid staking, lending —
issue #370) and `storage-providers/` (IPFS, Filecoin, Arweave — issue #371) are scaffolded and
awaiting first contributions.

---

## 8. Repository map

See workspace [`CLAUDE.md`](../CONTRIBUTING.md) lines 22–37 for the canonical table; this is the
short version.

| Repo | Role |
|------|------|
| `wrfcoin/core4` | Rust blockchain (this repo) — runtime, consensus, P2P, privacy |
| `wrfcoin/frontend` | React/TypeScript dashboard |
| `wrfcoin/backend` | 25+ Node.js microservices |
| `wrfcoin/smart-contracts` | 348+ Solidity contracts |
| `wrfcoin/infra` | Docker / k8s / Terraform / monitoring |
| `wrfcoin/security-testing` | Attack simulation and stress tests |
| `wrfcoin/mobile` | Flutter mobile app |
| `wrfcoin/defi-protocols` | DeFi primitives (in development) |
| `wrfcoin/storage-providers` | Decentralized storage adapters (in development) |
| `wrfcoin/blockchain-core` | Archived original Rust blockchain (reference only) |

---

## Further reading

- [`README.md`](../README.md) — `core4` workspace entry
- [`docs/README.md`](README.md) — feature-level documentation index
- [`docs/architecture-overview.md`](architecture-overview.md) — full architecture deep-dive
- [`security/HSM-SECURITY-GUIDE.md`](../security/HSM-SECURITY-GUIDE.md) — HSM model
- [`docs/wvm_18_month_roadmap.md`](wvm_18_month_roadmap.md) — strategic roadmap
- [`docs/hardware-integration-roadmap.md`](hardware-integration-roadmap.md) — contributor hardware
- [Workspace `CLAUDE.md`](../CONTRIBUTING.md) — multi-repo development guide
