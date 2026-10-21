# Issue #386 Exploration: Panic-Free Transition/Apply Error Strategy

Date: 2026-02-27  
Scope: `shared-protocol` + `native-chain` transition/apply/state-machine runtime paths (tests excluded unless they reveal production wiring).

## 1. Audit Results

### 1.1 High-priority callsites (transition/apply path)

| File:Line | Current panic pattern | Category | Consensus-critical path? | Proposed replacement | Priority |
|---|---|---|---|---|---|
| `shared-protocol/src/blockchain/transaction.rs:213` | `self.data_cid.as_deref().unwrap()` inside `RevealPayload::validate_cid_fields()` | Invariant violation | Yes (`StateTransition::ApplyReveal` -> `CommitRevealState::apply_reveal`) | Replace `unwrap()` tuple assumptions with explicit destructuring and deterministic typed error code `SP_TX_INV_CID_TUPLE_BROKEN` | P0 |
| `shared-protocol/src/blockchain/transaction.rs:218` | `self.data_hash.as_deref().unwrap()` | Invariant violation | Yes | Same as above | P0 |
| `shared-protocol/src/blockchain/transaction.rs:223` | `self.storage_provider.as_deref().unwrap()` | Invariant violation | Yes | Same as above | P0 |
| `shared-protocol/src/consensus/proof_of_publishing.rs:349` | `block.hash.as_ref().unwrap()` in success log | Invariant violation | Yes (block production) | Replace with `if let Some(hash)` branch; on `None` return typed internal consensus error `SP_CONS_INV_BLOCK_HASH_MISSING` (no panic) | P1 |
| `native-chain/src/storage.rs:97` | `u64::from_le_bytes(bytes.try_into().unwrap())` for `META_WAL_SEQ` | Invariant violation | No (node recovery path, availability-critical) | Replace with checked conversion and `StorageError::CorruptMetaLength { key, expected, actual }` | P1 |
| `native-chain/src/storage.rs:126` | same pattern for `META_HEIGHT` | Invariant violation | No | Same as above | P1 |
| `native-chain/src/storage.rs:153` | same pattern for `META_LAST_CHECKPOINT` | Invariant violation | No | Same as above | P1 |
| `native-chain/src/storage.rs:182` | `u32::from_le_bytes(raw[..4].try_into().unwrap())` | Invariant violation | No | Guard on `raw.len() >= 4`; emit `StorageError::CorruptWalPrefix { sequence, actual_len }` | P1 |

### 1.2 Secondary callsites (runtime safety, lower consensus risk)

| File:Line | Current panic pattern | Category | Consensus-critical path? | Proposed replacement | Priority |
|---|---|---|---|---|---|
| `shared-protocol/src/governance/mod.rs:313` | `unreachable!("checked by quality-target guard")` | Invariant violation | No (proposal preview helper) | Replace with typed error branch `SP_GOV_INV_UNHANDLED_QUALITY_TARGET` | P2 |
| `shared-protocol/src/bridge/challenge.rs:56` | `self.active_windows.get(tx_id).unwrap()` after insert | Invariant violation | No (bridge safety state machine) | Return `Result` via `ok_or_else` with code `SP_BRIDGE_INV_WINDOW_MISSING_AFTER_INSERT` | P2 |
| `native-chain/src/bin/launch_api.rs:3503` | `.expect("valid multiaddr")` | Unrecoverable bug | No (startup path) | Convert to fallible parse path; disable P2P init with structured error `NC_NODE_BUG_INVALID_LISTEN_MULTIADDR` | P2 |
| `shared-protocol/src/common/constants.rs:6,9,12,15,18` | `Address::from_hex(...).unwrap()` static init | Unrecoverable bug | Indirectly yes (global constants) | Replace parsed string constants with byte-array `Address([...])` constants; eliminate runtime parse/panic | P2 |

### 1.3 Classification summary

- Protocol-invalid input panic callsites found in audited runtime paths: **none**.
- Observed panic-style risks are mostly:
  - guard-dependent `unwrap()` / `unreachable!()` invariant assumptions
  - startup/static initialization assumptions

## 2. Deterministic Typed Error Taxonomy (Implementation-Ready)

### 2.1 Core type shape

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorClass {
    ProtocolInvalid,
    InvariantViolation,
    UnrecoverableBug,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProtocolErrorCode {
    // Transition/apply protocol-invalid
    SpTxInvalidCidFields,
    SpTxInvalidDataHashFormat,
    SpCommitRevealUnknownCommit,
    SpGovernanceInvalidParameter,

    // Transition/apply invariants
    SpTxInvCidTupleBroken,
    SpConsInvBlockHashMissing,
    SpStorageInvMetaLength,
    SpStorageInvWalPrefixLength,
    SpBridgeInvWindowMissingAfterInsert,
    SpGovInvUnhandledQualityTarget,

    // Bugs
    SpBugInvalidStaticAddress,
    NcBugInvalidListenMultiaddr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: ProtocolErrorCode,
    pub class: ErrorClass,
    pub message: &'static str,          // stable, deterministic
    pub context: std::collections::BTreeMap<String, String>, // stable key ordering
}
```

### 2.2 Mapping strategy

| Internal source | Mapped code/class | Node outcome | Client/API outcome |
|---|---|---|---|
| Transition validation failure (`StateError::*` from malformed tx/payload) | `ProtocolInvalid` + specific `ProtocolErrorCode` | Reject transition/tx deterministically; continue node | `4xx` with `{error_code, message, request_id}` |
| Guarded-`unwrap` invariant failure in consensus path | `InvariantViolation` | Fail closed on current transition; do not panic; emit structured fault | `500` generic body + `error_code`; no internals leaked |
| Storage replay decode/length mismatch | `InvariantViolation` | Return `StorageError` deterministically; caller decides recover/fresh-start policy | Not directly exposed; if surfaced use generic `500` + code |
| Startup literal/config impossible state | `UnrecoverableBug` | Abort subsystem init gracefully where possible; avoid process panic | `503/500` with stable code only |

### 2.3 Determinism requirements

- Keep `ProtocolErrorCode` stable and append-only.
- Keep `message` static and non-localized in consensus paths.
- Use `BTreeMap` for context serialization stability.
- Never include unordered debug dumps (`{:?}` on hash maps) in consensus error text.
- Do not include timestamps/random IDs inside consensus error payloads.

## 3. Logging, Telemetry, and Redaction

Required fields on every emitted typed error:

- `error_code`
- `error_class`
- `component` (`state`, `consensus`, `storage`, `api`, `bridge`)
- `transition_type` (if applicable)
- `tx_id` (if present)
- `block_height` (if known)
- `consensus_path` (`true/false`)
- `state_root` (hex, if available)

Redaction rules:

- Do not log private keys, signatures, nonces, or full reveal payload bodies.
- Hash or truncate high-cardinality identifiers where feasible (`station_id`, arbitrary payload fields).
- For client responses, return stable `error_code` + sanitized message only; keep full context in internal logs.

## 4. Migration and Safety Notes

### 4.1 Consensus semantics

Planned change is **panic-to-typed-error conversion only**.  
No rule/threshold/window/math changes are required.

No-consensus-drift guardrails:

1. Preserve existing accept/reject outcomes for valid and invalid protocol inputs.
2. Replace only panic-style branches with deterministic error returns.
3. Keep current transition ordering and state-root recomputation unchanged.
4. Add regression tests asserting:
   - same transition results before/after for canonical fixtures
   - no panics under malformed payload fuzz cases

### 4.2 Native-chain note

`native-chain/src/legacy/*` panic patterns are imported only by `parity.rs` under `#[cfg(test)]`; they are not production runtime callsites.

## 5. Draft Follow-up Issue Packaging Options (Do Not Create Yet)

## Option A: Coarse (single implementation issue)

- Proposed title: `fix(shared-protocol+native-chain): replace panic-style transition/runtime failure paths with typed deterministic errors`
- Scope: all P0-P2 callsites + taxonomy + API error shape update.
- Risk: medium-high (large surface in one PR).
- Sequencing: one branch, one rollout.

## Option B: Two-phase (recommended)

- Proposed title 1: `fix(shared-protocol): remove panic-style paths in consensus transition/apply flow and introduce ProtocolErrorCode`
- Proposed title 2: `fix(native-chain): adopt typed protocol/storage errors in node recovery and API responses`
- Scope: phase 1 covers P0/P1 consensus-path items; phase 2 covers storage/API/startup/runtime safety.
- Risk: medium (controlled blast radius).
- Sequencing: merge title 1 first, then title 2 after compatibility checks.

## Option C: Fine-grained (three implementation issues)

- Proposed title 1: `fix(shared-protocol): harden ApplyReveal CID validation and remove guarded unwraps`
- Proposed title 2: `fix(shared-protocol): remove consensus block-hash unwrap and governance/bridge unreachable assumptions`
- Proposed title 3: `fix(native-chain): convert storage/startup panic-style assumptions to typed errors`
- Scope: smallest PRs, explicit ownership by subsystem.
- Risk: low per PR, higher coordination overhead.
- Sequencing: 1 -> 2 -> 3.

