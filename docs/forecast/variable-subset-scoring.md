# Variable-subset scoring: score only the variables a station reports

Status: owner ruling 2026-10-19 (epic #3604 Phase A, after the finding that both live WS-5000
stations report no humidity). Implemented scorer-side in PR `post-go/fable-forecast/variable-subset`
and in the forecaster kit; **no consensus change is needed or made**. Refs #3604, #3067, #3149.

## The rule

1. A forecast **declares** the variables it forecasts: `ForecastArtifactManifest.output_variables`
   (and `input_variables`), sorted and unique. The forecaster forecasts what the station reports
   throughout its input window and nothing else.
2. The truth observation **reports** some set of variables (the attested reveal payload at the
   target time).
3. The scorer scores **declared ∩ reported**, in sorted order. Per variable the receipt rule is
   unchanged (`max(0, 1 − |e| / |e_window-climatology|)`; under the unscheduled
   `forecast_skill_baseline` feature, the persistence-RMSE form). `skill_score` is the mean over
   the scored variables; MAE/RMSE/bias aggregate over the same set.
4. An empty intersection is **unscorable**: the scorer refuses, no receipt is signed, and the
   reveal (if submitted) mints at `BASELINE_FORECAST_SKILL = 0.1` like any receipt-less reveal.

The kit's pre-existing fallback is the spec: `predict` forecasts the variables present in the
window, `prepare-commit` declares exactly those in the manifest, `score` averages over the
declared-and-reported set and writes the `{mae, rmse, bias, skill_score}` file the receipt
signer (#3149 / PR #3150 `wrfcoin forecast sign-receipt --metrics-file`) consumes.

## Why this touches no consensus path

| Consensus surface | What it checks | Effect of a subset |
|---|---|---|
| `ForecastArtifactManifest::validate` (`forecast-score/src/blockchain.rs`), run by the node route and by block validation | schema 1, model_id length, 64-hex hashes, horizon 1..720, variable lists **non-empty, sorted, unique** | a two-variable list passes today; names are never interpreted |
| `forecast_manifest_digest` / receipt `manifest_digest` | bytes of the manifest | the declared list is bound by the digest: a forecaster cannot declare one set and be scored on another |
| Receipt five clauses (`shared-protocol/src/consensus/proof_of_publishing.rs:4193–4204`, `:5346–5389`) | validate, signature, authorized oracle, digest, commit id | unchanged; the receipt carries aggregate metrics only |
| `ForecastVerifier` trait (`consensus/forecast_verification.rs`) | `BaselineForecastVerifier` rejects everything → baseline | unchanged |
| Wire layout (`wire_layout_fixtures.rs`, bincode goldens #2883/#2913) | positional layout of manifest and receipt | no field added or removed |

So the whole rule lives in whoever computes `metrics` before signing: today `tools/forecast-proto`
(`score`, `skill::score`, via `wrfcoin_forecast_score::{scored_variables, mean_skill}`) and the kit.

## What changed (scorer, crate, kit, CLI)

- `forecast-score/src/variables.rs`: `scored_variables(declared, reported)` and `mean_skill`.
- `tools/forecast-proto`: observations with any subset of the three variables are kept; the
  window's commonly reported variables become the forecast and the manifest's variable lists;
  `score` / `skill::score` iterate the declared-and-reported set, record it as
  `scored_variables` in the score report and the run summary, and error on an empty set so no
  receipt is produced.
- Kit (`tools/forecast-kit`): `score` subcommand (writes `score.json` + `metrics.json`),
  `export-csv`; `predict` reads a model's variable list from its `-export.json`, so a
  two-variable graph (trained with the trainer's `--variables`) runs on a two-variable station.
- CLI: nothing to change — `forecast commit|reveal` take the manifest as a file and validate it
  with the same `validate()`.

## What is deliberately NOT done (would be consensus or policy) — POST-FREEZE

1. **A minimum-variable or weighting rule.** Averaging over the declared set lets a forecaster
   declare only the easiest variable (pressure persists well) and earn a higher mean skill than a
   three-variable forecaster who is better at everything. Today the receipt's `skill_score` is
   what consensus pays, so the incentive exists. Options for the owner (anti-gaming track, #3244 /
   story A6): (a) per-variable weights in the scorer with the weight table named in the
   manifest's `model_id` suffix (scorer-side, no consensus change); (b) a consensus-side
   minimum (`output_variables.len() >= k` for a station that reports ≥ k) — a manifest-validation
   change, needs a `ConsensusFeature` and an activation height → **POST-FREEZE**.
2. **The learned model's loader** (`tools/forecast-proto/src/learned.rs`) still expects a
   `[1,24,3]` graph. A two-variable graph is runnable by the kit, not by the producer, until the
   loader reads the variable list from a parity record. Not a consensus change; left for the
   model-loader follow-up because `main.rs` overlaps open PR #3113.
3. **Truth with more variables than the station usually reports** (e.g. a sensor that starts
   reporting humidity mid-window) is scored on the declared set only; the extra variable is
   ignored. Reverse case (truth lacks a declared variable) shrinks the scored set. Both are by
   design under rule 3.
