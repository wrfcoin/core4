### Profile: `D-single-core-bound`

| Field | Value |
|---|---|
| Profile | `D-single-core-bound` |
| Captured | 2026-07-22T05:52:31Z |
| Commit | `a35974e64` |
| rustc | rustc 1.88.0 (6b00bc388 2025-06-23) |
| Kernel | 6.6.87.2-microsoft-standard-WSL2 |
| CPU | AMD Ryzen 9 5950X 16-Core Processor |
| Cores visible | 1 |
| `RAYON_NUM_THREADS` | 1 |
| Load avg before / after | 9.86 6.17 3.82 / 5.88 5.67 3.77 |
| Sampling | setup 1+5, prove 2+10, verify 5+100, process_vk 2+10 (warmup+timed) |

#### Setup (one-time, per circuit) — the #1833 cost

| Case | Input size | Constraints | Setup median | Setup min–max | Peak RSS | PK on disk (uncompressed) | PK (compressed) | VK (uncompressed) |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| `range-proof/8` | 8 bits | 250 | 29 ms | 29–30 ms | 4 MiB | 0.09 MiB | 0.05 MiB | 584 B |
| `range-proof/16` | 16 bits | 258 | 33 ms | 32–35 ms | 4 MiB | 0.11 MiB | 0.06 MiB | 584 B |
| `range-proof/32` | 32 bits | 274 | 79 ms | 58–126 ms | 4 MiB | 0.12 MiB | 0.06 MiB | 584 B |
| `range-proof/64` | 64 bits | 306 | 37 ms | 36–43 ms | 5 MiB | 0.13 MiB | 0.06 MiB | 584 B |
| `confidential-transfer/1x1x64` | 1in x 1out @ 64 bits | 614 | 61 ms | 60–63 ms | 6 MiB | 0.25 MiB | 0.13 MiB | 712 B |
| `confidential-transfer/2x2x64` | 2in x 2out @ 64 bits | 744 | 68 ms | 68–69 ms | 6 MiB | 0.29 MiB | 0.15 MiB | 712 B |
| `confidential-transfer/4x4x64` | 4in x 4out @ 64 bits | 1004 | 88 ms | 85–90 ms | 7 MiB | 0.37 MiB | 0.19 MiB | 712 B |
| `selective-disclosure/1` | 1 fields | 238 | 27 ms | 27–28 ms | 4 MiB | 0.09 MiB | 0.04 MiB | 584 B |
| `selective-disclosure/4` | 4 fields | 484 | 46 ms | 45–47 ms | 6 MiB | 0.18 MiB | 0.09 MiB | 584 B |
| `selective-disclosure/16` | 16 fields | 1942 | 140 ms | 140–143 ms | 13 MiB | 0.72 MiB | 0.36 MiB | 584 B |
| `selective-disclosure/64` | 64 fields | 7774 | 503 ms | 478–511 ms | 46 MiB | 2.89 MiB | 1.45 MiB | 584 B |
| `weather-model/1` | 1 samples | 485 | 48 ms | 47–53 ms | 5 MiB | 0.18 MiB | 0.09 MiB | 776 B |
| `weather-model/8` | 8 samples | 1214 | 103 ms | 102–110 ms | 9 MiB | 0.50 MiB | 0.25 MiB | 776 B |
| `weather-model/32` | 32 samples | 4130 | 307 ms | 298–317 ms | 26 MiB | 1.77 MiB | 0.89 MiB | 776 B |
| `weather-model/128` | 128 samples | 15794 | 944 ms | 934–981 ms | 84 MiB | 5.86 MiB | 2.93 MiB | 776 B |

Setup is `1` warmup + `5` timed runs per case.

#### Proving (per proof, key loaded from disk)

| Case | Input size | Key load | Prove (median) | Prove (min–max) | Peak RSS | Proof bytes | Envelope bytes |
|---|---|---:|---:|---:|---:|---:|---:|
| `range-proof/8` | 8 bits | 0 ms | 30 ms | 28–36 ms | 4 MiB | 128 | 496 |
| `range-proof/16` | 16 bits | 0 ms | 36 ms | 34–58 ms | 4 MiB | 128 | 507 |
| `range-proof/32` | 32 bits | 0 ms | 55 ms | 36–58 ms | 4 MiB | 128 | 502 |
| `range-proof/64` | 64 bits | 0 ms | 36 ms | 35–43 ms | 4 MiB | 128 | 495 |
| `confidential-transfer/1x1x64` | 1in x 1out @ 64 bits | 1 ms | 57 ms | 55–58 ms | 6 MiB | 128 | 509 |
| `confidential-transfer/2x2x64` | 2in x 2out @ 64 bits | 1 ms | 62 ms | 56–64 ms | 6 MiB | 128 | 523 |
| `confidential-transfer/4x4x64` | 4in x 4out @ 64 bits | 1 ms | 57 ms | 56–58 ms | 7 MiB | 128 | 525 |
| `selective-disclosure/1` | 1 fields | 0 ms | 29 ms | 28–37 ms | 4 MiB | 128 | 501 |
| `selective-disclosure/4` | 4 fields | 0 ms | 46 ms | 46–49 ms | 5 MiB | 128 | 510 |
| `selective-disclosure/16` | 16 fields | 1 ms | 135 ms | 132–151 ms | 12 MiB | 128 | 510 |
| `selective-disclosure/64` | 64 fields | 5 ms | 457 ms | 446–463 ms | 39 MiB | 128 | 520 |
| `weather-model/1` | 1 samples | 0 ms | 49 ms | 47–51 ms | 5 MiB | 128 | 499 |
| `weather-model/8` | 8 samples | 1 ms | 101 ms | 99–102 ms | 9 MiB | 128 | 516 |
| `weather-model/32` | 32 samples | 3 ms | 294 ms | 286–310 ms | 24 MiB | 128 | 508 |
| `weather-model/128` | 128 samples | 12 ms | 858 ms | 834–917 ms | 76 MiB | 128 | 515 |

#### Verification (per proof, per node — the consensus-path cost)

| Case | Public inputs | `process_vk` | Verify (median) | Verify (min) | Peak RSS | VK on disk |
|---|---:|---:|---:|---:|---:|---:|
| `range-proof/8` | 1 | 1.34 ms | 1.455 ms | 1.416 ms | 3.0 MiB | 0.6 KiB |
| `range-proof/16` | 1 | 1.64 ms | 2.345 ms | 1.985 ms | 3.0 MiB | 0.6 KiB |
| `range-proof/32` | 1 | 0.90 ms | 1.525 ms | 1.430 ms | 3.0 MiB | 0.6 KiB |
| `range-proof/64` | 1 | 0.85 ms | 1.450 ms | 1.400 ms | 3.0 MiB | 0.6 KiB |
| `confidential-transfer/1x1x64` | 3 | 0.85 ms | 1.525 ms | 1.504 ms | 3.0 MiB | 0.7 KiB |
| `confidential-transfer/2x2x64` | 3 | 1.02 ms | 1.565 ms | 1.497 ms | 3.0 MiB | 0.7 KiB |
| `confidential-transfer/4x4x64` | 3 | 0.83 ms | 1.509 ms | 1.480 ms | 3.0 MiB | 0.7 KiB |
| `selective-disclosure/1` | 1 | 0.87 ms | 1.466 ms | 1.407 ms | 3.0 MiB | 0.6 KiB |
| `selective-disclosure/4` | 1 | 0.83 ms | 1.489 ms | 1.391 ms | 3.0 MiB | 0.6 KiB |
| `selective-disclosure/16` | 1 | 0.88 ms | 1.416 ms | 1.390 ms | 3.0 MiB | 0.6 KiB |
| `selective-disclosure/64` | 1 | 0.87 ms | 1.479 ms | 1.386 ms | 3.0 MiB | 0.6 KiB |
| `weather-model/1` | 4 | 0.83 ms | 1.471 ms | 1.406 ms | 3.0 MiB | 0.8 KiB |
| `weather-model/8` | 4 | 0.84 ms | 1.441 ms | 1.387 ms | 3.0 MiB | 0.8 KiB |
| `weather-model/32` | 4 | 0.83 ms | 1.418 ms | 1.385 ms | 3.0 MiB | 0.8 KiB |
| `weather-model/128` | 4 | 0.92 ms | 1.480 ms | 1.423 ms | 3.0 MiB | 0.8 KiB |

Budgets (`zk-circuits/src/budgets.rs`): proof <= 192 B, verify <= 25 ms.
All 15 cases are within budget on this profile.
