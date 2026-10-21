# Native/Shared Parity Matrix

Date: 2026-02-28  
Parent issue: https://github.com/wrfcoin/core4/issues/387

## Scope

This matrix defines parity-critical checks between:
- `wrfcoin-native-chain` wrapper namespace (`native-chain/src/*`)
- `wrfcoin-shared-protocol` canonical implementation (`shared-protocol/src/*`)

Parity classes:
- `serialization`: canonical representation/hash/schema behavior
- `semantics`: state transition or validation behavior
- `error_behavior`: rejection/failure class and message shape

## Coverage Targets

| Module family | Serialization | Semantics | Error behavior |
|---|---|---|---|
| `accounts` | `accounts_serialization_parity` | `accounts_semantics_parity` | `accounts_error_parity` |
| `identity` | `identity_serialization_parity` | `identity_semantics_parity` | `identity_error_parity` |
| `staking` | `staking_serialization_parity` | `staking_semantics_parity` | `staking_error_parity` |
| `weather` | `weather_serialization_parity` | `weather_semantics_parity` | `weather_error_parity` |
| `invariants` | `invariants_serialization_parity` | `invariants_semantics_parity` | `invariants_error_parity` |

Implementation location:
- `native-chain/src/parity_shared.rs`

The parity suite includes a matrix completeness guard that fails tests if any
module/class combination above is missing.

## Baseline Policy

Baseline policy is encoded in `native-chain/src/parity_shared.rs` as:
- `legacy_equivalent`: use when a maintained legacy behavior source exists.
- `canonical_only`: use when no legacy-equivalent baseline is available.

Current matrix policy:

| Module family | Baseline policy | Notes |
|---|---|---|
| `accounts` | `legacy_equivalent` | Adapter-backed parity against archived `blockchain-core/src/core/account_abstraction.rs` surface |
| `identity` | `legacy_equivalent` | Adapter-backed parity against archived `blockchain-core/src/core/identity/{did,naming_service,verifiable_credentials}.rs` fixture surface |
| `staking` | `legacy_equivalent` | Adapter-backed parity against archived `blockchain-core/src/core/liquid_staking.rs` fixture shape |
| `weather` | `canonical_only` | No maintained legacy-equivalent baseline in this matrix scope |
| `invariants` | `canonical_only` | Canonical shared-protocol invariants are the baseline authority |

## Identity Legacy Adapter Boundary (Issue #418)

Legacy source status:
- Archived baseline paths: `blockchain-core/src/core/identity/{did,naming_service,verifiable_credentials}.rs`.
- In-repo source status: files are not present in this repository; parity uses archived fixture adapters in `native-chain/src/parity_shared.rs`.

Included parity scope:
- Adapter-backed projection parity from legacy identity fixture surface into canonical `ContributorIdentity` registry model.
- Deterministic serialization parity for adapted identity snapshot representations.
- Semantics parity for registration and verification-level transitions.
- Error-behavior parity for conflicting duplicate registration and invalid transition paths (verification update on a missing identity).

Excluded parity boundary:
- Legacy DID document resolution, naming ownership proofs, and verifiable-credential revocation semantics are intentionally excluded from canonical native/shared identity-registry parity scope.
- The boundary is asserted in `identity_parity_boundary_excluded_capabilities_rule_is_explicit`; excluded legacy metadata must not alter canonical register/update outcomes.

## Staking Legacy Adapter Boundary (Issue #419)

Legacy source status:
- Archived baseline path: `blockchain-core/src/core/liquid_staking.rs`.
- In-repo source status: file is not present in this repository; parity uses archived fixture adapters in `native-chain/src/parity_shared.rs`.

Included parity scope:
- Adapter-backed projection parity from legacy staking fixture shape into canonical native/shared liquid staking pool state.
- Deterministic serialization parity for seeded state and post-deposit state snapshots.
- Exchange-rate-sensitive semantics parity for mint/redeem transitions (seeded pool `3000/2000`, mint `750 -> 500`, redeem `400 -> 600`, post-state `3150/2100`).
- Error-behavior parity for zero-amount, insufficient-liquidity, and math-overflow rejects.

Excluded parity boundary:
- Legacy unbonding queue/epoch-delay semantics are intentionally excluded from canonical native/shared liquid staking parity scope.
- The boundary is asserted in `staking_parity_boundary_unbonding_rule_is_explicit`; unbonding metadata must not alter canonical mint/redeem outcomes.

Invariants drift assertions additionally pin canonical baseline expectations for:
- Registered invariant names and ordering.
- Semantics projection for `ApplyReward` parity scenario.
- Emission-cap failure name/pass/detail tuple.

These assertions are covered by dedicated negative tests so baseline drift fails
deterministically in the parity gate.

## Accounts Legacy Adapter Boundary (Issue #417)

Included in parity boundary (must remain equivalent between native wrapper and shared protocol):
- Legacy fixture adapter projection from archived blockchain-core account-abstraction fields into canonical `UserOperation` fields.
- Serialization/hash parity:
  - canonical user-operation projection parity after adapter mapping,
  - chain-aware `user_operation_hash` parity and chain-id variance checks.
- Semantics parity:
  - nonce progression and replay rejection parity,
  - signature verification path parity,
  - sponsorship policy application parity.
- Error behavior parity:
  - equivalent rejection category/message contract for invalid time window, signature, nonce mismatch, sponsorship-disallowed, and sponsored-fee bound errors.

Explicitly excluded from this parity boundary:
- Legacy entry-point deposit accounting and paymaster deposit balance checks.
- Legacy account-factory deployment (`init_code`) and bundler/aggregation internals.

## CI Gate

Workflow:
- `.github/workflows/rust-shared-native-parity.yml`

Job:
- `Shared/Native Parity Tests`

PR policy:
- For branch protection, mark `Rust Shared/Native Parity Gate / Shared/Native Parity Tests` as a required check for `main`.
- Workflow triggers on PRs touching `native-chain/**` or `shared-protocol/**`.

## Ownership

- Protocol parity tests: `@wrfcoin/blockchain-team`
- Gate/workflow maintenance: `@wrfcoin/devops-team`
- Coverage quality review: `@wrfcoin/qa-team`

CODEOWNERS anchors:
- `/native-chain/src/parity.rs`
- `/native-chain/src/parity_shared.rs`
- `/.github/workflows/rust-shared-native-parity.yml`
