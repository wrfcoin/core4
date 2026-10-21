# WRFCOIN Route Matrix Audit (Port 4001)

> **Reconciled 2026-07-21:** This is a historical CLI/backend smoke matrix for a
> TypeScript service that is no longer present in this repository. It is not an
> inventory of registered native-node routes. The authoritative, code-checked
> `launch_api` surface is [`docs/security/RPC_SURFACE.md`](security/RPC_SURFACE.md).

This checklist ensures every CLI command path documented in `docs/WRFCOIN_CLI_GUIDE.md` is backed by a running backend process on `http://localhost:4001/api`. It maps 404s/5xx responses to the correct process (main API server, unified API server, or other microservice) and documents any missing routes so launch readiness stays accurate.

## 1. Start the backend stack
1. Run `npm run start:backend` or the equivalent command that boots the service listening on `:4001`. The startup blockers listed in the recent notes (dynamic OTEL, CORS fallback, JWT secret fallback, metrics fix) have been patched so this service should now start without permission errors.
2. Verify `http://localhost:4001/health` and `/ready` return `200` before proceeding.

## 2. Route walk-through
Use either the CLI or `curl` to hit every documented command. Collect the HTTP path and expected handler. At a minimum cover the commands that were previously stubbed but are now wired:

| CLI Command | API Path | Expected backend | Expected behavior | Notes / 404 results |
| --- | --- | --- | --- | --- |
| `wrfcoin wallet balance` | `/api/wallet/balance` | main API | Returns wallet envelope | 404 → route missing in main server? document process responsible |
| `wrfcoin monitor iot` | `/api/monitor/iot` | unified API? note route mapping |
| `wrfcoin forecast publish` | `/api/forecasts` | main/unified? update docs |
| … | … | … | … | … |

## 3. Live audit snapshot
Running the `curl` list from the previous instructions against the backend that is presently bound to `:3001` produced the following observations:

| Category | Sample command | API path(s) hit | HTTP status | Backend process | Notes |
| --- | --- | --- | --- | --- | --- |
| Wallet | `wallet new-address`, `wallet/send` | `/api/wallet/*` | `200 OK` | `simple-http-server.js` (main entrypoint) | Every wallet endpoint responded with `{"success":true,"message":"API endpoint","path":"…"}` so the simple backend currently proxies these paths as expected. |
| Chain | `chain/info`, `chain/block`, `chain/export` | `/api/chain/*` | `200 OK` | `simple-http-server.js` | Chain commands hit the same handler above and all returned the structured API envelope. |
| NFT | `nft/list`, `nft/categories`, `nft/verify` | `/api/nft/*` | `200 OK` | `simple-http-server.js` | No `404`s observed; the simple backend stub is covering the call surface for now. |
| Node | `node/status`, `node/start-mining`, `node/resources` | `/api/node/*` | `200 OK` | `simple-http-server.js` | All node management calls were routed successfully. |
| Cross-chain | `crosschain/list-bridges`, `crosschain/test-connection` | `/api/crosschain/*` | `200 OK` | `simple-http-server.js` | Similar success; continue to map these to the proper runtime when the real routes are ready. |
| Compute | `compute/list-providers`, `compute/task-status` | `/api/compute/*` | `200 OK` | `simple-http-server.js` | |
| Weather | `weather/submit`, `weather/status` | `/api/weather/*` | `200 OK` | `simple-http-server.js` | |
| Forecast | `forecast/publish`, `forecast/list` | `/api/forecast/*` | `200 OK` | `simple-http-server.js` | |
| Setup | `setup/init`, `setup/generate-config` | `/api/setup/*` | `200 OK` | `simple-http-server.js` | |
| Monitor | `monitor/status`, `monitor/iot` | `/api/monitor/*` | `200 OK` | `simple-http-server.js` | |
| System | `config`, `version` | `/api/config`, `/api/version` | `200 OK` | `simple-http-server.js` | Backend currently replies with the placeholder envelope for these routes. |

Record any deviations (404/500) by adding rows beneath this table as soon as the real runtime exposes the service, noting which process should own the handler.
*Tip:* Use `WRFCOIN_API_URL=http://localhost:4001/api` + `curl` or `wrfcoin --json` to run each command. Capture structured `error.code` to determine whether failure is user input (4xx) or backend missing route (404/500).

### 2026-02-18 dual-port rerun (agent verification)
- Environment check:
  - `http://localhost:4001/health` -> `000` (connection failure)
  - `http://localhost:4001/ready` -> `000` (connection failure)
  - `http://localhost:3001/health` -> `200`
- Full matrix (`72` CLI paths tested) results:
  - Port `4001`: `72/72` returned `000` (no listener reachable).
  - Port `3001`: `72/72` returned `200` from `simple-http-server.js`.
- Blocking conclusion:
  - Route ownership mapping between `main_api_server.ts` and `unified_api_server.ts` cannot be completed until a real API process is bound to `:4001`.
  - Current backend runtime available to the CLI audit is still the lightweight placeholder server on `:3001`.

## 3. Tracking 404/500 responses
When a request returns 404/500:
1. Note the HTTP `/api/...` path and timestamp.
2. Check the backend logs for the process owning port 4001 (main API or unified API) to see which router handled the request. If the process name is not immediately obvious, search in `backend/server/routes` for the registered handler.
3. Record the responsible backend entry point, e.g., `main_api_server.ts` vs. `unified_api_server.ts`, and plan route registration there.
4. Once the route exists, re-run the CLI command to confirm the structured exit code is `0` and the JSON envelope is populated.

## 4. Example audit capture template
```csv
command,path,backend,resolution
wrfcoin wallet balance,/api/wallet/balance,main-api-server,routing present
wrfcoin forecast publish,/api/forecasts,main-api-server,needs POST handler (pending)
wrfcoin monitor iot,/api/monitor/iot,unified-api-server,responsive
```

## 5. Post-audit follow-up
- Update `docs/WRFCOIN_CLI_GUIDE.md` once any command path changes or new endpoints are wired.
- If the audit uncovers backend errors (500, timeouts), capture stack traces and attach to the issue tracker for the relevant service.
- After all 14 top-level commands succeed on port 4001, rerun `wrfcoin --help`, `wrfcoin --json config`, and sample commands from each domain to ensure consistent structured output.

## 6. 2026-02-19 CLI smoke run for #278
- Full command-by-command table: `docs/SMOKE_TABLE_278_2026-02-19.md`
- Run input: 76 CLI commands (all active command groups).
- Result: 9 pass, 67 fail.
- Major caveat: the run was executed inside a restricted sandbox; several failures show `tcp open error: Operation not permitted`, which is environment-related and should be revalidated in a normal host shell.
- Reliable findings from this run:
  - `setup` suite is green (`6/6`).
  - `wallet` local flows (`new-address`, `list-addresses`, `history`) are green.
  - Large backend suites remain intentionally not implemented on `main_api_server` and correctly return explicit errors instead of fake fallbacks (forecast, compute, monitor iot/compute/storage/aiml, node control, advanced nft, chain block operations).

## 7. 2026-02-21 Route coverage audit
- **CLI command handlers**: All 8 command handler files (`chain.rs`, `node.rs`, `nft.rs`, `crosschain.rs`, `compute.rs`, `forecast.rs`, `weather.rs`, `monitor.rs`) are fully wired to make real HTTP calls to `main_api_server.ts` via `ApiClient::get()`/`post()`/`delete()`. Zero "not implemented" error messages remain in command handlers.
- **Backend route coverage**: `main_api_server.ts` implements 83+ route handlers covering all CLI command groups:
  - Chain: 5/5 (block, blocks, pending-transactions, export, import)
  - Node: 11/11 (mining start/stop, peers connect/disconnect/ban/unban/banned, sync status/force, resources, reset)
  - NFT: 9/9 (create, submit, categories GET/POST, verifiers GET/POST, verify, marketplace, tokenId)
  - Forecast: 6/6 (create, ensemble, list, show, validate, compare)
  - Compute: 6/6 (status, providers, register, tasks, task-status, task-results)
  - Storage: 2/2 (status, store)
  - AI/ML: 2/2 (status, inference)
  - IoT: 3/3 (status, sensors, readings)
  - Weather: 2/2 (submit, history)
  - Bridges: 5/5 (list, transfer, transactions, configure, test)
  - Governance: 2/2 (proposals, vote)
  - Mining: 2/2 (stats, pools)
  - Network: 1/1 (info)
  - Validators: 1/1
  - Monitor: Uses /iot/status, /compute/status, /storage/status, /aiml/status endpoints
- **API client stubs cleaned up**: 15 methods in `api.rs` that returned `Err(anyhow!("...is not implemented"))` were wired to actual API calls (create_wallet, get_transaction, get_pending_transactions, get_nfts_by_owner, create_nft, transfer_nft, get_node_peers, get_cross_chain_mining_rewards, get_compute_network_status, submit_compute_task, get_storage_network_status, store_data, get_aiml_network_status, submit_ml_inference, get_weather_station_data).
- **Remaining**: Re-run the 76-command smoke test in a non-sandboxed environment to get accurate current-state pass/fail counts.
