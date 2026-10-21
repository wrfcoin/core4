### Profile: `B-workstation-rayon4`

| Field | Value |
|---|---|
| Profile | `B-workstation-rayon4` |
| Captured | 2026-07-22T05:52:10Z |
| Commit | `a35974e64` |
| rustc | rustc 1.88.0 (6b00bc388 2025-06-23) |
| Kernel | 6.6.87.2-microsoft-standard-WSL2 |
| CPU | AMD Ryzen 9 5950X 16-Core Processor |
| Cores visible | 32 |
| `RAYON_NUM_THREADS` | 4 |
| Load avg before / after | 12.67 6.42 3.84 / 9.86 6.17 3.82 |
| Sampling | setup 1+5, prove 2+10, verify 5+100, process_vk 2+10 (warmup+timed) |

#### Setup (one-time, per circuit) — the #1833 cost

| Case | Input size | Constraints | Setup median | Setup min–max | Peak RSS | PK on disk (uncompressed) | PK (compressed) | VK (uncompressed) |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| `range-proof/8` | 8 bits | 250 | 14 ms | 14–15 ms | 4 MiB | 0.09 MiB | 0.05 MiB | 584 B |
| `range-proof/16` | 16 bits | 258 | 15 ms | 14–15 ms | 4 MiB | 0.11 MiB | 0.06 MiB | 584 B |
| `range-proof/32` | 32 bits | 274 | 15 ms | 15–16 ms | 4 MiB | 0.12 MiB | 0.06 MiB | 584 B |
| `range-proof/64` | 64 bits | 306 | 16 ms | 15–16 ms | 4 MiB | 0.13 MiB | 0.06 MiB | 584 B |
| `confidential-transfer/1x1x64` | 1in x 1out @ 64 bits | 614 | 24 ms | 23–24 ms | 6 MiB | 0.25 MiB | 0.13 MiB | 712 B |
| `confidential-transfer/2x2x64` | 2in x 2out @ 64 bits | 744 | 26 ms | 26–27 ms | 6 MiB | 0.29 MiB | 0.15 MiB | 712 B |
| `confidential-transfer/4x4x64` | 4in x 4out @ 64 bits | 1004 | 31 ms | 30–32 ms | 7 MiB | 0.37 MiB | 0.19 MiB | 712 B |
| `selective-disclosure/1` | 1 fields | 238 | 13 ms | 13–14 ms | 4 MiB | 0.09 MiB | 0.04 MiB | 584 B |
| `selective-disclosure/4` | 4 fields | 484 | 20 ms | 20–20 ms | 5 MiB | 0.18 MiB | 0.09 MiB | 584 B |
| `selective-disclosure/16` | 16 fields | 1942 | 53 ms | 53–55 ms | 13 MiB | 0.72 MiB | 0.36 MiB | 584 B |
| `selective-disclosure/64` | 64 fields | 7774 | 181 ms | 180–186 ms | 45 MiB | 2.89 MiB | 1.45 MiB | 584 B |
| `weather-model/1` | 1 samples | 485 | 21 ms | 20–22 ms | 5 MiB | 0.18 MiB | 0.09 MiB | 776 B |
| `weather-model/8` | 8 samples | 1214 | 40 ms | 38–40 ms | 9 MiB | 0.50 MiB | 0.25 MiB | 776 B |
| `weather-model/32` | 32 samples | 4130 | 111 ms | 110–114 ms | 27 MiB | 1.77 MiB | 0.89 MiB | 776 B |
| `weather-model/128` | 128 samples | 15794 | 347 ms | 344–353 ms | 88 MiB | 5.86 MiB | 2.93 MiB | 776 B |

Setup is `1` warmup + `5` timed runs per case.

#### Proving (per proof, key loaded from disk)

| Case | Input size | Key load | Prove (median) | Prove (min–max) | Peak RSS | Proof bytes | Envelope bytes |
|---|---|---:|---:|---:|---:|---:|---:|
| `range-proof/8` | 8 bits | 0 ms | 15 ms | 15–16 ms | 4 MiB | 128 | 509 |
| `range-proof/16` | 16 bits | 0 ms | 17 ms | 16–18 ms | 4 MiB | 128 | 503 |
| `range-proof/32` | 32 bits | 0 ms | 16 ms | 16–17 ms | 4 MiB | 128 | 512 |
| `range-proof/64` | 64 bits | 0 ms | 18 ms | 16–18 ms | 4 MiB | 128 | 502 |
| `confidential-transfer/1x1x64` | 1in x 1out @ 64 bits | 1 ms | 25 ms | 24–26 ms | 6 MiB | 128 | 525 |
| `confidential-transfer/2x2x64` | 2in x 2out @ 64 bits | 1 ms | 25 ms | 25–30 ms | 6 MiB | 128 | 517 |
| `confidential-transfer/4x4x64` | 4in x 4out @ 64 bits | 1 ms | 27 ms | 26–27 ms | 6 MiB | 128 | 513 |
| `selective-disclosure/1` | 1 fields | 0 ms | 14 ms | 14–15 ms | 4 MiB | 128 | 509 |
| `selective-disclosure/4` | 4 fields | 0 ms | 21 ms | 20–22 ms | 5 MiB | 128 | 508 |
| `selective-disclosure/16` | 16 fields | 2 ms | 55 ms | 54–59 ms | 12 MiB | 128 | 504 |
| `selective-disclosure/64` | 64 fields | 6 ms | 187 ms | 182–192 ms | 38 MiB | 128 | 510 |
| `weather-model/1` | 1 samples | 0 ms | 22 ms | 21–23 ms | 5 MiB | 128 | 506 |
| `weather-model/8` | 8 samples | 1 ms | 43 ms | 42–46 ms | 9 MiB | 128 | 521 |
| `weather-model/32` | 32 samples | 4 ms | 115 ms | 113–122 ms | 23 MiB | 128 | 503 |
| `weather-model/128` | 128 samples | 11 ms | 335 ms | 330–345 ms | 76 MiB | 128 | 505 |

#### Verification (per proof, per node — the consensus-path cost)

| Case | Public inputs | `process_vk` | Verify (median) | Verify (min) | Peak RSS | VK on disk |
|---|---:|---:|---:|---:|---:|---:|
| `range-proof/8` | 1 | 0.85 ms | 1.442 ms | 1.406 ms | 3.0 MiB | 0.6 KiB |
| `range-proof/16` | 1 | 0.91 ms | 1.545 ms | 1.396 ms | 3.0 MiB | 0.6 KiB |
| `range-proof/32` | 1 | 0.87 ms | 1.431 ms | 1.399 ms | 3.0 MiB | 0.6 KiB |
| `range-proof/64` | 1 | 0.86 ms | 1.425 ms | 1.398 ms | 3.0 MiB | 0.6 KiB |
| `confidential-transfer/1x1x64` | 3 | 0.83 ms | 1.517 ms | 1.474 ms | 2.8 MiB | 0.7 KiB |
| `confidential-transfer/2x2x64` | 3 | 0.88 ms | 1.502 ms | 1.467 ms | 3.0 MiB | 0.7 KiB |
| `confidential-transfer/4x4x64` | 3 | 0.84 ms | 1.546 ms | 1.469 ms | 3.0 MiB | 0.7 KiB |
| `selective-disclosure/1` | 1 | 0.81 ms | 1.412 ms | 1.389 ms | 3.0 MiB | 0.6 KiB |
| `selective-disclosure/4` | 1 | 0.85 ms | 1.426 ms | 1.391 ms | 3.0 MiB | 0.6 KiB |
| `selective-disclosure/16` | 1 | 0.88 ms | 1.421 ms | 1.381 ms | 3.0 MiB | 0.6 KiB |
| `selective-disclosure/64` | 1 | 0.88 ms | 1.515 ms | 1.413 ms | 2.8 MiB | 0.6 KiB |
| `weather-model/1` | 4 | 0.86 ms | 1.472 ms | 1.395 ms | 3.0 MiB | 0.8 KiB |
| `weather-model/8` | 4 | 0.90 ms | 1.496 ms | 1.392 ms | 3.0 MiB | 0.8 KiB |
| `weather-model/32` | 4 | 0.86 ms | 1.433 ms | 1.397 ms | 3.0 MiB | 0.8 KiB |
| `weather-model/128` | 4 | 0.86 ms | 1.460 ms | 1.411 ms | 2.8 MiB | 0.8 KiB |

Budgets (`zk-circuits/src/budgets.rs`): proof <= 192 B, verify <= 25 ms.
All 15 cases are within budget on this profile.
