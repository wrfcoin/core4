# WRFCoin MCP Server

Refs #3249

The stdio server retains 22 tool names and three resource URIs. Verified reads
and mnemonic operations call mounted routes. Unsupported operations return
an explicit `not wired` error before HTTP, including weather submission whose
current MCP sensor-array payload and station-token integration are unavailable.
It does not promise a reward, confirmation or governance write it cannot perform.

## Setup

Use Node 24 and install this package with `npm ci`. Set `WRFCOIN_API_HOST` to the
unified backend base URL; the local source default is `http://localhost:3001`.
Wallet, weather and governance inherit that URL. Explicit `WRFCOIN_WALLET_HOST`,
`WRFCOIN_WEATHER_HOST` and `WRFCOIN_GOVERNANCE_HOST` overrides remain supported.
The node uses `WRFCOIN_CHAIN_HOST` (default `http://localhost:8556`). Defaults
are local development addresses, not deployment discovery.

`WRFCOIN_API_KEY` sets X-API-Key; `WRFCOIN_JWT_TOKEN` sets the bearer header.
Node reads that require Reader/operator authorization need an appropriate
node bearer credential. `WRFCOIN_REQUEST_TIMEOUT` defaults to 10000 ms.
The existing dotenv loader supports a local `.env`; no credentials are needed
for tests. Configure only endpoints and credentials you intend the server to use.

Run `npm start` from this directory. An MCP host can launch the same command
with this directory as its working directory and use the stdio transport.
No HTTP listener or automatic deployment is introduced.

## Tools and targets

| Tool | Target/path | Status |
| --- | --- | --- |
| generate_mnemonic | backend POST /api/wallet/mnemonic/generate | Mounted; returns backend JSON |
| validate_mnemonic | backend POST /api/wallet/mnemonic/validate | Mounted; forwards mnemonic payload |
| list_hardware_wallets | no hardware discovery endpoint | Not wired; no HTTP |
| submit_weather_data | backend POST /api/weather/submit exists | Not wired: incompatible sensor-array payload and missing station-token integration |
| query_weather_data, get_weather_security_status | no corresponding unfiltered data/security routes | Not wired; no HTTP |
| get_chain_status | backend GET /api/blockchain/status | Mounted; returns backend JSON |
| list_blocks | backend GET /api/blockchain/blocks | Mounted bounded listing; explicit order refuses before HTTP; anonymous pagination may be clamped |
| get_block | backend lookup takes height, not requested hash | Not wired; use get_block_by_height |
| get_transaction | backend hash lookup reports unavailable | Not wired; no HTTP |
| list_proposals | backend GET /api/governance/proposals | Mounted bounded listing; explicit status filter refuses before HTTP |
| create_proposal, vote_on_proposal | backend routes exist with different native/admin/proof inputs | Not wired; no HTTP |
| health_check | backend GET /api/health | Mounted; returns backend JSON |
| get_mining_rewards | no mining reward-history endpoint | Not wired; no HTTP |
| get_block_by_height | node GET /chain/block/{height} | Registered read |
| get_address_balance, get_address | node GET /addresses/{address} | Registered read; identifier escaped |
| get_validator_metrics | node GET /transactions/stake/metrics | Registered read |
| get_commit_status | node GET /transactions/commit/{commit_id}/status | Registered read; identifier escaped |
| get_oracle_status | node GET /transactions/oracle/status | Registered read |
| get_readyz | node GET /readyz | Registered readiness read |

Successful node reads return JSON unchanged; this is source/mocked evidence,
not a claim that any deployment is reachable. Backend routes were verified
against origin/main 8a540f6e984ee402f7def39222532b9a437ad195 and its unified
entrypoint/handlers. The three existing resource names remain: chain/status
and services/health read their mounted backend routes; weather/latest reports
`not wired` without an HTTP request.

## Validation

Run `npm test` and `npm run build`. Tests drive the registered MCP server with
an in-memory MCP client and axios adapters: no sockets, live services or keys.
The method/target/path AST gate scans every tool/resource request against
`src/routes.allowlist.json` and cross-checks node entries against the actual
launch_api router. Dynamic uncheckable paths and unlisted methods fail.
Set `BACKEND_REPO=/path/to/backend` to also check allowlisted backend mounts
and handlers against that checkout's refreshed origin/main. Without it the
backend freshness test is explicitly skipped; this README records the source
revision used for the audit. Adding a hardware HTTP request trips both the
dispatch no-I/O regression and route gate.

zen-mcp-server is an unmodified upstream project with no WRFCoin integration.
Recommend moving it out of core4 in a separate hygiene PR; this work makes no
change to that project.
