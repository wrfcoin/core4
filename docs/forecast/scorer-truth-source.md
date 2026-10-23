# Forecast scoring: what is "truth" today, and how `oracle-reference` relates

Status: facts as of `origin/main` 2026-10-19 (core4 `dd1801a3d`, oracle-reference `origin/main`).
Written for epic #3604 Phase A (forecaster earning loop). Anything not read directly from
code or a merged document is marked **UNVERIFIED**. Refs #3067, #3604, #3149.

## 1. The three places a forecast gets a number

| Where | What it computes | Truth it uses | Status |
|---|---|---|---|
| **Consensus** (`shared-protocol/src/consensus/proof_of_publishing.rs`) | Pays a `ForecastReveal` at `receipt.metrics.skill_score` when the attached `ForecastVerificationReceipt` (a) validates, (b) has a signature that verifies under the node's chain binding, (c) names an `oracle_id` that `is_authorized_oracle` (genesis oracle set), (d) `manifest_digest == forecast_manifest_digest(manifest)`, (e) `commit_id` matches; otherwise `BASELINE_FORECAST_SKILL = 0.1` (`:144`, `BASELINE_FORECAST_SKILL_MU = 100_000`). Clauses at `:4193–4204` and `:5346–5389`. | **None.** Consensus never sees an observation. It trusts the receipt's numbers because an oracle-set key signed them. | Live at height 0 (ADR-004 §2.2). |
| **Producer / scorer prototype** (`tools/forecast-proto/src/{main,skill}.rs`) | Per variable `skill = clamp(1 − |forecast − truth| / |climatology − truth|, 0, 1)`, averaged into `skill_score`; MAE/RMSE/bias over the three variables of ONE forecast. Climatology = mean of the station's own input window. Under the unscheduled `forecast_skill_baseline` feature the reference becomes the last observation's persistence error (RMSE form; `skill.rs`). | **The station's own next attested observation** — the reveal payload of the same station at the target time, read from the chain (`read_observations`) or from `wrfcoin station build-observation` artifacts. | Prototype; it also signs the receipt with a throwaway oracle key. |
| **`forecast-score` crate** (`forecast-score/src/*`) | `ForecastAccuracyMetrics::from_pairs`: MAE, RMSE, bias and `skill_score = clamp(1 − RMSE / stddev(observed), 0, 1)`; `from_pairs_with_reference` with `SampleStdDev`, `FixedClimatology { reference_rmse }` or `Persistence`; CRPS/Brier for probabilistic forecasts; categorical scores. `examples/score_csv.rs` runs these over a predictions CSV. | Whatever pairs the caller gives it. The crate defines the arithmetic, not the truth. | Library; consumed by the prototype and by tests. |

Two different skill definitions are therefore in play: the prototype's per-forecast
`1 − MAE/MAE_climatology` (what the signed receipt carries today) and the crate's
`1 − RMSE/stddev(obs)` (what `from_pairs` returns). They agree on "zero skill" for a
forecast no better than its reference and on the `[0, 1]` range, not on the number. A
receipt signer that uses the crate's definition would pay differently from the prototype;
which one the testnet oracle signs is **not decided on main** (#3149 / PR #3150 takes the
metrics as input and is scorer-agnostic).

## 2. What "truth" means for a station forecast today

- The forecast targets a station: `ForecastArtifactManifest.region = station_id`, horizon 6 h,
  variables `temperature_c, pressure_hpa, humidity_pct` (sorted in the manifest).
- The only truth the prototype knows is **that station's own later reading** as revealed on
  chain (`ContributionReveal` payload, `contribution_type = "contribution"`). That reading is
  itself attested: the core4 attester (`native-chain/src/bin/oracle_attester.rs`) approves or
  rejects each contribution commit by physics ranges, freshness and a **reference comparison
  against the nearest NWS/ASOS station** (`WRFCOIN_ATTESTER_NWS_STATION`,
  `WRFCOIN_ATTESTER_REFERENCE_MODE = fixed|shadow|per_station`, bands from
  `shared-protocol/src/weather/reference.rs`). The chain receives only the signed
  `approved` / `confidence`, never the reference value or the residual.
- So the truth a forecast is scored against is **"the contributor's reading that the oracles
  approved"**, not an independent analysis. If the station misreports and the reading still
  lands inside the attester's bands, the forecast is scored against the misreport.
- The public backend route `GET /api/stations/<alias>/observations` exposes the chain tail of
  those readings (~64 blocks); the node's `GET /transactions/contributions/<station_id>/observations`
  exposes the full history to a Reader-role bearer. Humidity is absent in the current WS-5000
  payloads (`humidityPct: null` on both live stations, 2026-10-19), so a three-variable forecast
  for those stations has **no humidity truth** today; the prototype's `read_observations` drops
  readings that lack any of the three variables.

## 3. What `oracle-reference` is and is not

`wrfcoin/oracle-reference` (`app/`, FastAPI, CPU-only) wraps earth2studio **data sources only**
— GFS/GFS_FX, HRRR/HRRR_FX, MRMS, ERA5 — and serves:

- `POST /v1/reference`: per point/time, per source, the gridded reference value in canonical
  units (pressure corrected hypsometrically to station elevation), with `cycle` and `lead_hours`
  (0 for a true analysis; GFS off-cycle times are served from the latest cycle's short-lead
  forecast and say so).
- `POST /v1/residuals`: observations in → `observed − reference` per source, plus
  `band_ratios = |residual| / marginal_max` from the embedded bands v3 (a byte-parity copy of
  `core4/shared-protocol/fixtures/reference_bands.json`, drift-alarm tested), `worst_ratio`,
  advisory `tier`.
- `POST /v1/location-plausibility`: ≥7-day series vs claimed coordinates.
- `POST /v1/forecast`: point extraction from operator-staged GFS/GEFS subsets
  (`docs/forecast-subsets.md`).

Its README states the design: **"Two-pass scoring model (consumed by the backend): advisory at
reveal vs short-lead HRRR_FX/GFS_FX forecasts (no same-instant analysis exists yet),
authoritative at T+6h vs actual HRRR/GFS analysis"** and **"Shadow mode first: advisory scores
and metrics only; zero reward impact until the Phase 0.5 enforcement flip is human-approved."**
The Python attestation loop in `agent/attestation_loop.py` is **RETIRED** (owner ruling
2026-10-03; superseded by the core4 attester) and must not be run against a live node.

Relation to forecast scoring:

- `oracle-reference` verifies **observations** (is this reading plausible against a model
  analysis?). It does not score forecasts and nothing in core4 consensus or in
  `tools/forecast-proto` calls it. The ADR-004 §2.2 mechanism names it as an *alternative* truth
  ("scores against the attested observation (or a reference field from `oracle-reference`)").
- Its residual bands are the same bands the core4 attester applies to contributions, so a
  forecast scored against an attested reading is, indirectly, scored against a reading that
  sat inside those bands.
- `/v1/reference` at the forecast's `valid_at` with `lead_hours = 0` **is** an independent truth
  candidate for a station forecast: the HRRR/GFS analysis value at the station's (coarsened)
  location. It is what the README's "authoritative at T+6h" pass means. **UNVERIFIED:** whether
  the deployed instance (the backend's `REFERENCE_URL`) has the analysis cycles it would need
  at T+6h, and what its latency is for a fresh analysis.

## 4. What a verified-truth receipt would need

A `ForecastVerificationReceipt` already binds `manifest_digest`, `commit_id`, `oracle_id`,
`metrics { mae, rmse, bias, skill_score }`, `verified_at`, `nonce`, optional `zk_proof`, and the
signature over the canonical message under the chain binding (`forecast-score/src/blockchain.rs`).
It does **not** say where the truth came from. For the chain, or anyone auditing a payout, to
know what a skill number was measured against, a receipt would need — without changing the
consensus wire layout, which is pinned by `wire_layout_fixtures.rs` and the bincode golden
fixtures (#2883/#2913):

1. **A truth-source identifier folded into something the receipt already signs.** The cheapest
   place is the manifest's `model_id` suffix convention the prototype already uses
   (`;skill=persistence-rmse/v1`): e.g. `;truth=station-attested/v1` or
   `;truth=oracle-reference:hrrr:analysis/v1`. The manifest digest then binds it, and consensus
   bytes do not change. A new receipt field would be a wire change and needs a `ConsensusFeature`.
2. **A truth record the oracle keeps and can publish**, keyed by `commit_id`: the truth value per
   variable, its source (`station_id` + the reveal's `commit_id` of the truth reading, or the
   `oracle-reference` source/cycle/lead/grid metadata the `/v1/reference` response returns), the
   time pairing (`|t_truth − valid_at|`), and the reference (climatology or persistence) used for
   skill. `tools/forecast-proto` writes this as evidence files today (`05-score.json`,
   `06-receipt.json`); nothing serves it.
3. **A rule for missing or late truth.** If the station never reveals a reading within the
   pairing window, or `oracle-reference` has no analysis for `valid_at`, the receipt must either
   not be issued (reveal falls to baseline 0.1) or be issued with an explicit "no truth" marker.
   Today the prototype refuses to publish when it cannot score.
4. **Humidity (or any variable) absent from the truth** must either drop that variable from both
   `input_variables`/`output_variables` of the manifest (the forecaster declares what can be
   scored) or be scored against the independent reference. The current three-variable manifest
   cannot be fully scored for the two live stations.
5. **Which key signs** (ADR-004 §6 Q3) and whether the signer is the core4 attester process
   (same oracle-set key) or a separate scorer: owner decision, open.
6. **Anti-gaming references** (#3244, `anti_gaming.rs`): the receipt's skill must be measured
   against a reference the forecaster cannot trivially copy; the crate's `Persistence` strategy
   and the prototype's window-climatology are the two inert candidates on main.

## 5. Where this leaves the trainer in this PR

A chain-history model was trained on two live contributor stations as an experiment (same
held-out discipline, two variables); skill 0.4251 vs 0.3561 baseline. The artifact is not
published pending an owner decision. Source stations will be cited by their on-chain station records once station identifiers no longer carry personal names.

`tools/forecast-proto/gpu/train_forecast.py --observations` scores its held-out window with the
prototype's rule (per-forecast `1 − MAE/MAE_window-climatology`, mean over forecasts) because
that is what a signed receipt carries today, and `forecast-score/examples/score_csv.rs` reports
the crate's `from_pairs` metrics over the same predictions so the crate's number is on record
too. Its training truth is public ASOS station observations near the chain's stations (not the
chain's attested readings, which are not reachable read-only beyond a ~1.3 h tail; see
`tools/forecast-proto/models/README.md`). None of this changes what consensus pays: a reveal
with no receipt still mints at 0.1.
