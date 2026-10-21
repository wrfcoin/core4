# Consensus Audit — Dan r6 — 2026-06-20

Branch: `dan/consensus-audit-r6` (off `origin/main`)
Scope: `consensus-engines/` + `native-chain/src/consensus/`
Excluded (already fixed): PR #2002 (DAG/Raft/HotStuff DoS bounds), PR #1984 (phantom-finality cert DoS), PR #2005 (execute_reorg fork points), PR #1989 (WAL CRC fail-closed)

---

## Findings

| # | File:line (origin/main) | Severity | Verdict | Title |
|---|------------------------|----------|---------|-------|
| 1 | `consensus-engines/src/security/manager.rs:1034`<br>`consensus-engines/src/security/coordinator.rs:478` | **HIGH** | REAL | Unbounded leader-election candidate list — O(N) async lock acquisitions before any auth check (DoS) |
| 2 | `consensus-engines/src/algorithms/hotstuff/mod.rs:397,657` | **HIGH** | REAL | Unauthenticated votes enter equivocation-evidence store; Vec grows without bound (DoS + memory leak) |
| 3 | `consensus-engines/src/algorithms/raft.rs:802` | **MEDIUM** | REAL | `handle_install_snapshot` accepts unbounded `data: Vec<u8>` — no size guard (OOM DoS) |

---

## Detail

### Finding 1 — Leader-election candidate list DoS
**File:** `consensus-engines/src/security/manager.rs:1034` (`coordinate_secure_leader_election`) and `consensus-engines/src/security/coordinator.rs:478` (`secure_leader_election`)
**Severity:** HIGH
**Verdict:** REAL on `origin/main`; NOT covered by any merged PR (PR #2002 fixed DAG/Raft/HotStuff append paths, not the security-manager leader election).
**Root cause:** Both functions iterate over a caller-supplied `candidate_nodes: &[String]` with no length check.  Each iteration acquires the `authorized_keys` async read-lock (and in `coordinator.rs` also `attack_prevention`), so a message carrying N=1 000 000 candidates forces N lock acquisitions before any term or auth validation runs.
**Fix sketch:** Enforce `candidate_nodes.len() <= config.max_election_candidates` at function entry; default cap = 1 000.
**Status:** Fix committed in this branch (carried from `fix/consensus-audit-slice3`).

---

### Finding 2 — HotStuff equivocation-evidence store: sig-verify ordering + unbounded growth
**File:** `consensus-engines/src/algorithms/hotstuff/mod.rs`
- Ordering bug: line 397 (`check_equivocation` call) vs line 401 (`verify_vote_signature` call)
- Growth bug: line 657 (`voter_votes.push(vote.clone())`)

**Severity:** HIGH
**Verdict:** REAL on `origin/main`; not addressed by any merged PR.

**Root cause (ordering):** `process_vote` calls `check_equivocation` *before* `verify_vote_signature`.  `check_equivocation` pushes the vote into the `equivocation_evidence: Arc<RwLock<FxHashMap<NodeId, Vec<HotStuffVote>>>>` *before* the cryptographic check runs.  A peer therefore writes an arbitrary number of entries with invalid/missing signatures: each fabricated vote uses a distinct `(voter_id, view_number)` pair so it passes the double-vote check, gets committed to the store, and only *then* fails the signature check — leaving the entry in memory permanently.

**Root cause (unbounded growth):** Even for legitimate validators the store accumulates every (view, phase) vote with no pruning.  Over a long-running node the Vec grows to O(views × 3) entries per validator.

**Exploitability:** Any peer reachable by the network message path can trigger this.  A flood of N messages with distinct `view_number` values costs O(N) heap and O(N²) CPU (linear scan per incoming vote across all prior evidence for that voter_id).

**Fix:** Swap the call order so signature verification runs first.  Add `MAX_EVIDENCE_PER_VOTER = 3 * 1024` cap; emit `warn!` on cap-hit rather than silently growing.  No wire-format or protocol-behavior change.
**Status:** Fixed and committed in this branch (`388dd9ec5`).

---

### Finding 3 — Raft InstallSnapshot accepts unbounded snapshot data (OOM DoS)
**File:** `consensus-engines/src/algorithms/raft.rs:802` (`handle_install_snapshot`)
**Severity:** MEDIUM
**Verdict:** REAL on `origin/main`; not addressed by any merged PR.  PR #2002 added `max_append_entries` guard to `handle_append_entries` (line 701) but the sibling `handle_install_snapshot` handler at line 802 has no equivalent.

**Root cause:** The `RaftMessage::InstallSnapshot { data, .. }` variant carries an arbitrary `Vec<u8>` payload.  `handle_install_snapshot` applies it directly after a single term check, with no size bound:
```rust
// raft.rs:817 — no size guard before this
let snapshot = RaftSnapshot { data, ... };
*self.snapshot.write().unwrap() = Some(snapshot);
```
`RaftConfig` has `snapshot_threshold: u64` (line 149) which controls *when* to compact the log, not the *maximum size* of a received snapshot.

**Exploitability:** Requires a message from a peer with `term >= current_term` (i.e., a current or future leader, or a peer that can forge/replay a higher-term message).  A single crafted `InstallSnapshot` with a multi-GiB `data` vec will OOM the follower before any application-level validation.

**Fix sketch:** Add `max_snapshot_bytes: usize` to `RaftConfig` (default 256 MiB), enforce at the top of `handle_install_snapshot` before touching any state.  This is a pure internal guard; no wire-format change.
**FLAG:** scoped & low-compat — safe to fix inline.

---

## Already-confirmed non-issues reviewed

- `BftFinalityGadget::validate_certificate` — has correct DoS guard (`cert.votes.len() > validators.len()`) and replays all votes through `submit_vote_verified` before advancing finality. **Clean.**
- `CheckpointManager::create_checkpoint` (probabilistic finality path) — does not deduplicate `validator_address` in the signature slice, allowing a single-signer quorum bypass, but `CheckpointManager` has no callers outside tests and the `FinalityGadget` wrapper on `origin/main`; the live BFT path goes through `BftFinalityGadget`. **Latent / test-only risk; not prioritised.**
- `hard_fork.rs` `SignalingTracker::signaling_validators` — bounded by the actual validator set; no unbounded growth. **Clean.**
- `active_transitions` in `SecureConsensusCoordinator` — properly removed on both success and failure paths. **Clean.**
