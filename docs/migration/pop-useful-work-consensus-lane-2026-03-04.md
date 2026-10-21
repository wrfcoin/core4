# PoP Useful-Work Consensus Migration Notes (2026-03-04)

## Scope
This migration covers consensus and economics changes for:
- `core4#461`: remove active PoW-era low-score fallback from PoP runtime path.
- `core4#467`: make useful-work low-score handling the default PoP mode.
- `core4#471`: enforce useful-work-only migration invariants.
- `core4#464`: task-assigner utility and anti-redundancy hooks.
- `core4#465`: scarcity weighting hooks.
- `core4#474`: multidomain reward hooks with explicit readiness markers.

## Useful-Work Runtime Default (#467)
PoP now keeps low-score blocks on the useful-work liveness path for all heights.

Runtime behavior:
- `create_next_block` no longer routes low-score blocks to nonce mining.
- `verify_block` no longer routes low-score blocks through `verify_block_pow`.
- Low-score acceptance is observable through `useful_work_liveness_fallback=true` metadata.

`pop.useful_work_activation_height` remains in state/metadata for compatibility with existing
surfaces, but it is now locked to `0` and cannot be used as a PoW-compatibility switch.

## Irreversible Useful-Work Checkpoint (#471)
PoP finalization locks useful-work-only mode with fail-fast checks:
- Governance proposal validation rejects `pop.useful_work_activation_height != 0`.
- State transition application rejects `pop.useful_work_activation_height != 0`.
- Consensus invariant `INV-POP-1` fails if post-state activation height is non-zero.
- PoP runtime policy validation rejects non-zero activation heights.

This makes PoW compatibility mode unavailable after finalization.

## New Governance-Tunable Policies
Added to `GovernanceParameters` with validation and parser support:
- `task_assigner_policy`
  - `task_assigner.enabled`
  - `task_assigner.underserved_bonus_multiplier`
  - `task_assigner.redundancy_penalty_multiplier`
- `scarcity_policy`
  - `scarcity.data_desert_multiplier`
  - `scarcity.proximity_multiplier`
- `multidomain_reward_policy`
  - `multidomain.soil_enabled`
  - `multidomain.ocean_enabled`
  - `multidomain.hydrology_enabled`
  - `multidomain.schema_version_min`
  - `multidomain.soil_weight`
  - `multidomain.ocean_weight`
  - `multidomain.hydrology_weight`

## Multidomain Readiness Markers (#474)
Domain reward weighting requires both schema and pipeline readiness markers.

Readiness source of truth in contribution payload JSON:
- Flat keys (supported):
  - `pipeline.backend39_ready`
  - `pipeline.storage_providers6_ready`
- Nested keys (also supported):
  - `pipeline.backend39_ready` as `{"pipeline": {"backend39_ready": true}}`
  - `pipeline.storage_providers6_ready` as `{"pipeline": {"storage_providers6_ready": true}}`

Default behavior:
- If domain is enabled but readiness markers are missing/false, multidomain hook weight resolves to `0.0`.
- If schema version is below `multidomain.schema_version_min`, hook weight resolves to `0.0`.

## Invariants and Parity
Added PoP invariant:
- `INV-POP-1: Useful-Work Activation Non-Regression`

Parity baseline updated in `native-chain/src/parity_shared.rs` to include this invariant name in canonical invariant registry checks.

## Operator Release Checklist
1. Confirm governance value for `pop.useful_work_activation_height` is `0` before rollout and record it in release notes.
2. Run PoP low-score verification tests to confirm useful-work fallback behavior.
3. Run shared-protocol and native-chain parity tests.
4. Confirm no active PoP runtime path uses PoW fallback (all heights).
5. Confirm multidomain readiness markers are present from backend/storage pipelines before enabling domain reward flags.

## Rollback Strategy (#467)
1. Pre-release: take a state snapshot and binary artifact checkpoint before enabling this build.
2. If incidents occur before irreversible checkpointing, rollback by restoring both binary and snapshot together.
3. Do not perform mixed rollback (new state + old binary, or old state + new binary).
4. After `core4#471`, treat useful-work-only mode as irreversible and use forward-fix releases instead of toggling PoW compatibility.
