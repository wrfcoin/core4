# Elsa Mempool/Storage/State Audit — 2026-06-20

Branch: `elsa/mempool-storage-audit-r6`  
Scope: `shared-protocol/src/mempool/`, `native-chain/src/storage.rs`, `shared-protocol/src/blockchain/state.rs`  
Verified against: `origin/main` @ `46ee05d63`

---

## Findings

| # | File:Line | Severity | Status | Summary |
|---|-----------|----------|--------|---------|
| 1 | `shared-protocol/src/mempool/pool.rs:275–294` | HIGH (correctness) | **REAL — FIXED in this branch** | `double_spends` stale entry leak on multi-input partial rejection |
| 2 | `native-chain/src/storage.rs:566–608` | MEDIUM (perf) | **REAL — NOT fixed** | `load_wal_transitions_range` full WAL table scan (needs format change) |
| 3 | `shared-protocol/src/blockchain/state.rs:3373,3378,3398` | HIGH (determinism) | **REAL — NOT fixed** (PR #2006 open) | `nft.updated_at` uses wall-clock `SystemTime::now()` → state_root divergence |

---

## Detail

### Finding 1 — `double_spends` stale entry leak (FIXED)

**Location:** `shared-protocol/src/mempool/pool.rs:275–294`

**Bug:** `add_transaction_with_signing_keys` processed inputs one at a time, writing each
outpoint to `self.double_spends` before completing all checks. If input at index N passed
but input at index N+k caused a `"Double spend detected"` early return, inputs 0..N had
already been written to `double_spends` with the rejected transaction's `tx_id` — a tx_id
that was never inserted into `self.transactions`. Those orphan entries permanently blocked
any subsequent transaction from spending those outpoints (`"Double spend detected"` would
fire on a clean outpoint), with no recovery path until mempool restart.

**Attack surface:** A crafted multi-input transaction with a clean early input and a
known-double-spend late input poisons outpoints belonging to honest UTXOs without the
attacker's tx being admitted to the mempool.

**Fix (this commit):** Two-pass input processing — pre-flight loop checks ALL inputs
(and detects intra-tx duplicate inputs) with no state mutation; a second pass applies
the `double_spends` inserts only after all checks pass.

**Verification:** No format change. No behavior change for single-input transactions
(the common case). Existing tests (`adversarial_double_spend_same_input`,
`purge_older_than_secs_clears_double_spend_tracking`, etc.) cover the invariants.
Cargo check passes.

---

### Finding 2 — `load_wal_transitions_range` O(n) full WAL table scan (NOT FIXED — format change needed)

**Location:** `native-chain/src/storage.rs:566–608`

**Bug:** The WAL table is keyed by monotonic `wal_sequence` (u64), not block height.
`load_wal_transitions_range(start_height, end_height)` must iterate every WAL entry,
decrypt it, verify CRC, and deserialize the entry header just to filter by
`block_height`. For a node with `k` blocks between checkpoints (default 100,
configurable), each reorg reconstruction costs O(k × entries_per_block) I/O and
decode work. Called at `native-chain/src/bin/launch_api/p2p_events.rs:870` on every
fork-switch event.

**Impact:** Reorg latency scales linearly with WAL depth. Under the default 100-block
checkpoint interval, this is bounded but still visible; with `WRFCOIN_CHECKPOINT_INTERVAL`
set large (disaster-recovery nodes), it becomes a multi-second stall per reorg.

**Fix sketch:** Add block_height as a composite WAL key component, e.g. key =
`(block_height << 32) | sequence_within_block`, enabling redb range scans directly
by height. Alternatively, maintain a height→(min_seq, max_seq) side-table in META.
Both are breaking WAL format changes requiring a migration path. **Not fixed here.**

---

### Finding 3 — `nft.updated_at` wall-clock time → consensus state_root divergence (NOT FIXED)

**Location:** `shared-protocol/src/blockchain/state.rs:3373, 3378, 3398`

**Bug:** Three call sites in the NFT apply path call `current_unix_timestamp()`:
```rust
nft.updated_at = current_unix_timestamp();
```
`current_unix_timestamp()` is:
```rust
fn current_unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
```
`nft.updated_at` is included verbatim in `state_root_material` (line 1908):
```rust
push_u64(&mut material, nft.updated_at);
```
Two nodes processing the same block at wall-clock second boundary get different
`updated_at` values → different `state_root` → consensus fork for any block that
updates an NFT.

**Open PR:** #2006 (`fix/1968-nft-updated-at-determinism`) targets this; NOT merged
to origin/main as of this audit.

**Fix sketch:** Replace `current_unix_timestamp()` at those three sites with the
consensus block timestamp (passed as an argument to the apply path) or block height,
both of which are identical on all nodes for a given block.

---

## Cargo check result

`cargo check -p shared-protocol` — passes (see commit for output reference).
