# Wrfcoin CLI Guide

This document walks through every top-level command that ships with the `wrfcoin` CLI and shows a practical example for each subcommand. The CLI is designed for operators, data contributors, and integrators who need structured responses for automation (JSON output, structured exit codes, reusable completions, and agent-friendly flags).

## Global Flags and Environment Variables
| Flag / Env | Purpose |
| --- | --- |
| `--json` | Forces structured `{ ok, data, error }` output for scripts and agents. |
| `--quiet` / `-q` | Emits bare JSON payload for minimum parsing noise. |
| `--why` | Explains routing, retries, and exit-code decisions for debugging. |
| `--no-color` / `NO_COLOR=1` | Disables ANSI coloring when embedding CLI output into pipelines. |
| `--api-url <URL>` / `WRFCOIN_API_URL` | Point the CLI to a different API endpoint (default `http://localhost:4001/api`). |
| `--api-key <KEY>` / `WRFCOIN_API_KEY` | Supply an API key if your backend enforces auth. |
| `--config <FILE>` | Override the default `~/.wrfcoin/cli-config.json` file. |

General usage pattern (JSON preferred for automation):

```bash
wrfcoin wallet balance --json
wrfcoin monitor blockchain --quiet
WRFCOIN_API_URL=https://api.wrfcoin.network wrfcoin weather submit --input station-xyz.json
```

## Command Reference

### wallet
Manage device keys, balances, and history.

| Subcommand | Purpose | Example |
| --- | --- | --- |
| `new-address` | Generate a fresh address in your wallet. | `wrfcoin wallet new-address --json` |
| `list-addresses` | Enumerate configured addresses and highlight the primary. | `wrfcoin wallet list-addresses --quiet` |
| `balance` | Poll the API for the balance of the primary address. | `wrfcoin wallet balance --address 0xAbc123 --json` |
| `set-primary` | Promote an address to be your default signer. | `wrfcoin wallet set-primary 0xAbc123` |
| `send` | Send tokens from the primary address. The testnet charges no network fee, and `--fee`/`--memo` are refused (a transparent transfer carries neither). Add `--dry-run` to preview or `--interactive` to confirm. | `wrfcoin wallet send --to 0xDef456 --amount 10.0 --json` |
| `history` | List recent transactions for an address. | `wrfcoin wallet history --address 0xAbc123 --limit 20 --json` |

### chain
Inspect the blockchain and import/export snapshots.

| Subcommand | Purpose | Example |
| --- | --- | --- |
| `info` | Get chain height, difficulty, and sync status. | `wrfcoin chain info --json` |
| `block` | Dump a block by hash or slot. | `wrfcoin chain block --hash 000000abcd --json` |
| `list-blocks` | Tail the latest blocks (limit, offset). | `wrfcoin chain list-blocks --limit 25 --json` |
| `pending-transactions` | Show transactions waiting in the mempool. | `wrfcoin chain pending-transactions --json` |
| `export` | Export the chain to a file for offline inspection. | `wrfcoin chain export --output ./backup/chain-2026-02-18.bin` |
| `import` | Import an exported chain snapshot. | `wrfcoin chain import --input ./backup/chain-2026-01-01.bin` |

### nft
Manage and verify computational NFTs.

| Subcommand | Purpose | Example |
| --- | --- | --- |
| `list` | List NFTs you own or operate. | `wrfcoin nft list --json` |
| `show` | Show metadata for a specific NFT. | `wrfcoin nft show --id nft-1234 --json` |
| `create` | Mint a new computational NFT. | `wrfcoin nft create --creator 0xAbc123 --title "Storm Model" --description "Forecast run" --computation-type ensemble --input-data ./models/model.json --reward 500` |
| `submit` | Submit a computation result for verification. | `wrfcoin nft submit --id nft-1234 --executor 0xDef456 --result ./results/run42.json --json` |
| `categories` | List NFT categories. | `wrfcoin nft categories --json` |
| `add-category` | Create a new NFT category. | `wrfcoin nft add-category --name "Boundary Layer" --description "Fine-grain physics"` |
| `register-verifier` | Register a verifier node. | `wrfcoin nft register-verifier --capacity 12 --types ensemble --fee 4.5` |
| `list-verifiers` | Show registered verifier nodes. | `wrfcoin nft list-verifiers --computation-type ensemble --json` |
| `verify` | Submit a verifier decision. | `wrfcoin nft verify --id nft-1234 --result-hash abcdef --decision pass --notes "converged" --json` |

### node
Control the local node and peers.

| Subcommand | Purpose | Example |
| --- | --- | --- |
| `info` | Show node metadata, version, and latest sync point. | `wrfcoin node info --json` |
| `status` | High-level health check. | `wrfcoin node status --quiet` |
| `peers` | List connected peers. | `wrfcoin node peers --json` |
| `start-mining` | Start mining threads. | `wrfcoin node start-mining --threads 4 --address 0xMiner --json` |
| `stop-mining` | Stop the miner. | `wrfcoin node stop-mining --json` |
| `mining-stats` | Show current mining performance. | `wrfcoin node mining-stats --json` |
| `connect` | Connect to a peer. | `wrfcoin node connect --address 203.0.113.5:4004 --json` |
| `disconnect` | Gracefully drop a peer. | `wrfcoin node disconnect --peer 203.0.113.5:4004` |
| `ban` | Ban a misbehaving peer for a duration. | `wrfcoin node ban --peer 203.0.113.5:4004 --duration 3600 --json` |
| `unban` | Lift a ban. | `wrfcoin node unban --peer 203.0.113.5:4004` |
| `sync-status` | Report sync progress. | `wrfcoin node sync-status --json` |
| `force-sync` | Trigger an immediate sync attempt. | `wrfcoin node force-sync --json` |
| `resources` | Report CPU/memory/disk usage. | `wrfcoin node resources --json` |
| `reset` | Reset state (blockchain, wallet, peers). | `wrfcoin node reset --blockchain --wallet --peers` |

### crosschain

`NOT_AVAILABLE` in the public release. The node status route is fail-closed and
there are no supported transfer or bridge-configuration commands.

### compute
Coordinate remote compute providers and tasks.

| Subcommand | Purpose | Example |
| --- | --- | --- |
| `list-providers` | Show registered compute providers. | `wrfcoin compute list-providers --json` |
| `submit-task` | Submit a compute task. | `wrfcoin compute submit-task --provider gpu-cluster --task-file ./tasks/run.json --json` |
| `task-status` | Poll a task by ID. | `wrfcoin compute task-status --task-id task-789 --json` |
| `get-results` | Fetch completed task artifacts. | `wrfcoin compute get-results --task-id task-789 --json` |
| `register-provider` | Register a compute provider. | `wrfcoin compute register-provider --provider gpu-cluster --capacity 128 --fee 6.5` |
| `setup-golem` | Bootstrap Golem integration. | `wrfcoin compute setup-golem --config ./golem.json` |
| `golem-task` | Submit a task via Golem. | `wrfcoin compute golem-task --task ./golem-run.json --json` |
| `setup-bacalhau` | Bootstrap Bacalhau integration. | `wrfcoin compute setup-bacalhau --config ./bacalhau.yaml` |

### weather
Submit telemetry and mint weather NFTs.

| Subcommand | Purpose | Example |
| --- | --- | --- |
| `submit` | Submit a single reading. | `wrfcoin weather submit --input ./samples/wind.json --json` |
| `submit-batch` | Send a batch of observations. | `wrfcoin weather submit-batch --input ./samples/batch.json --json` |
| `submit-wind` | Submit a wind-specific reading. | `wrfcoin weather submit-wind --input ./samples/wind.json --json` |
| `create-nft` | Create an NFT for weather contributors. | `wrfcoin weather create-nft --station-id S-100 --title "Wind Rewards" --json` |
| `query-history` | Query historical submissions. | `wrfcoin weather query-history --start 2026-01-01 --end 2026-02-01 --types temperature,wind --json` |
| `status` | Show ingestion queue status. | `wrfcoin weather status --json` |

### forecast
Publish forecasts and compare runs.

| Subcommand | Purpose | Example |
| --- | --- | --- |
| `publish` | Publish a forecast with metadata and payload. | `wrfcoin forecast publish --title "Lake Storm" --forecast-type deterministic --region "Great Lakes" --data-file ./forecasts/lake_storm.json --validity 48h --confidence 0.65 --json` |
| `publish-ensemble` | Publish ensemble members from a directory. | `wrfcoin forecast publish-ensemble --title "Lake Storm Ensemble" --forecast-type ensemble --region "Great Lakes" --members ./forecasts/ensemble/*.json --json` |
| `list` | List forecasts for a region or type. | `wrfcoin forecast list --forecast-type deterministic --region "Great Lakes" --limit 25 --json` |
| `show` | Show forecast details by ID. | `wrfcoin forecast show --id forecast-2026-02-01 --json` |
| `validate` | Validate a forecast against observed data. | `wrfcoin forecast validate --id forecast-2026-02-01 --observed ./obs/ground-truth.json --method ssr --json` |
| `compare` | Compare multiple forecasts. | `wrfcoin forecast compare --ids forecast-2026-02-01,forecast-2026-02-03 --metrics mae,crps --json` |

### profile
Manage CLI profiles for rapid context switching.

| Subcommand | Purpose | Example |
| --- | --- | --- |
| `list` | Show all saved profiles and highlight the active one. | `wrfcoin profile list` |
| `use` | Switch the active profile to another saved workspace. | `wrfcoin profile use station-west` |
| `create` | Clone the current configuration into a new profile name. | `wrfcoin profile create staging` |

### setup
Bootstrap and configure a station node or SDK.

| Subcommand | Purpose | Example |
| --- | --- | --- |
| `init` | Initialize CLI config directory. | `wrfcoin setup init --config-dir ~/.wrfcoin` |
| `generate-config` | Output a new node/station config. | `wrfcoin setup generate-config --output ./configs/station.json` |
| `install-deps` | Install runtime dependencies. | `wrfcoin setup install-deps --profile station` |
| `configure-station` | Write station-specific settings. | `wrfcoin setup configure-station --station-id S-100 --region "Great Lakes"` |
| `configure-backup` | Configure backup endpoints. | `wrfcoin setup configure-backup --target s3://wrfcoin-backups --retention 30d` |
| `generate-sdk` | Scaffold SDK credentials. | `wrfcoin setup generate-sdk --language python --output ./sdks/python-config.json` |

### monitor
Live status and metrics dashboards from the terminal.

| Subcommand | Purpose | Example |
| --- | --- | --- |
| `status` | Show consolidated health summary. | `wrfcoin monitor status --json` |
| `metrics` | Stream live metrics with refresh. | `wrfcoin monitor metrics --live --json` |
| `blockchain` | Show blockchain-specific metrics. | `wrfcoin monitor blockchain --json` |
| `iot` | Report IoT network status. | `wrfcoin monitor iot --json` |
| `compute` | Compute marketplace metrics. | `wrfcoin monitor compute --json` |
| `storage` | Storage node health. | `wrfcoin monitor storage --json` |
| `aiml` | AI/ML pipeline health. | `wrfcoin monitor aiml --json` |

### version
Prints the CLI version (structured).

```bash
wrfcoin version --json
```

### config
Read and write CLI configuration.

```bash
wrfcoin config --list
wrfcoin config --set api_url=https://api.wrfcoin.network
```

### completions
Export shell completion helpers.

```bash
wrfcoin completions bash > ~/.wrfcoin/completions/wrfcoin.bash
wrfcoin completions zsh > ~/.wrfcoin/completions/_wrfcoin
wrfcoin completions fish > ~/.wrfcoin/completions/wrfcoin.fish
```
