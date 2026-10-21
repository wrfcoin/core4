# Wrfcoin CLI - Sovereign Node Management

The Wrfcoin CLI provides complete control over your sovereign cryptocurrency node. Built for the
unstoppable, permissionless future.

## Node payment chain selection

`node bond deposit` and `node receipt submit` require `--chain-id` (for example,
`--chain-id 888 --network-id testnet`); they have no default chain. The supplied
chain, network and genesis must match the target node's `/network/info` before
submission. Offline receipt dry runs use the explicitly supplied domain.

`snapshot fetch` obtains its signing domain from the source's `/network/info`.
It verifies the snapshot in that domain and requires the receiver to report the
same chain and network before ingest. A known receiver genesis must also match.
A fresh receiver reporting genesis `unknown` may ingest; the node verifies the
carried block 1 against its independent genesis pin before installation.
`snapshot submit-receipt` verifies
the retained packet's owner authorization against its target's identity before
POST. The source and receipt-submission target require a known genesis; missing
or malformed identity data is refused.

## Quick Start

### One-Command Installation

```bash
npx wrfcoin-core install
```

This single command sets up a complete, sovereign Wrfcoin node with:

- Docker containerization for all services
- IPFS distributed storage integration
- Hardware security module support
- Geographic redundancy capabilities
- Real-time monitoring and health checks

### Installation Options

#### Basic Installation (Testnet)

```bash
npx wrfcoin-core install
```

#### Production Installation (Mainnet)

```bash
npx wrfcoin-core install --mainnet --hardware-security
```

#### Geographic Distribution Setup

```bash
npx wrfcoin-core install --geographic-redundancy --hardware-security
```

#### Custom Installation

```bash
npx wrfcoin-core install \
  --network mainnet \
  --hardware-security \
  --geographic-redundancy
```

## Command Reference

### Node Management

```bash
# Check node status
wrfcoin status

# Start all services
wrfcoin start

# Stop all services
wrfcoin stop

# Restart services
wrfcoin restart

# Update node software
wrfcoin update
```

### Security Operations

```bash
# Generate hardware-backed keys
wrfcoin keys generate --hardware

# List security keys
wrfcoin keys list

# Backup keys securely
wrfcoin keys backup

# Rotate security keys
wrfcoin keys rotate
```

### Network Operations

```bash
# Connect to network
wrfcoin network connect

# Show peer connections
wrfcoin network peers

# Test cross-chain bridges
wrfcoin network test-bridges

# Check IPFS connectivity
wrfcoin network ipfs-status
```

### Monitoring

```bash
# Real-time system status
wrfcoin monitor

# Generate resilience report
wrfcoin audit

# Check service health
wrfcoin health

# View logs
wrfcoin logs [service]
```

### Governance

```bash
# Submit governance proposal
wrfcoin governance propose

# Vote on proposals
wrfcoin governance vote [proposal-id] [yes/no]

# View governance status
wrfcoin governance status
```

## Architecture Overview

The Wrfcoin CLI manages a distributed architecture designed for maximum resilience:

```
┌─────────────────────────────────────────────────────────────┐
│                    Wrfcoin Sovereign Node                    │
├─────────────────────────────────────────────────────────────┤
│  API Layer     │ WebSocket  │ Cross-Chain │ Governance     │
│  Port 3001     │ Port 8080  │ Port 8001   │ Port 8002      │
├─────────────────────────────────────────────────────────────┤
│  Storage       │ Compute    │ IPFS        │ Monitoring     │
│  Port 7000     │ Port 6001  │ Port 5001   │ Port 9090      │
├─────────────────────────────────────────────────────────────┤
│  PostgreSQL    │ Redis      │ Nginx       │ Grafana        │
│  Port 5432     │ Port 6379  │ Port 80/443 │ Port 3000      │
└─────────────────────────────────────────────────────────────┘
```

## Unstoppable Features

### 🔐 Hardware Security

- YubiKey/HSM integration for critical operations
- Multi-signature governance controls
- Encrypted key storage with hardware backing

### 🌍 Geographic Distribution

- Multi-region deployment capabilities
- Automated failover between regions
- Load balancing across global nodes

### 📡 Censorship Resistance

- IPFS distributed storage
- Tor/I2P network support
- Mesh networking capabilities
- Satellite connectivity options

### 🔄 Self-Healing Architecture

- Automatic service recovery
- Health monitoring and alerting
- Rolling updates without downtime
- Container orchestration

### ⚡ Cross-Chain Resilience

- 13+ bridge protocol support
- Redundant connection paths
- Automatic bridge failover
- Real-time bridge monitoring

## Configuration

### Environment Variables

```bash
# Network Configuration
WRFCOIN_NETWORK=testnet          # testnet|mainnet
WRFCOIN_NODE_ID=auto             # Auto-generated or custom

# Security Configuration
WRFCOIN_HARDWARE_SECURITY=false  # Enable hardware keys
WRFCOIN_KEY_STORAGE=filesystem   # filesystem|hsm

# Database Configuration
DATABASE_URL=postgresql://...    # PostgreSQL connection

# IPFS Configuration
IPFS_ENDPOINT=http://localhost:5001
IPFS_GATEWAY=https://gateway.ipfs.io

# Monitoring Configuration
PROMETHEUS_ENABLED=true
GRAFANA_ENABLED=true
```

### Service-family subcommands (compute / storage / iot / aiml)

These four command groups do **not** talk to the node. They were written against
`wrfcoin/backend`'s per-family microservices, and before #3204 they inherited the node
default (`WRFCOIN_API_URL` → `http://127.0.0.1:8556`) and 404ed against routes the node
never served. Each family now has its own base URL (backend's `/api` prefix included):

| Group | Env var | Default target | Backend service |
|---|---|---|---|
| `compute` | `WRFCOIN_COMPUTE_API_URL` | `http://127.0.0.1:6000/api` | `server/compute_networks_server.ts` |
| `storage` | `WRFCOIN_STORAGE_API_URL` | `http://127.0.0.1:7000/api` | `server/storage_networks_server.js` |
| `iot` | `WRFCOIN_IOT_API_URL` | `http://127.0.0.1:3002/api` | `server/iot_sensor_networks_server*.ts` |
| `aiml` | `WRFCOIN_AIML_API_URL` | `http://127.0.0.1:6001/api` | `server/ai_ml_networks_server*.js` |

`WRFCOIN_API_URL` is deliberately ignored by these groups, and the node's operator/reader
bearer token is not forwarded to them (different trust domain). Every result carries a
`target` block (`service`, `base_url`, `env_var`, `note`) so the output says what it hit.

What each subcommand actually reaches, as of 2026-09 (epic wrfcoin/infra#943):

| Subcommand | Route on the target | Status |
|---|---|---|
| `compute list-providers` | `GET /compute/networks` | catalog only — `integration.wired=false` (backend#1118); output `wired:false` |
| `compute submit-task`, `golem-task` | `POST /compute/submit` | **501 NOT_IMPLEMENTED** from backend (no provider client); surfaced as `Target reports this capability is not wired: …` |
| `compute task-status`, `get-results` | `GET /compute/jobs/{id}` | **501** (same) |
| `compute register-provider` | `POST /compute/providers/register` | **404** — no backend route; node registration is `POST /compute/nodes/register` (backend#57) and needs a `walletAddress` |
| `storage status` | `GET /storage/networks` | responds, but the service is a **simulation** (backend#1117 pending storage-providers#65 / infra#944) |
| `storage store`, `retrieve` | `POST /storage/store`, `POST /storage/retrieve` | **simulated** — nothing reaches IPFS/Filecoin/Arweave |
| `storage list`, `capacity`; `iot list`; `aiml list`, `models` | none (local static tables) | `catalog_only:true`; the figures are not live |
| `iot status` | `GET /iot/networks` | responds |
| `iot register`, `read`, `coverage` | `/iot/sensors`, `/iot/readings`, `/iot/coverage` | **404** — no backend route yet; error names the exact URL |
| `aiml status` | `GET /aiml/networks/status` | responds |
| `aiml submit`, `job` | `POST /ai/inference`, `GET /ai/inference/{id}` | responds; treat results as unverified |

Errors are never silent: a 404 reports `no such route on this service` with the full URL and
exit code 7 (`NOT_FOUND`); backend's honest-gap 501 envelope (`{unavailable:true, reason}`) is
reported as `Target reports this capability is not wired: <reason> [see <issue>]`, also exit 7.

Manual check (backend compute service running locally):

```bash
wrfcoin --json compute list-providers            # ok:true, data.wired:false, data.target.base_url=…:6000/api
WRFCOIN_COMPUTE_API_URL=http://127.0.0.1:9 wrfcoin --json compute list-providers   # connection error, not a 404
```

### Docker Environment

```bash
# Start with Docker Compose
docker-compose -f infrastructure/docker/docker-compose.yml up -d

# Check service status
docker-compose ps

# View logs
docker-compose logs -f [service]

# Scale services
docker-compose up -d --scale wrfcoin-api=3
```

## Troubleshooting

### Common Issues

#### IPFS Connection Failed

```bash
# Start IPFS daemon
ipfs daemon

# Check IPFS status
wrfcoin network ipfs-status

# Reset IPFS configuration
wrfcoin network ipfs-reset
```

#### Database Connection Issues

```bash
# Check database status
wrfcoin status database

# Reset database connection
wrfcoin database reconnect

# Run database migrations
wrfcoin database migrate
```

#### Hardware Security Setup

```bash
# Test hardware key access
wrfcoin keys test-hardware

# Configure YubiKey
wrfcoin keys setup-yubikey

# Generate hardware-backed keys
wrfcoin keys generate --hardware --backup
```

## Development

### Building from Source

```bash
git clone https://github.com/wrfcoin/wrfcoin-core
cd wrfcoin-core
npm install
npm run build
```

### Running Tests

```bash
# Run all tests
npm test

# Run resilience audit
npm run audit

# Test cross-chain bridges
npm run test:bridges

# Test hardware security
npm run test:security
```

## Support

For technical support and documentation:

- Documentation: `docs/`
- Issues: GitHub Issues
- Security: security@wrfcoin.com

## License

MIT License - See LICENSE file for details.

## On-chain forecast submission

Use a contributor bearer in `WRFCOIN_OPERATOR_TOKEN` (or pass `--token`). The
signer file contains an Ed25519 hex private key and must have permissions no
wider than 0600. Keep the forecast JSON object, nonce and timestamp identical
between commit and reveal; the CLI computes the canonical forecast commit ID.
Set the signing chain/network to the destination node's configured identity.

```sh
chmod 600 forecaster.key
wrfcoin --output json forecast commit --node-url http://localhost:8556 \
  --forecaster wrf1forecast --station-id station-forecast \
  --data-file forecast.json --nonce forecast-001 --timestamp 1788979200 \
  --signer-key forecaster.key --chain-id 777 --network-id testnet
wrfcoin --output json forecast reveal --node-url http://localhost:8556 \
  --forecaster wrf1forecast --station-id station-forecast \
  --data-file forecast.json --nonce forecast-001 --timestamp 1788979200 \
  --manifest-file manifest.json --receipt-file receipt.json
```

Replace the example timestamp with a current Unix timestamp before committing,
then reuse it for reveal. The manifest uses the protocol's
`ForecastArtifactManifest` JSON schema. The optional receipt file contains a
signed `ForecastVerificationReceipt` from an authorized oracle, bound to this
manifest and commit ID. Omit `--receipt-file` for baseline scoring; a supplied
missing or invalid file is an error. The node verifies oracle authorization and
the receipt signature. Successful commands return `commit_id` and `tx_id` in
the JSON output. Acceptance into the mempool does not prove a reward was paid.

---

_Making cryptocurrency truly unstoppable, one node at a time._
