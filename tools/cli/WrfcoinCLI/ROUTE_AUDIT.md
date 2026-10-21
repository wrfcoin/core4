# CLI route review — Refs #3247

Source audit at main `58f1cdcc8add48bf39dafb42130b21e15f7e72b0`, with
the DEX/bridge changes in this PR. Router authority:
`native-chain/src/bin/launch_api/routes/mod.rs` and the launch_api `main.rs`.
Paths below are logical client paths; an `/api` prefix in a backend base URL
does not make that backend a node service. This is a review inventory, not
the complete executable drift gate still tracked by [#3419](https://github.com/wrfcoin/core4/issues/3419).

| Paths raised in review | CLI source/caller | Actual target and disposition | Tracking |
| --- | --- | --- | --- |
| `/swap/quote`, `/swap/execute`, `/swap/price`, `/swap/liquidity` | `commands/swap.rs`; dormant helpers in `api.rs` | No node routes. All DEX commands refuse before I/O, including dry run; hidden from wallet help. Retained option help says unavailable. | This PR; helper inventory under #3419, api.rs ownership #3184 |
| `/bridges/transfer`, `/bridges/transactions/{hash}`, `/bridges/configure` and bridge setup/list/watch paths | `commands/bridge.rs`; dormant transfer helper in `api.rs` | No node routes. All eight command variants now refuse before I/O and are hidden from bridge help. Node `/bridge/safety/*` operator controls are distinct and unchanged. Top-level `bridge` remains parseable/visible while every invocation refuses honestly. | This PR; top-level help ownership #3129 |
| `/nft/create`, `/nft/marketplace` | `commands/nft.rs`; API helpers have no CLI source callers | Node serves POST `/nft/mint`, GET `/nft/list`, GET `/nft/{id}`. Computational create payload differs from mint. Prepared service tranche refuses unsupported operations and uses real list; not included here. | #3247, SOL-I service tranche held for baseline metadata authorization |
| NFT categories/verifiers/submit/verify | `commands/nft.rs` | No matching computational-workflow routes. Node POST `/nft/{id}/verify` has a different operator payload/gate. Prepared service tranche refuses these operations rather than repointing incompatible payloads. | #3247, same held service tranche |
| `/aiml/status`, `/aiml/inference` | API helpers: no CLI source callers | Reachable `commands/aiml.rs:39` uses `ServiceTarget::AiMl`, backend `/aiml/networks/status` and `/ai/inference` (base port 6001 + `/api`). Node route membership is not its contract. | Prior #3204/#3209; dormant helper inventory #3419 |
| `/compute/status`, `/compute/tasks` | API helpers: no CLI source callers | Reachable `commands/compute.rs:117` uses `ServiceTarget::Compute`, backend `/compute/networks`, `/compute/submit`, etc. (port 6000 + `/api`). Provider catalog reports its wiring status. | Prior #3204/#3209; dormant helper inventory #3419 |
| `/storage/status`, `/storage/store` | API status helper has no CLI caller; `commands/storage.rs:39` has a direct store call | Reachable storage commands use `ServiceTarget::Storage` (port 7000 + `/api`), with `/storage/networks` and `/storage/store`; not a node client. Backend integrations retain their own capability/status results. | Prior #3204/#3209; dormant helper inventory #3419 |
| `/iot/*` | `commands/iot.rs:45` uses a backend target; contribute and monitor still use node-targeted IoT paths | Main IoT family uses `ServiceTarget::Iot` (port 3002 + `/api`) and backend `/iot/networks`, sensors/readings/coverage. `contribute.rs:407` still calls `/iot/sensors/{id}`; `monitor.rs:84` still calls `/iot/status` against the node: residual gaps. | Prior #3204/#3209 for IoT family; residual calls #3419 |
| `/mining/rewards` | `ApiClient::get_cross_chain_mining_rewards`: no CLI source caller | Absent node route; dormant helper. Node GET `/mining/stats` is separate and served. | #3419; api.rs ownership #3184 |
| `/wallet/create` | `ApiClient::create_wallet`: no CLI source caller | No node HTTP wallet creation route; local key generation is distinct. | #3419; wallet behavior ownership #3236 |
| `/wallet/{address}` | `tui/app.rs:211` invokes `get_wallet` | Absent. Node GET `/addresses/{address}` has the real account shape; TUI conversion remains a tracked gap. Main wallet send/balance/history are outside this change. | #3419; wallet behavior ownership #3236 |
| `/weather/submit` | `commands/weather.rs`, `contribute.rs:427`, `batch.rs:202` | Absent node route. Backend POST `/api/weather/submit` is real and authenticated. Prepared weather tranche refuses this family; contribute/batch need a separate authenticated integration or honest refusal. | #3247 held weather tranche; contribute/batch #3419 |
| `/weather/stations/{id}`, summaries and `/weather/history` | `tui/app.rs:197`, `contribute.rs:216,298`, `batch.rs:221`, weather history | Absent legacy node paths; station registrations and contribution observations are different contracts. Prepared weather tranche refuses history; other source callers remain tracked. | #3247 held weather tranche; TUI/contribute/batch #3419 |
| `/stats` | `api.rs:721`, `commands/monitor.rs:42,56`; metrics helpers have no CLI caller | Actual node `/network/info` includes `chain_height`, selecting served `/mining/stats`; `/stats` is the legacy-backend branch. Dormant metrics helpers remain. | Prior node-shape behavior retained; dormant helper inventory #3419 |
| `/privacy/enable`, `/privacy/features` | API helpers: no CLI source callers | Absent. Feature-gated `/privacy/keys/*`, commitments, ring signatures, etc. are distinct routes, not aliases for these helpers. | #3419; api.rs ownership #3184 |
| `/transactions/estimate-fee` | API helper: no CLI source caller | Absent; main wallet send's fee behavior belongs to #3390/#3236 and is unchanged here. | #3419; api.rs ownership #3184 |
| `/chain/pending-transactions` | `commands/chain.rs:110`; API helper has no caller (similarly named cli/commands.rs method is a local stub) | Absent. Node serves GET `/mempool/pending` with bounded response shape; needs a real conversion. | #3419 |

Other known gaps remain explicitly tracked: chain height/hash lookup,
export/import and the complete route-contract gate in #3419; wallet-watch
and top-level help ownership #3129; forecast ownership #3150; governance
voting-history in the prepared #3247 service tranche. This PR makes no claim
that every CLI path is served or that all of #3247 is complete.
