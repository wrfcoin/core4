# Bridge Safety Runbook

Operational guide for WRFCoin bridge safety controls including auto-halt, exposure
caps, challenge windows, and anomaly detection.

> **Source-audit notice (2026-07-21):** These are library/control-plane behaviors,
> not proof that a value-moving bridge is protected. The current native
> `BridgeLock`/`BridgeUnlock` execution path is a no-op, and no transfer path calls
> the safety manager's exposure or completion methods. See
> [`security/CROSS_CHAIN_TRUST_MODELS.md`](security/CROSS_CHAIN_TRUST_MODELS.md) for
> the source-derived trust classification and current deployment boundary.

## Safety Architecture Overview

The current repository contains the following bridge-related surfaces:

| Surface | Module | Actual scope |
|-------|--------|-------|
| Cross-chain messages | `shared-protocol/src/cross_chain/` | Unsigned message conversion and an in-memory relay queue; no receiving execution |
| Safety library | `shared-protocol/src/bridge/` | Exposure, halt, challenge, alert, and stuck-state primitives; not wired to a transfer |
| Native control plane | `native-chain/src/bin/launch_api/routes/bridge.rs` | Role-gated, in-memory safety administration; no transfer endpoint |
| Historical runbook surfaces | `src/cross_chain/bridge.rs`, `src/layer2/bridge.rs`, `backend/routes/bridges.ts` | Not present in the current core4 tree |

`BridgeSafetyManager::check_bridge_operation` checks halt state before reserving
exposure when a caller invokes it. No current deposit, withdrawal, confirmation,
finalization, mint, or release path invokes it.

## Auto-Halt Triggers

The bridge will automatically halt under these conditions:

### 1. Per-Chain Exposure Cap Breach
- **Default**: 5,000,000 WRF per chain
- **Behavior**: When an outbound bridge transaction would push a single chain's locked
  exposure over the cap, the transaction is rejected and the bridge halts.
- **Indicator**: `halt_reason` contains "exposure limit for chain"

### 2. Global Exposure Cap Breach
- **Default (cross-chain)**: 25,000,000 WRF total across all chains
- **Default (L2)**: 100,000,000,000 units
- **Behavior**: When total locked exposure across all chains exceeds the global cap, the
  bridge halts and the triggering transaction is rejected.
- **Indicator**: `halt_reason` contains "global exposure" or "exposure cap exceeded"

### 3. Anomaly Threshold Exceeded
- **Default**: 5 failures in a 3600-second (1 hour) sliding window
- **Behavior**: Each processing or verification failure increments the anomaly counter.
  When the threshold is reached, the bridge halts immediately.
- **Window reset**: If the window duration expires without reaching threshold, the counter
  resets automatically.
- **Indicator**: `halt_reason` contains "anomaly threshold exceeded"

### 4. Challenge Quorum (Cross-Chain Only)
- **Condition**: When the number of unique validator challenges reaches the configured
  `min_signatures` quorum on a single transaction.
- **Behavior**: The bridge halts because validator consensus disputes the transaction.
- **Indicator**: `halt_reason` contains "challenge quorum reached"

## Monitoring Safety Status

### Cross-Chain Bridge

The `get_safety_status()` method returns:

```json
{
  "halted": false,
  "halt_reason": null,
  "halted_at": null,
  "max_exposure_per_chain": 5000000,
  "max_global_exposure": 25000000,
  "global_exposure": 1234567,
  "exposure_by_chain": {
    "ethereum": 500000,
    "polygon": 734567
  },
  "anomaly_failures_in_window": 2,
  "anomaly_failure_threshold": 5,
  "challenge_window_secs": 1800
}
```

### Layer 2 Bridge

The `get_safety_state()` method returns:

```json
{
  "halted": false,
  "halt_reason": null,
  "halted_at": null,
  "global_exposure": 0,
  "global_exposure_cap": 100000000000,
  "anomaly_failures_in_window": 0,
  "anomaly_failure_threshold": 5,
  "anomaly_window_start": 1739700000,
  "anomaly_window_duration_secs": 3600,
  "challenge_window_secs": 604800
}
```

### Backend (TypeScript)

```bash
# Check bridge safety state
curl http://localhost:3000/bridge/safety-status
```

## Resuming After Halt

### Prerequisites
1. Identify and resolve the root cause of the halt
2. Obtain governance or admin authorization
3. Document the resolution in the resume reason

### Cross-Chain Bridge Resume

```rust
bridge_manager.resume_bridge("governance vote #42: root cause resolved, validator 3 key rotated");
```

**Effects:**
- Clears `halted`, `halt_reason`, and `halted_at`
- Resets anomaly failure counter to 0
- Resets anomaly window start to current time
- Appends a `Resume` entry to the safety audit log

### Layer 2 Bridge Resume

```rust
layer2_bridge.resume_from_halt("admin authorization: exposure released after finalization").await?;
```

**Effects:**
- Clears safety halt state
- Resets anomaly counters
- Sets bridge status back to `Operational`

### Backend Resume

```bash
# POST to admin resume endpoint (requires admin auth)
curl -X POST http://localhost:3000/bridge/admin/resume \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"reason": "governance approved resume after investigation"}'
```

## Challenge Window Lifecycle

### Cross-Chain Bridge
1. Transaction enters `AwaitingFinality` state
2. Validators submit finality attestations (unique per validator, idempotent)
3. When quorum is reached (>= `required_confirmations`), status becomes `ChallengeWindowOpen`
4. `challenge_window_end` is set to `current_time + challenge_window_secs`
5. During the window, validators can submit challenges
6. After the window expires, `finalize_transaction` can complete the transaction
7. If challenged, `resolve_challenge` must be called before finalization

### Layer 2 Bridge
1. Deposit/withdrawal enters `Pending` state with `challenge_window_end` set
2. Oracle signatures are collected (BFT 2/3+1 threshold)
3. `finalize_deposit` checks:
   - Bridge is not halted
   - Sufficient oracle signatures
   - Challenge window has expired (`now >= challenge_window_end`)
   - Transaction is not in `Challenged` state
4. On finalization, exposure is released

## Configuration Defaults

| Parameter | Cross-Chain | Layer 2 |
|-----------|-------------|---------|
| Max exposure per chain | 5,000,000 | N/A (global only) |
| Max global exposure | 25,000,000 | 100,000,000,000 |
| Anomaly threshold | 5 failures | 5 failures |
| Anomaly window | 3,600 sec (1 hour) | 3,600 sec (1 hour) |
| Challenge window | 1,800 sec (30 min) | 604,800 sec (7 days) |
| Min validator signatures | Configurable | 2/3+1 of oracles |

### Overriding Defaults

```rust
// Cross-chain bridge
let manager = BridgeManager::new(blockchain, mempool, validators, min_sigs)
    .with_finality_policy(3, 7200) // 3 confirmations, 2 hour challenge
    .with_safety_limits(10_000_000) // 10M per chain
    .with_global_exposure_limit(50_000_000) // 50M global
    .with_anomaly_policy(10, 1800); // 10 failures in 30 min window
```

## Audit Trail

All halt and resume events are recorded in the safety audit log with:
- **Timestamp**: Unix seconds when the event occurred
- **Event type**: `AutoHalt`, `Resume`, `AnomalyWindowReset`, `AnomalyFailure`, `ExposureCapBreach`
- **Reason**: Human-readable context string

Access the audit log:
```rust
let log = bridge_manager.safety_audit_log();
for entry in log {
    println!("[{}] {:?}: {}", entry.timestamp, entry.event, entry.reason);
}
```

## Incident Response Checklist

1. **Identify halt trigger** - Check `halt_reason` and `halted_at` timestamp
2. **Check audit log** - Review safety audit entries leading to the halt
3. **Assess exposure** - Check `global_exposure` and per-chain exposure levels
4. **Investigate anomalies** - If anomaly-triggered, review the failure contexts
5. **Check pending transactions** - Identify transactions in non-terminal states
6. **Resolve root cause** - Fix the underlying issue before resuming
7. **Resume with documentation** - Call `resume_bridge(reason)` with a clear explanation
8. **Monitor post-resume** - Watch anomaly counters and exposure levels after resuming
