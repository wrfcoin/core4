# ZK proving costs

Measured baseline for the `wrfcoin-zk-circuits` Groth16/BN254 stack.
Story #2177, epic #2093 consumer epic #2176. ark 0.4.

**Every figure below is a distribution from repeated runs with warmup, and
every profile carries the machine state that produced it.** The first revision
of this document reported single-shot setup timings; the #2230 review
reproduced `range-proof/8` setup at 314/87/90 ms against a committed 40 ms and
was right to block on it. See [Methodology](#methodology) for what changed and
[What this supersedes](#what-this-supersedes) for what it cost.

---

## Headline

Derived from the **repeated workstation profiles only** (A, B, D). The N5 node
was not re-measured under this methodology — see
[What is not measured](#what-is-not-measured).

| Question | Answer |
|---|---|
| Is proving minutes-per-proof? | **No.** Median 14–858 ms across all circuits and thread configurations. |
| Is proving gigabytes of RAM? | **No.** 2.8–88 MiB phase-process peak RSS. |
| What does a node pay to verify? | **1.41–2.35 ms** median and ~3 MiB, flat in circuit size. |
| How big is a proof? | **Exactly 128 bytes**, every circuit, every profile. |
| What actually dominates cost? | **Thread configuration.** See finding 1. |

The epic's framing — "a correct proof that takes 40 seconds and 16 GB on a
station operator's machine is not a feature" — does not describe this codebase.
Groth16 proving here is a sub-second, sub-100 MiB operation. The costs worth
engineering attention are configuration and caching, not the proof system.

---

## Methodology

Two harnesses.

**`zk-circuits/src/bin/zk-proving-report.rs`** produces the tables below. Each
`(case, phase)` pair runs in its **own child process**, and peak RSS is that
child's `/proc/self/status` `VmHWM`. `VmHWM` is a monotonic high-water mark, so
it is only attributable if exactly one thing ran in the process.

Sampling — every phase warms up, then reports a distribution:

| Phase | Warmup | Timed samples | Reported |
|---|---:|---:|---|
| setup | 1 | 5 | median, with min–max |
| prove | 2 | 10 | median, with min–max |
| verify | 5 | 100 | median, with min–max |
| `process_vk` | 2 | 10 | median, with min–max in the JSON |

`process_vk` was the last single-shot timing and is repeated now for the reason
finding 2 records: one sample of it produced, and then retracted, a documented
finding.

Warmup matters more than it looks: the first key generation in a process pays
page-fault and allocator costs no later one does, and a single-shot harness
reports exactly that sample.

Phases are split the way deployment splits them — `setup` writes keys to disk,
`prove` loads the proving key **from disk** then proves, `verify` loads the
verifying key then verifies. Key load and `process_vk` are timed separately
from prove/verify so neither is smuggled into the other.

**`zk-circuits/benches/proving.rs`** is the criterion sweep over the same
matrix (`zk-circuits/src/bench_matrix.rs`), for regression tracking with
confidence intervals. It is *not* what produced these tables.

All measurements are `--release`. Debug-profile ark arithmetic is ~50x slower
and meaningless.

Raw per-profile JSON, including full provenance, is committed under
`docs/zk/measurements/`. The markdown each sweep emitted is committed beside
it.

### Provenance

| Field | A | B | D |
|---|---|---|---|
| Captured | 2026-07-22T05:51:39Z | 2026-07-22T05:52:10Z | 2026-07-22T05:52:31Z |
| Commit | `a35974e64` | `a35974e64` | `a35974e64` |
| rustc | rustc 1.88.0 (6b00bc388 2025-06-23) | rustc 1.88.0 (6b00bc388 2025-06-23) | rustc 1.88.0 (6b00bc388 2025-06-23) |
| Kernel | 6.6.87.2-microsoft-standard-WSL2 | 6.6.87.2-microsoft-standard-WSL2 | 6.6.87.2-microsoft-standard-WSL2 |
| CPU | AMD Ryzen 9 5950X 16-Core Processor | AMD Ryzen 9 5950X 16-Core Processor | AMD Ryzen 9 5950X 16-Core Processor |
| Cores visible | 32 | 32 | 1 |
| `RAYON_NUM_THREADS` | unset (rayon defaults to all cores) | 4 | 1 |
| Load before | 13.65 5.90 3.60 | 12.67 6.42 3.84 | 9.86 6.17 3.82 |
| Load after | 12.67 6.42 3.84 | 9.86 6.17 3.82 | 5.88 5.67 3.77 |
| Sampling | setup 1+5, prove 2+10, verify 5+100 (warmup+timed) | same | same |

This is a shared development workstation, not a quiet benchmark host; that is
disclosed rather than hidden, and it is part of why A's spreads are the widest.

**Read the load figures carefully — they are partly self-induced.** The three
profiles run back to back, and profile A drives 32 rayon threads, so the
one-minute average it hands to B is elevated *by A*. A "load after" higher than
"load before" is the sweep's own footprint, not evidence of an external
process. Across four independent full sweeps taken while writing this document,
profile A's starting load varied from 2.71 to 4.13 and its `range-proof/8`
prove median varied from 59 to 66 ms — the spread the tables report already
covers that band.

### Hardware profiles

| | Machine | Threads |
|---|---|---|
| **A** | AMD Ryzen 9 5950X, 47 GiB, WSL2 | 32 — **stock configuration** |
| **B** | Same machine | 4 (`RAYON_NUM_THREADS=4`) |
| **D** | Same machine, pinned to one core | 1 |

B and D isolate the thread-count effect from the hardware: same silicon, same
kernel, same build.

---

## Circuit structure and key sizes

Machine-independent — properties of the circuit, identical across profiles.

| Case | Input size | Constraints | PK on disk | PK (compressed) | VK |
|---|---|---:|---:|---:|---:|
| `range-proof/8` | 8 bits | 250 | 95 KiB | 48 KiB | 584 B |
| `range-proof/16` | 16 bits | 258 | 114 KiB | 57 KiB | 584 B |
| `range-proof/32` | 32 bits | 274 | 119 KiB | 59 KiB | 584 B |
| `range-proof/64` | 64 bits | 306 | 129 KiB | 64 KiB | 584 B |
| `confidential-transfer/1x1x64` | 1in x 1out @ 64 bits | 614 | 257 KiB | 129 KiB | 712 B |
| `confidential-transfer/2x2x64` | 2in x 2out @ 64 bits | 744 | 298 KiB | 149 KiB | 712 B |
| `confidential-transfer/4x4x64` | 4in x 4out @ 64 bits | 1,004 | 381 KiB | 190 KiB | 712 B |
| `selective-disclosure/1` | 1 fields | 238 | 92 KiB | 46 KiB | 584 B |
| `selective-disclosure/4` | 4 fields | 484 | 185 KiB | 93 KiB | 584 B |
| `selective-disclosure/16` | 16 fields | 1,942 | 741 KiB | 370 KiB | 584 B |
| `selective-disclosure/64` | 64 fields | 7,774 | 2,962 KiB | 1,481 KiB | 584 B |
| `weather-model/1` | 1 samples | 485 | 186 KiB | 93 KiB | 776 B |
| `weather-model/8` | 8 samples | 1,214 | 512 KiB | 256 KiB | 776 B |
| `weather-model/32` | 32 samples | 4,130 | 1,814 KiB | 907 KiB | 776 B |
| `weather-model/128` | 128 samples | 15,794 | 6,001 KiB | 3,001 KiB | 776 B |

Verifying keys are 584–776 bytes; proving keys 92 KiB to 5.9 MiB.

## Setup — one-time trusted setup

Median, with (min–max) across 5 timed runs.

| Case | Constraints | A. 32 thr | B. 4 thr | D. 1 thr |
|---|---:|---:|---:|---:|
| `range-proof/8` | 250 | 41 (35–50) | 14 (14–15) | 29 (29–30) |
| `range-proof/16` | 258 | 35 (33–40) | 15 (14–15) | 33 (32–35) |
| `range-proof/32` | 274 | 37 (36–44) | 15 (15–16) | 79 (58–126) |
| `range-proof/64` | 306 | 40 (25–44) | 16 (15–16) | 37 (36–43) |
| `confidential-transfer/1x1x64` | 614 | 45 (34–57) | 24 (23–24) | 61 (60–63) |
| `confidential-transfer/2x2x64` | 744 | 41 (34–44) | 26 (26–27) | 68 (68–69) |
| `confidential-transfer/4x4x64` | 1,004 | 41 (38–51) | 31 (30–32) | 88 (85–90) |
| `selective-disclosure/1` | 238 | 37 (31–46) | 13 (13–14) | 27 (27–28) |
| `selective-disclosure/4` | 484 | 38 (32–47) | 20 (20–20) | 46 (45–47) |
| `selective-disclosure/16` | 1,942 | 59 (55–60) | 53 (53–55) | 140 (140–143) |
| `selective-disclosure/64` | 7,774 | 139 (134–147) | 181 (180–186) | 503 (478–511) |
| `weather-model/1` | 485 | 39 (31–46) | 21 (20–22) | 48 (47–53) |
| `weather-model/8` | 1,214 | 53 (45–57) | 40 (38–40) | 103 (102–110) |
| `weather-model/32` | 4,130 | 84 (83–86) | 111 (110–114) | 307 (298–317) |
| `weather-model/128` | 15,794 | 247 (232–260) | 347 (344–353) | 944 (934–981) |

## Proving — per proof, key loaded from disk

Median, with (min–max) across 10 timed runs, in ms.

| Case | Constraints | A. 32 thr | B. 4 thr | D. 1 thr |
|---|---:|---:|---:|---:|
| `range-proof/8` | 250 | 63 (56–67) | 15 (15–16) | 30 (28–36) |
| `range-proof/16` | 258 | 75 (58–81) | 17 (16–18) | 36 (34–58) |
| `range-proof/32` | 274 | 74 (67–82) | 16 (16–17) | 55 (36–58) |
| `range-proof/64` | 306 | 72 (68–85) | 18 (16–18) | 36 (35–43) |
| `confidential-transfer/1x1x64` | 614 | 93 (78–99) | 25 (24–26) | 57 (55–58) |
| `confidential-transfer/2x2x64` | 744 | 89 (76–94) | 25 (25–30) | 62 (56–64) |
| `confidential-transfer/4x4x64` | 1,004 | 88 (78–103) | 27 (26–27) | 57 (56–58) |
| `selective-disclosure/1` | 238 | 59 (52–65) | 14 (14–15) | 29 (28–37) |
| `selective-disclosure/4` | 484 | 79 (67–89) | 21 (20–22) | 46 (46–49) |
| `selective-disclosure/16` | 1,942 | 115 (99–138) | 55 (54–59) | 135 (132–151) |
| `selective-disclosure/64` | 7,774 | 218 (192–250) | 187 (182–192) | 457 (446–463) |
| `weather-model/1` | 485 | 75 (68–80) | 22 (21–23) | 49 (47–51) |
| `weather-model/8` | 1,214 | 112 (87–124) | 43 (42–46) | 101 (99–102) |
| `weather-model/32` | 4,130 | 164 (139–187) | 115 (113–122) | 294 (286–310) |
| `weather-model/128` | 15,794 | 338 (285–375) | 335 (330–345) | 858 (834–917) |

## Verification — per proof, per node

Median, with (min–max) across 100 timed runs, in ms.

| Case | Constraints | A. 32 thr | B. 4 thr | D. 1 thr |
|---|---:|---:|---:|---:|
| `range-proof/8` | 250 | 1.50 (1.41–1.90) | 1.44 (1.41–2.01) | 1.46 (1.42–2.23) |
| `range-proof/16` | 258 | 1.43 (1.39–1.71) | 1.54 (1.40–1.98) | 2.35 (1.99–6.24) |
| `range-proof/32` | 274 | 1.42 (1.39–1.54) | 1.43 (1.40–1.74) | 1.52 (1.43–2.82) |
| `range-proof/64` | 306 | 1.43 (1.39–1.68) | 1.43 (1.40–1.73) | 1.45 (1.40–2.21) |
| `confidential-transfer/1x1x64` | 614 | 1.56 (1.47–1.79) | 1.52 (1.47–2.11) | 1.52 (1.50–1.61) |
| `confidential-transfer/2x2x64` | 744 | 1.54 (1.47–1.90) | 1.50 (1.47–1.60) | 1.56 (1.50–3.68) |
| `confidential-transfer/4x4x64` | 1,004 | 1.54 (1.46–1.71) | 1.55 (1.47–1.74) | 1.51 (1.48–1.58) |
| `selective-disclosure/1` | 238 | 1.47 (1.41–1.60) | 1.41 (1.39–1.49) | 1.47 (1.41–2.13) |
| `selective-disclosure/4` | 484 | 1.46 (1.38–1.70) | 1.43 (1.39–1.79) | 1.49 (1.39–2.25) |
| `selective-disclosure/16` | 1,942 | 1.43 (1.39–1.65) | 1.42 (1.38–1.57) | 1.42 (1.39–1.65) |
| `selective-disclosure/64` | 7,774 | 1.42 (1.39–1.83) | 1.51 (1.41–1.72) | 1.48 (1.39–1.77) |
| `weather-model/1` | 485 | 1.42 (1.39–1.51) | 1.47 (1.40–1.78) | 1.47 (1.41–1.84) |
| `weather-model/8` | 1,214 | 1.43 (1.39–1.65) | 1.50 (1.39–1.78) | 1.44 (1.39–1.93) |
| `weather-model/32` | 4,130 | 1.45 (1.42–1.71) | 1.43 (1.40–1.76) | 1.42 (1.39–2.42) |
| `weather-model/128` | 15,794 | 1.48 (1.39–1.86) | 1.46 (1.41–2.31) | 1.48 (1.42–1.92) |

## Phase-process peak RSS (profile A)

**Labelled precisely:** this is the *whole phase process's* high-water mark —
constraint synthesis, key generation, in-memory keys and serialization buffers
for setup; key load plus proving for prove. It is not incremental or
steady-state memory. Memory is within a few percent across profiles.

| Case | Constraints | Setup peak | Prove peak | Verify peak |
|---|---:|---:|---:|---:|
| `range-proof/8` | 250 | 4.2 MiB | 4.0 MiB | 3.2 MiB |
| `range-proof/16` | 258 | 4.0 MiB | 4.0 MiB | 3.2 MiB |
| `range-proof/32` | 274 | 4.4 MiB | 4.2 MiB | 3.2 MiB |
| `range-proof/64` | 306 | 4.4 MiB | 4.0 MiB | 3.2 MiB |
| `confidential-transfer/1x1x64` | 614 | 6.8 MiB | 6.8 MiB | 3.2 MiB |
| `confidential-transfer/2x2x64` | 744 | 6.5 MiB | 6.0 MiB | 3.2 MiB |
| `confidential-transfer/4x4x64` | 1,004 | 7.0 MiB | 7.0 MiB | 3.2 MiB |
| `selective-disclosure/1` | 238 | 3.8 MiB | 4.0 MiB | 3.2 MiB |
| `selective-disclosure/4` | 484 | 5.3 MiB | 5.5 MiB | 3.2 MiB |
| `selective-disclosure/16` | 1,942 | 13 MiB | 13 MiB | 3.2 MiB |
| `selective-disclosure/64` | 7,774 | 47 MiB | 42 MiB | 3.2 MiB |
| `weather-model/1` | 485 | 5.5 MiB | 6.0 MiB | 3.2 MiB |
| `weather-model/8` | 1,214 | 9.6 MiB | 9.3 MiB | 3.2 MiB |
| `weather-model/32` | 4,130 | 31 MiB | 27 MiB | 3.2 MiB |
| `weather-model/128` | 15,794 | 87 MiB | 86 MiB | 3.2 MiB |

---

## Findings

### 1. Rayon's default thread pool is the dominant cost factor

`ark-groth16`'s **default** feature set includes `parallel`; nothing in this
workspace opted in. Rayon therefore sizes its pool to all cores. At the
constraint counts WRFCoin uses, that is a large net loss.

`range-proof/8` (250 constraints):

| Threads | Prove median | Prove min–max |
|---:|---:|---:|
| 1 | 29.4 ms | 29.2–29.8 ms |
| 2 | 18.7 ms | 18.0–18.8 ms |
| **4** | **14.0 ms** | 13.7–14.7 ms |
| 8 | 16.7 ms | 15.2–18.4 ms |
| 16 | 35.9 ms | 31.9–39.3 ms |
| 32 | 57.4 ms | 43.4–62.8 ms |

`weather-model/128` (15,794 constraints):

| Threads | Prove median | Prove min–max |
|---:|---:|---:|
| 1 | 857.8 ms | 842.9–870.2 ms |
| 2 | 508.9 ms | 500.1–512.9 ms |
| 4 | 341.6 ms | 334.1–352.7 ms |
| **8** | **265.0 ms** | 260.5–273.5 ms |
| 16 | 269.5 ms | 259.5–277.4 ms |
| 32 | 316.2 ms | 299.4–369.9 ms |

**A 32-thread validator proves a range proof ~4x slower than a 4-thread one,
and ~2x slower than a single-threaded one.** The min–max bands at 4 and 32
threads do not overlap — 13.7–14.7 ms against 43.4–62.8 ms in the sweep above,
and 14.6–16.2 ms against 56.0–67.2 ms in the independent B-vs-A profile run —
so this is not sampling noise, and two separate experiments agree on the
direction and the rough magnitude.

The crossover is around ~4,000 constraints; above it more threads finally wins,
and even there the benefit is modest and stops early. For the largest circuit
the optimum is **8 threads** (265 ms), with 16 statistically indistinguishable
from it (269 ms, overlapping bands) and 32 clearly worse (316 ms). An earlier
revision of this document named 16 as the optimum; on repeated measurement 8
and 16 are not separable, so the honest statement is "somewhere around 8-16,
and definitely not 32".

Every production circuit except `weather-model/128` sits below the crossover.

This is the single largest cost effect measured and it is a configuration fix.
It also means "hardware profile" is the wrong axis for capacity planning here:
core count matters more than core speed, and more is worse.

**Design implication for #2176:** numbers taken on a big machine in stock
configuration understate this stack's real speed by up to 4x. Do not size the
architecture off profile A.

### 2. `process_vk` — corrected twice, and the second correction is the interesting one

**Revision 1** claimed `process_vk` costs *more* than a verification (1.0–3.1 ms
against 1.1–1.4 ms), and drew a consensus-path conclusion from it.

**Revision 2** withdrew the magnitude and blamed profile A's thread count —
A measured 2.04–2.73x while B and D sat below 1.3x, which looked exactly like
the rayon pathology of finding 1.

**That attribution was also wrong.** `process_vk` was still being timed *once*
per case while every other phase had been converted to warmup + repeated
samples. One sample on `weather-model/1` came back at 29.9 ms against a ~3.2 ms
typical — a 22x ratio that a single-shot harness reports as data. Repeating
`process_vk` 2+10 like everything else collapses the spread on **every**
profile, including the 32-thread one:

| Profile | `process_vk` : `verify` ratio |
|---|---|
| A (32 thr) | 0.54–0.63x |
| B (4 thr) | 0.54–0.62x |
| D (1 thr) | 0.55–0.92x |

`process_vk` is consistently **cheaper than a verification** — 0.54–0.92x one —
regardless of thread configuration. There is no rayon effect here at all.

The band is tight on A and B (0.54–0.63x) and reaches 0.92x on D. That upper
figure is a single case on the single-core profile, where D's spread is widest
(87% on setup, 66% on prove); it is the sort of number that would have become a
finding under the old single-sample methodology. The `process_vk_min_micros` /
`_max_micros` / `_samples` fields are now in the committed JSON precisely so a
reader can check that judgement instead of taking it.

The operational advice has survived all three revisions unchanged: cache the
`PreparedVerifyingKey`, never call `process_vk` per verification. Every
*explanation* attached to it has been wrong at least once.

Worth stating plainly, because it is the sharpest lesson in this document:
revision 2 was itself a confident, well-argued, wrong diagnosis — it found a
real effect (finding 1) and over-applied it to a measurement that was simply
under-sampled. Repeating a measurement is what distinguishes the two, and the
review's blocker was correct twice over.

### 3. `poseidon_config_bn254()` is recomputed on every synthesis

`poseidon_config_bn254()` derives the Poseidon round constants and MDS matrix
from the Grain LFSR **on every call**, measured at **1.32 ms**
(`zk_fixed_overhead/poseidon_config_bn254`, criterion — this figure has
confidence intervals from criterion's own sampling).

The parameters are constant. Every circuit calls it inside
`generate_constraints`, and `poseidon_hash` calls it again per hash. A
`OnceLock` removes it entirely — cheap, additive, independent of the ark
migration.

### 4. Setup is small, which reframes #1833

The #1833 / #2038 OOM is attributed to on-demand key generation. Measured, the
per-call cost is modest: the largest setup peaks at **87 MiB** and the
largest proving key is 5.9 MiB.

A *single* unauthenticated setup request cannot OOM a node. The vector is
**concurrency**: N simultaneous requests at ~87 MiB and ~0.2–1.2 CPU-seconds
each. That is a rate-limiting problem, and neither a memory cap nor smaller
keys will fix it.

The standing rule is now quantified: loading a key from disk costs
**0.16–10.0 ms** versus 32–228 ms to generate one.

### 5. Verification is not constant-time in public-input count

`Groth16::prepare_inputs` runs an MSM over `vk.gamma_abc_g1`, length
`num_public_inputs + 1`. Verification is linear in public-input count by
construction.

At the sizes in use (1–4 inputs) the effect is unmeasurable — the three
pairings dominate and verify time is flat. `budgets::verification_cost_scales_with_public_inputs`
pins the structural fact rather than asserting a timing ratio, which would be
noise at this scale. Re-measure if a circuit ever takes tens of inputs.

### 6. Proof is 128 bytes; the JSON envelope is ~4x that

`ZkProof::proof_bytes` is **exactly 128 bytes** for every circuit on every
profile — Groth16/BN254 compressed. It does not grow with constraints, witness
size or public inputs.

`ZkProof::to_bytes()` serializes through `serde_json`, producing ~500 bytes — a
**~3.9x** overhead on a consensus-path object. Worth settling which encoding is
canonical before #2178 locks a wire format.

### 7. Proving has a large fixed floor at small sizes

On profile A prove time is nearly flat from 250 constraints to ~1,000. Most of
that floor is finding 1. Constraint-count optimization buys little until the
thread pool is fixed; on B and D cost tracks constraints roughly linearly, as
expected.

---

## Enforced budgets

`zk-circuits/src/budgets.rs`, enforced in the `--lib` test run:

| Budget | Value | Rationale |
|---|---|---|
| `MAX_PROOF_BYTES` | 192 B | Consensus-path storage. Measured exactly 128 B on every profile; headroom catches an encoding change without tripping on noise. |
| `MAX_VERIFY_MILLIS` | 25 ms | Consensus-path CPU. Measured 1.41–2.35 ms median; ~10x headroom so a loaded CI runner cannot flake it. |

Both are costs every node pays on every block, forever. Generation budgets are
deliberately **not** enforced — a generation regression is a product problem,
not a consensus one, and a wall-clock assertion on a 300 ms operation would
flake more than it catches.

`proof_size_is_constant_across_circuits` asserts the *exact* 128 bytes, which
is stronger than the budget and cannot flake.

---

## Reproducing

**Commit first, and write to a scratch directory, not into the tree.** Each
sweep stamps `HEAD` and appends `-dirty` if any tracked file differs from it.
Writing sweep output straight into `docs/zk/measurements/` therefore dirties the
tree, and *every profile after the first* stamps `-dirty` — the earlier
revision of this section could not produce three cleanly-stamped profiles at
all. Stage the outputs, then move them in as one step.

```bash
cargo build --release -p wrfcoin-zk-circuits --bin zk-proving-report
git status --porcelain --untracked-files=no   # must be empty, or every stamp says -dirty

W=$(mktemp -d)

# full sweep, stock configuration
./target/release/zk-proving-report --profile A-workstation-default \
    --out "$W/A-workstation-default.md" --json "$W/A-workstation-default.json"

# tuned thread pool (finding 1)
RAYON_NUM_THREADS=4 ./target/release/zk-proving-report --profile B-workstation-rayon4 \
    --out "$W/B-workstation-rayon4.md" --json "$W/B-workstation-rayon4.json"

# single-core floor
RAYON_NUM_THREADS=1 taskset -c 0 ./target/release/zk-proving-report --profile D-single-core-bound \
    --out "$W/D-single-core-bound.md" --json "$W/D-single-core-bound.json"

cp "$W"/* docs/zk/measurements/

# criterion, with confidence intervals
cargo bench -p wrfcoin-zk-circuits --bench proving
cargo bench -p wrfcoin-zk-circuits --bench proving -- zk_fixed_overhead
```

Each sweep writes its own provenance into both outputs, so a re-run can be
compared against the committed one on equal terms. A full sweep takes ~35 s on
profile A and ~50 s on profile D.

---

## What this supersedes

The first revision reported **single-shot** setup timings and five prove
samples with no warmup, and drew architectural conclusions from them. Measured
spread under the current methodology, worst case per profile:

| Profile | Setup spread (max−min as % of median) | Prove spread |
|---|---:|---:|
| A | 50% | 35% |
| B | 7% | 19% |
| D | 87% | 66% |

A single sample from a distribution that wide cannot support "therefore build X
rather than Y".

**Two conclusions have now failed to survive repetition, and the second failure
is the more instructive.** Finding 2's original magnitude claim died when setup
and prove were repeated. Its *replacement explanation* — that profile A's thread
count was responsible — then died when `process_vk`, the last measurement still
being taken once, was repeated too. A plausible mechanism was available
(finding 1 is real), which is precisely what made the wrong attribution
convincing.

The generalisable rule: **a measurement that has not been repeated is not a
result, however well its number fits an existing theory.** The harness now
repeats every timed quantity it reports, and `commit_stamp()` refuses to record
a clean commit for a dirty tree, so neither failure can recur silently.

---

## What is not measured

- **The N5 testnet node has not been re-measured** under this methodology. The
  earlier single-run figures are in the appendix, clearly separated, and no
  conclusion above depends on them. The node is the live testnet block
  producer and re-running on it needs an explicit decision, so it was not
  re-run for this revision.
- **A real CI runner.** No PR-gating Rust workflow exists in core4 to host one
  (#2233).
- **Genuinely low-end hardware.** Profile D emulates a slow node by pinning a
  fast core. Real slow silicon — an ARM SBC, a 2015 laptop — would be slower
  still, plausibly 2–4x on top of D. Treat D as a lower bound on the lower
  bound.
- **Recursion / aggregation** (#2181) — nothing to measure until #2178 lands.
- **ark 0.5 / 0.6.** Everything here is ark 0.4. The migration (#1472) does not
  change the proof system, so wall-clock should carry over, but must be
  re-taken after it lands.
- **Concurrent load.** Every measurement is a single prover or verifier at a
  time. Given finding 4, the concurrency behaviour of setup is the number that
  actually matters for #1833 and it remains unmeasured.

---

## Appendix — superseded N5 snapshot

Recorded for continuity only. **Single-run, no warmup, old methodology** — not
comparable to the tables above and not load-bearing for any conclusion.

Profile: AMD Ryzen AI 9 HX PRO 370, 8 cores / 32 GiB, LXC on Proxmox, the live
testnet node, run `nice -n 19`, 2026-07-21.

| Case | Constraints | Setup (1 run) | Prove (min of 5) | Verify (min of 100) |
|---|---:|---:|---:|---:|
| `range-proof/8` | 250 | 11 ms | 9.0 ms | 1.077 ms |
| `range-proof/64` | 306 | 14 ms | 9.5 ms | 1.078 ms |
| `confidential-transfer/1x1x64` | 614 | 20 ms | 16.8 ms | 1.138 ms |
| `selective-disclosure/64` | 7,774 | 127 ms | 120.8 ms | 1.084 ms |
| `weather-model/128` | 15,794 | 239 ms | 233.2 ms | 1.083 ms |

Its 8-thread configuration sits near the optimum identified in finding 1, which
is consistent with it being the fastest profile measured — but with one sample
per figure that is an observation, not a result.
