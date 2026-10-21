# Session 4 — Bridge Safety Hardening (#205)

## Agent Prompt

You are working on GitHub issue #205 (P0, high-priority) for the WRFCoin blockchain platform: **Bridge safety hardening: auto-halt, exposure caps, challenge windows**.

### Context

WRFCoin has cross-chain bridge infrastructure spanning Rust core (`src/cross_chain/bridge.rs`) and a modular Layer 2 bridge system (`blockchain-core/layer2/bridge/`). Initial risk controls (auto-halt + basic caps) were already added in `src/cross_chain/bridge.rs` during a prior security audit. This issue completes the remaining safety controls.

### What needs to be done

1. **Explicit challenge windows** for inbound/outbound bridge claims — users and validators must have a defined time window to dispute bridge operations before they finalize
2. **Per-chain and global exposure caps** with governance-configured thresholds — limit how much value can be bridged per chain and globally, with thresholds adjustable via governance
3. **Anomaly auto-halt wired to operator/governance unhalt flows** — when anomalies are detected, the bridge halts automatically; operators or governance must explicitly unhalt
4. **Stuck-state and partial-finality handling** — detect and handle bridge operations that get stuck or have partial finality on the source/destination chain
5. **Emergency runbooks and alerting hooks** — structured alerting for bridge incidents with hooks for external monitoring

### Key files to read first

Start by reading these files to understand the existing implementation:

- `src/cross_chain/bridge.rs` — Existing auto-halt + caps implementation (start here)
- `blockchain-core/layer2/bridge/mod.rs` — Main bridge module
- `blockchain-core/layer2/bridge/facade.rs` — Public unified API
- `blockchain-core/layer2/bridge/engines/deposit_engine.rs` — Deposit processing
- `blockchain-core/layer2/bridge/engines/withdrawal_engine.rs` — Withdrawal processing
- `blockchain-core/layer2/bridge/engines/validation_engine.rs` — Validation logic
- `blockchain-core/layer2/bridge/monitoring/health_checker.rs` — Health monitoring
- `blockchain-core/layer2/bridge/monitoring/metrics_collector.rs` — Metrics
- `blockchain-core/layer2/bridge/events/event_monitor.rs` — Event monitoring

Also read the gap analysis for context:
- `reports/wrfcoin-rust-blockchain-feature-gap-analysis-2026-02-15.md` (search for "bridge" sections)

### Architecture constraints

- This is Rust code in the `blockchain-core` crate (workspace member)
- Follow existing error handling patterns (`thiserror` errors, `Result<T, E>`)
- Use existing monitoring/metrics infrastructure (`blockchain-core/monitoring/`)
- The bridge has a modular architecture with facade pattern — new features should integrate through the existing facade
- Security-critical: use constant-time operations where relevant, validate all inputs
- Add tests for new safety logic (especially challenge windows and exposure cap enforcement)

### Definition of done

- Challenge window mechanism with configurable duration per chain
- Exposure caps enforced at both per-chain and global levels with governance override
- Auto-halt triggers with a clear unhalt flow (operator action or governance vote)
- Stuck-state detection with timeout-based recovery or escalation
- Alerting hooks that emit structured events for external monitoring systems
- Unit tests covering edge cases (cap exceeded, challenge disputed, stuck timeout)
