# Getting Started with WRFCoin

How to run a local WRFCoin node and interact with it.

## Prerequisites

- **Rust 1.70+** (install via [rustup](https://rustup.rs/))
- **Docker & Docker Compose** (optional, for containerized setup)
- An operator auth token (set via `WRFCOIN_OPERATOR_TOKEN`)

## Option A: Run from source

```bash
# Clone the repository
git clone https://github.com/wrfcoin/core4.git
cd core4

# Build the workspace
cargo build --workspace

# Start the node (API on port 8556 by default)
# Use a generated local token value; do not commit a live operator token.
WRFCOIN_OPERATOR_TOKEN="<generate_with_openssl_rand_hex_32>" \
WRFCOIN_API_HOST=127.0.0.1 \
WRFCOIN_API_PORT=8556 \
cargo run -p wrfcoin-native-chain --bin launch_api
```

## Option B: Run with Docker Compose

```bash
cd core4
docker compose up -d
```

The API server listens on `localhost:8556` by default. P2P networking
listens on port `9000` when enabled.

## Verify the node is running

```bash
# Health check
curl -s http://localhost:8556/health | jq .

# Readiness probe (checks storage, NTP, peers)
curl -s http://localhost:8556/readyz | jq .

# Node version
curl -s http://localhost:8556/version | jq .
```

## Query chain state

```bash
# Current chain info (height, block count, difficulty)
curl -s http://localhost:8556/blockchain | jq .

# List recent blocks
curl -s http://localhost:8556/chain/blocks | jq .

# Get a specific block by height
curl -s http://localhost:8556/chain/block/1 | jq .

# Mining statistics
curl -s http://localhost:8556/mining/stats | jq .
```

## Check an address balance

```bash
# Replace <ADDRESS> with a 40-character hex address
curl -s http://localhost:8556/addresses/<ADDRESS> | jq .
```

## Mine a block

Mining requires the `Operator` auth role. Pass the operator token in the
`Authorization` header:

```bash
curl -s -X POST http://localhost:8556/blocks/mine \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $WRFCOIN_OPERATOR_TOKEN" \
  -d '{"miner_address": "<YOUR_MINER_ADDRESS>"}' | jq .
```

## Submit weather data

Weather observations are submitted as signed transactions. The simplest
path is through the weather collector (see `infra/` repo) or via the REST
API:

```bash
curl -s -X POST http://localhost:8556/transactions/submit \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $WRFCOIN_OPERATOR_TOKEN" \
  -d '{
    "sender_address": "<STATION_ADDRESS>",
    "tx_type": "DataCollection",
    "data": "<BASE64_ENCODED_OBSERVATION>"
  }' | jq .
```

## Enable P2P networking

Set the following environment variables before starting the node:

```bash
WRFCOIN_P2P_ENABLED=true
WRFCOIN_P2P_PORT=9000
WRFCOIN_P2P_BOOTSTRAP=/ip4/<PEER_IP>/tcp/9000/p2p/<PEER_ID>
```

If running behind NAT or Docker, also set:

```bash
WRFCOIN_P2P_EXTERNAL_ADDR=/ip4/<YOUR_PUBLIC_IP>/tcp/9000
```

## WebSocket events

Connect to `ws://localhost:8556/ws` to receive real-time events. After
connecting, subscribe to channels:

```json
{"action": "subscribe", "channel": "blocks"}
```

Available channels: `blocks`, `governance`, `wallet:<address>`,
`rewards:<address>`.

## Key environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `WRFCOIN_API_HOST` | `127.0.0.1` | API bind address |
| `WRFCOIN_API_PORT` | `8556` | API port |
| `WRFCOIN_DATA_DIR` | `./data` | Blockchain data directory |
| `WRFCOIN_OPERATOR_TOKEN` | (none) | Auth token for operator endpoints |
| `WRFCOIN_P2P_ENABLED` | `false` | Enable P2P networking |
| `WRFCOIN_P2P_PORT` | `9000` | P2P listen port |
| `WRFCOIN_P2P_BOOTSTRAP` | (none) | Comma-separated bootstrap peer multiaddrs |
| `WRFCOIN_NETWORK_ID` | `unknown` | Network identifier |
| `WRFCOIN_CHAIN_ID` | `777` | Chain identifier |

## Project layout

| Directory | Description |
|-----------|-------------|
| `native-chain/` | Blockchain runtime and API server |
| `shared-protocol/` | Shared types, crypto, consensus logic |
| `consensus-engines/` | PoW, PoS, PoP, DPoS implementations |
| `p2p/` | libp2p-based P2P networking |
| `privacy/` | Ring signatures, stealth addresses |
| `tools/cli/` | Command-line tools |
| `docs/` | Documentation |

## Next steps

- Browse the [API routes](../native-chain/src/bin/launch_api/routes/mod.rs)
  for the full list of endpoints.
- See the [infra repo](https://github.com/wrfcoin/infra) for Docker,
  Kubernetes, and deployment scripts.
- See the [smart-contracts repo](https://github.com/wrfcoin/smart-contracts)
  for Solidity contracts (AMM, governance, carbon credits, NFTs).
