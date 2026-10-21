# CLI live contract coverage — why the live smoke leg was removed (#2318)

`wrfcoin-cli-integration.yml` had a `live-contract-smoke` job that started an API server and ran
eight `wrfcoin` CLI commands against it. It was removed rather than repaired. This records the
evidence, because the decision is only defensible with it.

## What it did

`tools/cli/WrfcoinCLI/scripts/live_contract_smoke.sh` started:

```sh
npx tsx backend/server/main_api_server.ts
```

then ran, against `http://localhost:4016/api`:

```
chain info | node start-mining --threads 2 | nft categories
crosschain transaction-info --hash 0xabc   | compute list-providers
weather query-history …                    | forecast list --limit 2 | monitor iot
```

## Three independent defects, each fatal on its own

### 1. The server entrypoint does not exist

`backend/server/main_api_server.ts` was deleted in `fd648c78` (2026-02-24) when `backend/` and
`frontend/` were extracted to standalone repos. `backend/` is entirely absent from this repository —
`git ls-tree origin/main` has no `backend` entry. The job therefore exhausted its 30 health attempts
and exited 7 with `ERR_MODULE_NOT_FOUND`, after paying for a cold Rust build and `npm ci`.

(That `npm ci` was itself vestigial: this repo's `package.json` declares `workspaces: ["contracts"]`,
so it installed Solidity contract dependencies and nothing a server could use.)

### 2. `crosschain` is not a CLI subcommand

Independent of any server, line 4 of the command list could never succeed:

```
$ wrfcoin --json crosschain transaction-info --hash 0xabc
error: unrecognized subcommand 'crosschain'
```

The correct group is `bridge` (`bridge transaction-info` exists). Seven of the eight invocations are
valid; this one never was. **A script cannot be "restored to working" if it was never working.**

### 3. The server it named never served most of these endpoints

The deleted TypeScript source is not recoverable from this repo, but `wrfcoin/backend` retains its
compiled artifact (`server/compiled/server/main_api_server.js`, 2026-02-24). It declares its routes
directly with `app.get/post` — no `express.Router()` mounting, and a catch-all 404 at line 596 — so
its **15 routes are the complete surface**:

```
/health                    /api/stats              /api/network/info
/api/mining/stats          /api/mining/pools       /api/nft/marketplace
/api/nft/:tokenId          /api/transactions       /api/validators
/api/wallet/:address       /api/bridges            /api/governance/proposals
POST /api/bridges/transfer POST /api/governance/vote POST /api/weather/submit
```

None of `/api/node/mining/start`, `/api/compute/providers`, `/api/weather/history`,
`/api/forecasts`, or `/api/iot/status` are among them.

Measured by replaying the eight commands against a stub serving exactly that historical surface:

| Command | Exit | Why |
|---|---|---|
| `chain info` | 0 | `/api/network/info` + `/api/stats` both existed |
| `node start-mining --threads 2` | 7 | `/api/node/mining/start` never existed |
| `nft categories` | 0 | only because `/api/nft/:tokenId` pattern-matched `/api/nft/categories` — it would have returned a token, not categories |
| `crosschain transaction-info` | 2 | invalid subcommand (defect 2) |
| `compute list-providers` | 7 | `/api/compute/providers` never existed |
| `weather query-history` | 7 | `/api/weather/history` never existed |
| `forecast list --limit 2` | 7 | `/api/forecasts` never existed |
| `monitor iot` | 7 | `/api/iot/status` never existed |

**The job would have failed at command 2 even on the day the entrypoint was deleted.** There is no
working state to restore it to.

## Why not point it at core4's own `launch_api`

Checked before assuming. `launch_api` serves exactly one of the eight endpoints:

| Endpoint | In `launch_api`? |
|---|---|
| `/network/info` | **yes** |
| `/stats`, `/node/mining/start`, `/nft/categories`, `/compute/providers`, `/weather/history`, `/forecasts`, `/iot/status` | no |

`launch_api` is a blockchain node API (chain, mining, network, governance, oracle, stake). The CLI's
live surface is the product API — NFT marketplace, compute providers, forecasts, IoT — which is a
different service with a different contract. Pointing the smoke at `launch_api` would produce a job
that is green on one command and red on seven, which is not an improvement over red on all eight.

## What was kept

The mocked legs, untouched and verified healthy. They exercise CLI request construction and command
grouping against a mock HTTP server, which is real coverage:

```
cargo test --locked -p wrfcoin_cli --test api_client_integration     2 passed
cargo test --locked -p wrfcoin_cli --test command_group_integration  16 passed
```

## Recommendation: live coverage belongs in `wrfcoin/backend`, and is not built here

This PR does **not** add live contract coverage anywhere. That is deliberate — siting it is an
architectural decision with real cost, not a cleanup. Recording what it would require:

- **It cannot live in `core4`.** The API the CLI talks to is in `wrfcoin/backend`; core4 has no
  server serving that contract. A live test here would mean checking out a second repository in CI
  and booting its stack — a cross-repo integration job whose cost and ownership need deciding under
  the current Actions budget.
- **The CLI's expected surface must be pinned first.** The eight commands assume endpoints that no
  server in either repo demonstrably serves today. Before a live test can pass, someone must
  establish which service owns `/compute/providers`, `/weather/history`, `/forecasts`, `/iot/status`
  and `/node/mining/start`, and at what version. Until that exists, a live job can only re-discover
  that it does not.
- **`crosschain` → `bridge`** must be corrected in whatever command list a future live test uses.
- **A negative control is required**, per this issue's acceptance criteria: prove that a genuine
  CLI/server contract mismatch fails the job *after* the server has become healthy. That criterion
  is unmet here and is not claimed — it only becomes meaningful once a live job exists at all.

## Trigger status

Unchanged: schedule (weekly) + `workflow_dispatch` only. The issue's criterion "restore a
path-scoped PR trigger **or** explicitly document that this is schedule/manual-only monitoring" is
met by the second branch, and the workflow header now states it. Adding a trigger is a separate
decision under Actions budget pressure and is not made here.

## The class this belongs to

A permanently-red job is worse than no job: while it stays red for an unrelated reason, a genuine
new failure in the same workflow is indistinguishable from the standing noise. This one also carried
a name claiming a property it never exercised — "live contract smoke" that never reached a live
contract. Removing it makes the remaining green signal mean something.

---

## Update (#2328): first recorded-contract coverage added

#2318/#2326 removed the broken live-smoke leg and documented the gap, but the *consequence* — "a
CLI that has never been shown to speak to the supported API" — was untouched. #2328 tracks that
consequence. This is the first real slice against it.

**Added:** `tools/cli/WrfcoinCLI/tests/cli_api_contract.rs` — a boundary contract test that exercises
the real `wrfcoin_cli::ApiClient` against two **recorded** server shapes via `mockito` (no server,
no network, no secret):

```sh
cargo test -p wrfcoin_cli --test cli_api_contract
```

- `tests/fixtures/backend_*.json` — the shape the CLI was built for (Node backend: camelCase inside
  a `{success,data}` envelope, `/api` prefix). The CLI reads real values from it; a rename on either
  side fails the test.
- `tests/fixtures/launch_api_network_info.json` — core4 `launch_api`'s **real** `/network/info` shape
  (snake_case `chain_id`/`network_id`/`chain_height`, no envelope), recorded from
  `native-chain/src/bin/launch_api/routes/network.rs::get_network_info`.

**What it establishes (answering the issue's open question #1):** the CLI and `launch_api` do **not**
share a CLI-facing contract. Run against `launch_api`, `get_blockchain_status()` connects and parses
valid JSON but returns *all defaults* — none of the camelCase fields it reads exist. That drift is
now a tested, committed fact; reconciling the two flips the assertion and forces this file to change.

**Deliberately still a slice — follow-ups:**

1. **One `/network/info` endpoint, one command path.** Extend to the rest of the CLI's surface
   (`chain info`, `bridge`, `nft`, `compute`, `weather`, `forecast`, `monitor`) once each endpoint's
   owning service and version are decided (the surface question this doc already raises).
2. **Recorded, not live.** The `backend_*` fixtures are recorded from the documented backend contract,
   not captured from a running backend. A cross-repo job that boots `wrfcoin/backend` and regenerates
   the fixtures would upgrade "recorded" to "live" — the architectural/cost decision this doc flags.
3. **API-side pin for `launch_api`.** Add a `launch_api`-side test asserting its live `/network/info`
   still emits these field names, so drift is caught from the producer side too (not only the CLI's).
4. **Negative control for a live job.** Still unmet (see above); becomes meaningful once a live job
   exists. The boundary test here already has its own negative direction: the `launch_api` fixture is
   a real mismatch and the test asserts the CLI fails to read it.

---

## Update (#2328): CLI reconciled with `launch_api` and live coverage restored

The CLI now defaults to the supported node API at `http://127.0.0.1:8556`, reads the raw
snake_case `/network/info` response, and fails when required semantic fields disappear instead of
returning plausible defaults. Explicit backend URLs remain compatible with the recorded
`{success,data}` fixtures.

`tools/cli/WrfcoinCLI/scripts/launch_api_contract_smoke.sh` starts the real Rust `launch_api`
binary, runs the built CLI's `chain info` command against it, and asserts the returned network,
chain, version, height, and peer fields. The `wrfcoin-cli-integration` workflow runs that smoke and
the recorded boundary test on path-scoped pull requests as well as its existing schedule/manual
triggers.
