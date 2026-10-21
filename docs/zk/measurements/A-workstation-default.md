### Profile: `A-workstation-default`

| Field | Value |
|---|---|
| Profile | `A-workstation-default` |
| Captured | 2026-07-22T05:51:39Z |
| Commit | `a35974e64` |
| rustc | rustc 1.88.0 (6b00bc388 2025-06-23) |
| Kernel | 6.6.87.2-microsoft-standard-WSL2 |
| CPU | AMD Ryzen 9 5950X 16-Core Processor |
| Cores visible | 32 |
| `RAYON_NUM_THREADS` | unset (rayon defaults to all cores) |
| Load avg before / after | 13.65 5.90 3.60 / 12.67 6.42 3.84 |
| Sampling | setup 1+5, prove 2+10, verify 5+100, process_vk 2+10 (warmup+timed) |

#### Setup (one-time, per circuit) — the #1833 cost

| Case | Input size | Constraints | Setup median | Setup min–max | Peak RSS | PK on disk (uncompressed) | PK (compressed) | VK (uncompressed) |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| `range-proof/8` | 8 bits | 250 | 41 ms | 35–50 ms | 4 MiB | 0.09 MiB | 0.05 MiB | 584 B |
| `range-proof/16` | 16 bits | 258 | 35 ms | 33–40 ms | 4 MiB | 0.11 MiB | 0.06 MiB | 584 B |
| `range-proof/32` | 32 bits | 274 | 37 ms | 36–44 ms | 4 MiB | 0.12 MiB | 0.06 MiB | 584 B |
| `range-proof/64` | 64 bits | 306 | 40 ms | 25–44 ms | 4 MiB | 0.13 MiB | 0.06 MiB | 584 B |
| `confidential-transfer/1x1x64` | 1in x 1out @ 64 bits | 614 | 45 ms | 34–57 ms | 7 MiB | 0.25 MiB | 0.13 MiB | 712 B |
| `confidential-transfer/2x2x64` | 2in x 2out @ 64 bits | 744 | 41 ms | 34–44 ms | 6 MiB | 0.29 MiB | 0.15 MiB | 712 B |
| `confidential-transfer/4x4x64` | 4in x 4out @ 64 bits | 1004 | 41 ms | 38–51 ms | 7 MiB | 0.37 MiB | 0.19 MiB | 712 B |
| `selective-disclosure/1` | 1 fields | 238 | 37 ms | 31–46 ms | 4 MiB | 0.09 MiB | 0.04 MiB | 584 B |
| `selective-disclosure/4` | 4 fields | 484 | 38 ms | 32–47 ms | 5 MiB | 0.18 MiB | 0.09 MiB | 584 B |
| `selective-disclosure/16` | 16 fields | 1942 | 59 ms | 55–60 ms | 13 MiB | 0.72 MiB | 0.36 MiB | 584 B |
| `selective-disclosure/64` | 64 fields | 7774 | 139 ms | 134–147 ms | 47 MiB | 2.89 MiB | 1.45 MiB | 584 B |
| `weather-model/1` | 1 samples | 485 | 39 ms | 31–46 ms | 6 MiB | 0.18 MiB | 0.09 MiB | 776 B |
| `weather-model/8` | 8 samples | 1214 | 53 ms | 45–57 ms | 10 MiB | 0.50 MiB | 0.25 MiB | 776 B |
| `weather-model/32` | 32 samples | 4130 | 84 ms | 83–86 ms | 31 MiB | 1.77 MiB | 0.89 MiB | 776 B |
| `weather-model/128` | 128 samples | 15794 | 247 ms | 232–260 ms | 87 MiB | 5.86 MiB | 2.93 MiB | 776 B |

Setup is `1` warmup + `5` timed runs per case.

#### Proving (per proof, key loaded from disk)

| Case | Input size | Key load | Prove (median) | Prove (min–max) | Peak RSS | Proof bytes | Envelope bytes |
|---|---|---:|---:|---:|---:|---:|---:|
| `range-proof/8` | 8 bits | 0 ms | 63 ms | 56–67 ms | 4 MiB | 128 | 503 |
| `range-proof/16` | 16 bits | 0 ms | 75 ms | 58–81 ms | 4 MiB | 128 | 499 |
| `range-proof/32` | 32 bits | 0 ms | 74 ms | 67–82 ms | 4 MiB | 128 | 504 |
| `range-proof/64` | 64 bits | 0 ms | 72 ms | 68–85 ms | 4 MiB | 128 | 492 |
| `confidential-transfer/1x1x64` | 1in x 1out @ 64 bits | 0 ms | 93 ms | 78–99 ms | 7 MiB | 128 | 521 |
| `confidential-transfer/2x2x64` | 2in x 2out @ 64 bits | 1 ms | 89 ms | 76–94 ms | 6 MiB | 128 | 521 |
| `confidential-transfer/4x4x64` | 4in x 4out @ 64 bits | 1 ms | 88 ms | 78–103 ms | 7 MiB | 128 | 525 |
| `selective-disclosure/1` | 1 fields | 0 ms | 59 ms | 52–65 ms | 4 MiB | 128 | 507 |
| `selective-disclosure/4` | 4 fields | 0 ms | 79 ms | 67–89 ms | 6 MiB | 128 | 519 |
| `selective-disclosure/16` | 16 fields | 1 ms | 115 ms | 99–138 ms | 13 MiB | 128 | 512 |
| `selective-disclosure/64` | 64 fields | 6 ms | 218 ms | 192–250 ms | 42 MiB | 128 | 514 |
| `weather-model/1` | 1 samples | 0 ms | 75 ms | 68–80 ms | 6 MiB | 128 | 495 |
| `weather-model/8` | 8 samples | 1 ms | 112 ms | 87–124 ms | 9 MiB | 128 | 516 |
| `weather-model/32` | 32 samples | 4 ms | 164 ms | 139–187 ms | 27 MiB | 128 | 505 |
| `weather-model/128` | 128 samples | 12 ms | 338 ms | 285–375 ms | 86 MiB | 128 | 502 |

#### Verification (per proof, per node — the consensus-path cost)

| Case | Public inputs | `process_vk` | Verify (median) | Verify (min) | Peak RSS | VK on disk |
|---|---:|---:|---:|---:|---:|---:|
| `range-proof/8` | 1 | 0.85 ms | 1.503 ms | 1.406 ms | 3.2 MiB | 0.6 KiB |
| `range-proof/16` | 1 | 0.90 ms | 1.431 ms | 1.393 ms | 3.2 MiB | 0.6 KiB |
| `range-proof/32` | 1 | 0.88 ms | 1.422 ms | 1.392 ms | 3.2 MiB | 0.6 KiB |
| `range-proof/64` | 1 | 0.85 ms | 1.427 ms | 1.393 ms | 3.2 MiB | 0.6 KiB |
| `confidential-transfer/1x1x64` | 3 | 0.86 ms | 1.565 ms | 1.468 ms | 3.2 MiB | 0.7 KiB |
| `confidential-transfer/2x2x64` | 3 | 0.83 ms | 1.540 ms | 1.473 ms | 3.2 MiB | 0.7 KiB |
| `confidential-transfer/4x4x64` | 3 | 0.88 ms | 1.542 ms | 1.464 ms | 3.2 MiB | 0.7 KiB |
| `selective-disclosure/1` | 1 | 0.90 ms | 1.470 ms | 1.412 ms | 3.2 MiB | 0.6 KiB |
| `selective-disclosure/4` | 1 | 0.87 ms | 1.461 ms | 1.381 ms | 3.2 MiB | 0.6 KiB |
| `selective-disclosure/16` | 1 | 0.86 ms | 1.431 ms | 1.385 ms | 3.2 MiB | 0.6 KiB |
| `selective-disclosure/64` | 1 | 0.85 ms | 1.424 ms | 1.388 ms | 3.2 MiB | 0.6 KiB |
| `weather-model/1` | 4 | 0.84 ms | 1.416 ms | 1.389 ms | 3.2 MiB | 0.8 KiB |
| `weather-model/8` | 4 | 0.85 ms | 1.433 ms | 1.392 ms | 3.2 MiB | 0.8 KiB |
| `weather-model/32` | 4 | 0.85 ms | 1.451 ms | 1.421 ms | 3.2 MiB | 0.8 KiB |
| `weather-model/128` | 4 | 0.89 ms | 1.475 ms | 1.394 ms | 3.2 MiB | 0.8 KiB |

Budgets (`zk-circuits/src/budgets.rs`): proof <= 192 B, verify <= 25 ms.
All 15 cases are within budget on this profile.
