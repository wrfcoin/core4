# Determinism baseline triage — the 726 frozen findings (#2244)

**Scope:** every finding frozen in `scripts/determinism_baseline.json` (from PR #2245,
`feat/2105-determinism-lint` @ `ebd7de33c`), triaged against `main` as of 2026-07-21.
**No code was changed in this round** — this is classification only, per #2244.

The full per-finding listing with bucket assignments is in
[`BASELINE_TRIAGE_APPENDIX.md`](./BASELINE_TRIAGE_APPENDIX.md) (all 726, `file:line` each).

## Reconciliation

A live `check_determinism.py --report` run on current `main` produces **798 raw line hits**
that collapse to **exactly the 726 frozen fingerprints** — 0 drift since the baseline was
frozen, 0 new findings. (The baseline key is `path|class|text`, so identical flagged text
repeated in one file counts once; the 72-line difference is that dedup, not missing data.)

| Bucket | Count | float-type | float-literal | wall-clock | unseeded-rng | env-var |
|---|---:|---:|---:|---:|---:|---:|
| **1. Consensus-critical** | **48** | 18 | 29 | 1 | – | – |
| **2. Needs a decision** | **7** | – | 5 | – | – | 2 |
| **3. Display / logging / metrics** | **7** | 5 | 2 | – | – | – |
| **4. Test-only** | **3** | – | 3 | – | – | – |
| **5. Unwired library code** | **661** | 287 | 309 | 54 | 11 | – |
| **Total** | **726** | 310 | 348 | 55 | 11 | 2 |

Totals sum to 726. ✔

Bucket 5 was added beyond the four suggested in the handoff because the data demanded it:
**91% of the baseline sits in code the production node never constructs** (evidence below).
Those findings are neither hazards today nor ignorable — they re-promote to bucket 1 the
moment anyone wires the module.

---

## ⚠ Headline: floats ARE on the money path (bucket 1)

The project rule is *all currency is `u64` base units, never `f64`*
(`WRF_UNITS_PER_COIN = 10^8`). The PoP **reward-scoring pipeline violates the spirit of
that rule today**: reward *amounts* are u64, but the *multipliers that scale them* are
computed in `f64` and only quantized to fixed-point (`quantize_f64` → μ-units,
`SCORE_PRECISION = 1_000_000`) at the boundary. Every one of these executes on the
production node (`launch_api` constructs `ProofOfPublishing` via
`ConsensusFactory::create_pop_adapter`, `native-chain/src/bin/launch_api/main.rs:915`).

The single highest-consequence chain, all in
`shared-protocol/src/consensus/proof_of_publishing.rs`:

- `:2180` `contribution_score = contribution_score_mu as f64 / SCORE_PRECISION as f64`
  — a fixed-point score converted *back* to f64 to participate in reward math.
- `:2206` `rep_factor = (raw_rep as f64 / SCORE_SCALE as f64).clamp(0.1, 2.0)` then
  `trust_weight = class_weight * rep_factor` — **f64 multiplication whose product scales
  the block reward**.
- `:2200` `class_weight` clamped in f64; `:2185` oracle confidence default `1.0`.
- `:2497` `(receipt.metrics.skill_score.clamp(0.0, 1.0) * 10_000.0) as u32` — f64→u32
  cast on the forecast-reward path.
- `:73` `BASELINE_FORECAST_SKILL: f64 = 0.1` — the comment itself says
  *"used in the reward path (f64 for emission API compatibility)"*.
- `:3178–3596` — the metric-extraction family (`quantize_f64(*metrics.get(...))` over
  accuracy/complexity/usefulness/quality/volume/uniqueness/amount/verification/impact,
  `multidomain_weight_for_tx` soil/ocean/hydrology weights, `parse_metrics` deserializing
  `FxHashMap<String, f64>` **from transaction JSON**) — attacker-suppliable f64 values
  quantized into reward scores.
- `:987–1008` `set_region_weights` / `set_accuracy_multiplier` /
  `set_reputation_multiplier` and the `BTreeMap<PublishingRole, f64>` percentages — the
  reward-config API stores f64 weights consumed at `:2230`.
- `:85` `chrono::Utc::now()` inside `SystemClock` (the production `Clock` impl) — the
  clock behind PoP timestamp handling; block timestamps reach the block header. Existing
  mitigation: producer-timestamp clamping (PR #1131) treats this as local-policy; it still
  meets this triage's bare definition of consensus-critical ("reaches a block").

Wired satellites of the same pipeline (all bucket 1):

| File | N | Why |
|---|---:|---|
| `shared-protocol/src/consensus/data_desert.rs` | 3 | `cell_for_station(lat: f64, lon: f64)` — `desert_multiplier` feeds the reward path (PoP `:2230`) |
| `shared-protocol/src/consensus/forecast_verification.rs` | 2 | `skill_score: f64` — `BaselineForecastVerifier` is constructed by the production PoP ctor (`:959`); skill feeds rewards (`:1816`, `:2497`) |
| `consensus-engines/src/factory.rs` | 3 | `:212` PoP `default_config` float (used by `main.rs:915`); `:310–312` f64 range-validation of `min_publication_score`, which gates tx acceptance into blocks |
| `consensus-engines/src/algorithms/pop_adapter.rs` | 2 | `:341` the f64→u64 score-scaling guard (`is_finite`/range check) on the production adapter |

PoP consensus-critical findings: 38 of its 44 (37 float + 1 wall-clock).
**Bucket 1 total: 48.**

Important nuance for whoever acts on this: these are *quantized-float* sites, not raw
float-accumulating state. `quantize_f64` truncates to μ-units and has a determinism unit
test (`:14588`). The hazard is the IEEE-754 arithmetic *before* quantization
(compiler/arch-dependent contraction, transcendental behavior) and the policy violation
itself — not an already-observed divergence.

## Bucket 2 — needs a decision (7)

These are the ones I could not classify from code alone. Each comes with its question:

1. `shared-protocol/src/consensus/proof_of_publishing.rs:254`
   (`WRFCOIN_STATE_PRUNING_RETENTION_BLOCKS`) and `:255` (the ack env var) — an env read
   inside a consensus crate, but deliberately guarded: non-default values require an
   explicit acknowledgment env var and log a warning (`decide_state_pruning_retention`).
   **Question:** can two nodes with different acknowledged retention values diverge in any
   externally-visible way (state-root availability, sync responses), or is pruning fully
   outside the root? If the latter, these two deserve `determinism-ok` annotations.
2. `consensus-engines/src/ffi.rs:71–74, 329` — five f64 placeholder values
   (`avg_block_time: 10.0`, zeros, `commission_rate: 0.0`) in `extern "C"` FFI functions.
   The crate has **no `cdylib`/`staticlib` crate-type** and I found no consumer of this
   ABI in the workspace. **Question:** does anything (mobile FFI build?) actually link
   these symbols? If nothing does, this is bucket 5 and a candidate for deletion; if
   mobile does, the f64s cross an ABI boundary and need a real owner.

## Bucket 3 — display / logging / metrics (7)

- `shared-protocol/src/consensus/proof_of_publishing.rs:3820` — `ratio_display` exists
  only to be printed in an `info!` line; the actual difficulty/threshold adjustment right
  above it (`:3814–3817`) is already integer-only. Textbook `determinism-ok` candidate.
- `consensus-engines/src/engine.rs:60, 61, 65` and `consensus-engines/src/traits.rs:105,
  108, 120` — the `NetworkStats` telemetry struct (f64 block-time/throughput/latency)
  held by the production `TypedConsensusManager`. **Nothing ever calls
  `update_network_stats` in native-chain and the PoP adapter never reads it** — it is
  initialized-once telemetry. (The only consumers that make decisions from `NetworkStats`
  are the unwired `dynamic_engine`/`coordinator` modules, bucket 5.)

## Bucket 4 — test-only (3)

All three are inside `mod tests` of `proof_of_publishing.rs` (starts `:4666`) — the
scanner's 400-line backward window is too short for a 4,600-line test module, so they
leaked into the baseline: `:8677`, `:8760`, `:8768` (JSON fixture literals like
`"temperature": 21.0`). No hazard; noted so nobody re-triages them. (A scanner fix —
tracking `#[cfg(test)]` module *spans* instead of a bounded lookback — would remove the
class; that's a #2245-side improvement, not urgent.)

## Bucket 5 — unwired library code (661)

**Evidence of unwiredness** (checked, not assumed): the only workspace crates depending on
`consensus-engines` are `native-chain`, `p2p`, and `fuzz`. The production binary imports
exactly `PopConsensusAdapter`, `TypedConsensusManager`, `ConsensusFactory`/`ConsensusType`,
`equivocation::*`, and `selection::bft_finality` (`BftFinalityGadget`) — none of which
carry baseline findings except the factory/engine lines counted in buckets 1/3. `p2p`
references `bft_finality` only in doc comments; `fuzz` targets `bft_finality` decode.
Inside the crate, `coordination/`, `security/`, and `economics/` are imported **only by
each other** (plus `ffi.rs` → `economics::smart_contract`, see bucket 2). The factory
*can* construct every algorithm engine, but production only ever calls
`create_pop_adapter` (`main.rs:915–916`; PoS is additionally constructed in
`#[cfg(test)]` evidence-store tests).

| Group | Findings | Files (count) | Exemplar |
|---|---:|---|---|
| `coordination/` | 308 | adaptive_fault_tolerance (78), consensus_builder (71), byzantine_coordinator (61), dynamic_engine (60), gossip (17), byzantine (10), hard_fork (9), proposer_builder_separation (2) | `coordination/adaptive_fault_tolerance.rs` — health/latency scoring floats + `Instant::now` |
| `security/` | 231 | manager (77), coordinator (66), attack_prevention (26), correlation_penalties (24), bridge (22), audit (16) | `security/manager.rs` — reputation/threat-score f64 lattices |
| Unconstructed algorithm engines | 42 | raft (15), hotstuff (16 across 4 files), pow (6), pos (3), dag (1), dpos (1) | `algorithms/pow.rs:668` difficulty retarget in f64 — would be bucket 1 **if** PoW were ever selected |
| `selection/` dead code | 30 | finality.rs (29, file is `#![allow(dead_code)]`), vrf.rs (1) | probabilistic-finality engine extracted from blockchain-core, zero users |
| Unused type definitions | 26 | types.rs (15), validator.rs (11) | `PublicationScore`/`ScoreWeights`/`MiningStats`/`ValidatorInfo` — re-exported from `lib.rs` but referenced by no wired code and not by native-chain/p2p at all |
| `economics/` | 12 | smart_contract (11, reachable only via the unconsumed FFI), eip1559 (1) | `economics/eip1559.rs` fee math in f64 |
| `task_orchestrator` | 12 | shared-protocol/src/consensus/task_orchestrator.rs — `TaskOrchestrator` is **never constructed anywhere** in the workspace | assignment-weight f64s incl. `desert_bonus_bps as f64 / 100.0` |

Sum: 308 + 231 + 42 + 30 + 26 + 12 + 12 = **661**. ✔

Two flags inside this bucket worth keeping visible:

- **All 11 `unseeded-rng` findings are here** (hotstuff/crypto, security/, coordination/)
  — i.e. **zero unseeded RNG on the production consensus path**. That materially shrinks
  the issue's "genuine hazards" estimate.
- **54 of 55 `wall-clock` findings are here** too; the one wired clock is PoP's
  `SystemClock` (bucket 1, already mitigated by #1131 clamping).

## What this enables (not done in this round, per the contract)

- The 48 bucket-1 sites are the real debt: a fixed-point (μ-unit) rewrite of the PoP
  multiplier pipeline would retire most of them at once, since they form one dataflow.
- The 7+7 bucket-2/3 sites are annotation work (`determinism-ok:`) plus two owner
  decisions listed above.
- The 661 bucket-5 sites should *not* be annotated file-by-file; the cheaper containment
  is a decision about whether `coordination/`/`security/`/unused engines stay in the
  scanned set (or in the crate) at all — that's a manager/architecture call, not mine.
