# Forecast scorer job — oracle operator runbook

Status: **not activated.** The job exists; nobody runs it against the live chain until the owner (a) picks
the skill definition, (b) activates the forecaster key binding (`forecaster_key_binding`, #3290), and
(c) names the operators. Testnet tokens carry no promised value; a receipt changes a test credit, not money.

Refs: `wrfcoin/core4-archive#3604` (story A1), `docs/design/agents-earning-phase-b-scope.md` (accepted
2026-10-24: "Oracle operators sign receipts. The forecaster key binding must land first"),
`docs/forecast/scorer-truth-source.md`.

## What it does and why it matters

Consensus pays a revealed forecast at the `skill_score` of an attached, valid
`ForecastVerificationReceipt` signed by an authorized oracle key, and at the 0.1 baseline without one
(`shared-protocol/src/consensus/proof_of_publishing.rs:5378-5393`). **Consensus never recomputes skill.**
So the scorer job is the thing that decides what a forecast earns:

```
forecast_kit prepare-commit records ─┐
scorer's own observation store ──────┼─▶ scorer_job.py ─▶ forecast-score-batch (crate) ─▶ cap ─▶
GET /network/upgrade (binding gate) ─┘        metrics.json + score.json ─▶ wrfcoin forecast sign-receipt ─▶ receipt.json
```

The forecaster attaches the receipt to its own reveal. The job submits nothing to the chain.

## Who runs it, with which key

- **Who:** an oracle operator whose key is in the chain's oracle set (`is_authorized_oracle`). A receipt
  signed by any other key is ignored by consensus and the reveal falls to baseline.
- **Which key:** the oracle signing key in its PKCS#11 role. Production signing needs a CLI built with
  `--features hsm` and `WRFCOIN_HSM_MODULE_PATH`; the PIN is prompted on a tty, never read from the
  environment (`tools/cli/WrfcoinCLI/src/commands/forecast.rs`, `receipt_signer`). A file key works only in
  a debug-build rehearsal (`--rehearsal-oracle-key`), never in production.
- **Truth:** the operator's OWN observation store (`forecast_kit.py fetch-observations`), not files a
  forecaster supplies. The truth is the station's attested reading at `valid_at`; the window is the 24 hourly
  means before `issued_at`. See `docs/forecast/scorer-truth-source.md` for what that truth is and is not.

## The gates (all required; each refusal has its own exit code)

| Gate | Refusal |
|---|---|
| `FORECAST_SIGNER_ENABLED=1` | exit 3, "switched off" |
| `SKILL_DEFINITION` = `rmse_sigma` or `mae_clim`, set explicitly. **There is no default.** | exit 2 |
| The node reports `forecaster_key_binding` known and `active_at_next` in `GET /network/upgrade` | exit 4 |
| `forecast-score-batch` found | exit 2 |
| `FORECAST_ANTI_GAMING` unset, `0` or `1`; `FORECAST_COPY_TOLERANCE` a JSON object of non-negative numbers | exit 2 |

Every reveal record is re-checked against its own files before scoring: the commit id is re-derived from
`data.json` + nonce + station + timestamp, the manifest digest must match, the manifest region must name the
station, and the forecaster must be a 20-byte address (with the key binding, only a key-derived address can
have committed). A record failing any check is listed under `skipped` in `summary.json` and is not scored.

## Skill definitions (owner choice — see the PR's comparison table)

| `SKILL_DEFINITION` | Per variable | Reference |
|---|---|---|
| `rmse_sigma` | `clamp(1 − |e| / σ_window, 0, 1)` | spread of the station's 24 h window (the crate's `1 − RMSE/σ`, via `ReferenceStrategy::FixedClimatology`) |
| `mae_clim` | `clamp(1 − |e| / |clim − truth|, 0, 1)` | how wrong the window mean was this time (the prototype rule) |

Skill is the mean over the variables the forecast declares AND the truth reports (owner ruling 2026-10-19).

## Anti-gaming (story A6, `FORECAST_ANTI_GAMING=1`, off by default)

With the flag on, both definitions change the same two ways (`forecast-score/src/skill.rs`):

1. **Copy rule.** A variable whose forecast is within the copy tolerance of a trivial baseline scores 0. The
   baselines are the window mean (climatology), the last window value (persistence) and any value in
   `--reference-forecasts` (an external model's point forecast, e.g. NOAA/GFS:
   `{station_id: {valid_at: {variable: value}}}`). Default tolerances: 0.05 °C, 0.05 hPa, 0.5 % humidity;
   override with `FORECAST_COPY_TOLERANCE='{"temperature_c": 0.1}'`.
2. **Best-baseline reference.** `rmse_sigma` divides by the smaller of the window spread and the persistence
   error **at the forecast's lead** (lag = `horizon_hours` on the hourly window). `mae_clim` divides by the
   smallest realised error among the baselines this time.

An honest forecast that happens to coincide with a baseline within the tolerance also scores 0 on that variable.
That is the rule working as designed: such a value adds no information over the baseline. It is still a trade
the owner should see (see the A6 PR's differential).

## Cap per cell and lead time (owner ruling 2026-10-24)

Per `(cell, lead window, UTC day of valid_at)`:

- **cell** is the forecast's `station_id` (`manifest.region`). No canonical geo cell is ratified yet; when one
  is, the cap key changes here and nowhere else.
- **lead window** is `ceil(horizon_hours / 6)` (1–6 h → 1, 7–12 h → 2, …), the quota ADR's proposed windows.
- at most **one receipt per forecaster** (its best forecast), then at most `--cap-per-cell-lead` receipts
  (default 3; a placeholder for the owner to set), best skill first, ties by commit id.
- a forecast whose skill does not exceed the 0.1 baseline gets no receipt (it would only lower its own credit).

Consensus already mints at the receipt's score, so the cap needs no consensus change.

## Dry run (do this first, always)

```sh
cargo build -p wrfcoin-forecast-score --bin forecast-score-batch   # through the shared lock and target dir
export FORECAST_SIGNER_ENABLED=1 SKILL_DEFINITION=rmse_sigma        # the definition the owner picked
python tools/forecast-kit/scorer_job.py \
  --reveals ./reveals --observations ./obs --node http://127.0.0.1:8556 \
  --chain-id <id> --network-id <net> --scorer-bin "$CARGO_TARGET_DIR/debug/forecast-score-batch" \
  --out ./scored --dry-run
```

`--dry-run` scores, writes `scored/<commit_id>/{metrics.json,score.json}` and `scored/summary.json` (every
decision with its reason and the exact `sign-receipt` command), and signs nothing. For an air-gapped run, save
`GET /network/upgrade` to a file and pass `--node-status-file`.

Dropping `--dry-run` runs the printed `wrfcoin forecast sign-receipt` commands. The CLI refuses to overwrite an
existing receipt file, refuses a chain/network id that differs from `WRFCOIN_CHAIN_ID`/`WRFCOIN_NETWORK_ID`,
and derives the commit id and manifest digest itself.

## Limits today

- **No API lists revealed forecasts.** `GET /chain/block/{height}` decodes attestation and contribution-reveal
  payloads only, so the operator collects reveal records (the kit's `prepare-commit` directories) from
  forecasters. A read route for `ForecastReveal` payloads is a follow-up.
- Receipts are optional for the forecaster to attach, and a reveal without one earns the 0.1 baseline. The
  scorer cannot stop a skill-less forecast earning the baseline; only consensus can.
- Never run against the live chain before the owner's go-ahead. Tests use fixtures only
  (`tools/forecast-kit/tests/test_scorer_job.py`).
