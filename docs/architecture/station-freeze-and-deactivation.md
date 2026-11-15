# Station report-stolen freeze, owner (de)activation, and `is_active` in rewards (core4#3347)

**Status:** DRAFT design, 2026-09-27. **Owner decisions Q1–Q5 DECIDED 2026-09-27 (owner; the
recommended option on every question, §8).** Epic: wrfcoin/infra#1095. Siblings: #3345 (owner-consent
transfer; design in `station-ownership-transfer.md` on its own draft PR), #3346 (hardware
commitment). All citations at core4 `main` = `9ef92a235` (the deployed commit), re-checked 2026-09-27.

## 1. Problem

An owner has no way to react to theft or to retire a station.

- No stolen/lost/freeze/suspend or owner self-deactivation transaction exists.
  `StationRegistry::deactivate` (`shared-protocol/src/station_registry.rs:177-188`) and
  `State::deactivate_weather_station` (`shared-protocol/src/blockchain/state.rs:1826`) are called only
  from a unit test; no transaction reaches them. The only way `is_active` changes today is a
  governance-signed re-registration carrying `is_active: false` (the field is in the governance
  preimage, `station_registration_authorization_message_with_domain`).
- **Verified (not inferred): an inactive Real station is paid.** On the production path (a
  governance-signed `StationRegistration` with `is_active = false`, then commit, reveal and 2-of-2
  oracle quorum, each mined by `ProofOfPublishing::create_next_block`), the reveal mints a reward:
  `rewards_minted_count = 1`, `real_paid = earned_balance_total = 10,710,000,000` base units
  (`shared-protocol/tests/station_active_reward_gate_3347.rs`, step 1).
  Why: the reward decision (`proof_of_publishing.rs`, contribution lane ~4775-4830 and forecast lane
  ~5127-5170) gates on `provenance_class` (captured at commit time from the registry,
  `state.rs:4221-4227`) and on the #2743 owner resolution, never on `is_active`. The only production
  reads of `is_active` near rewards are `State::station_coordinates`/`active_station_cell`
  (`state.rs:1744-1753`, `1851-1858`) — an inactive station simply falls back to the neutral
  data-desert multiplier (`unwrap_or(100)`) — and the ring-signature reward-claim membership check
  (`state.rs` ~7712). `proof_of_publishing.rs` itself reads `is_active` only in test fixtures, as the
  survey suspected.

Why a freeze matters even though the payee is owner-bound since h3,000 on chain 888 (`STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT`): a stolen device still reports
under the owner's `station_id`, so (1) its data — possibly from a new location — keeps earning and
keeps exposing the OWNER to data-quality slashing once `station_owner_slash_identity` (#3214) is
active, and (2) without #3346 the thief can register the device again under a new `station_id`.

## 2. Goals

1. `is_active` is honoured by rewards (activation-gated, not retroactive) — **implemented** in the
   draft PR.
2. An owner-signed **report stolen/lost** action: rewards stop at once, the station cannot be
   transferred, and (with #3346) its hardware commitment is frozen so nobody can register it.
3. Only an owner-signed **unfreeze** (or the owner-decided recovery path) lifts it.
4. Owner-signed **self-deactivation / reactivation**.
5. Replay-safe preimages; every root byte-identical below activation.

## 3. Design

### 3.1 R2 — rewards honour activity (IMPLEMENTED in the draft PR)

New consensus feature `ConsensusFeature::StationActiveRewardGate` (`"station_active_reward_gate"`),
**unscheduled**. `ProofOfPublishing::station_reward_refused_inactive(state, height, station_id)` is
`false` at every height until the feature activates; from activation it is `true` for a station that
is missing from the registry or has `is_active == false`. It is applied in three places:

1. contribution lane — after the #2460 provenance gate, before the #2743 owner resolution: a
   zero-amount `ContributionRewardRecord` (address `0x00…`, same shape as the #2743 refusal);
2. forecast lane — the same;
3. the #2464 deferred-settlement scan — a reward PLANNED while the station was active is not settled
   while it is inactive, and is settled by the first block in which the station is active again
   (**Q3 DECIDED: held, then released**). The record is never dropped; the scan simply skips it while
   the predicate is true. Evidence: step 3b of `tests/station_active_reward_gate_3347.rs` (reveal
   without quorum → governance `is_active=false` → quorum arrives: NOT settled → `is_active=true`:
   settled), RED with the deferred-scan gate removed.

Below activation the reward path is unchanged byte for byte (no new metadata key, no new State),
so `determinism_differential` and every existing reward test are untouched. R2 does not depend on the
owner ledger and can activate on its own (Q2).

When the owner ledger (§3.3) lands, the predicate becomes "effectively active":
`registry.is_active && ledger.frozen.is_none() && !ledger.owner_deactivated`.

### 3.2 Owner actions (SPECIFIED; preimage pinned in the draft, rest not yet implemented)

**Carrier.** `TransactionType::StationRegistration` with `tx_kind = "station_owner_action_v1"` (same
reasoning as the #3345 transfer: no new `TransactionType`, no wire-layout change; an old binary parses
it as `None` and emits nothing).

**Payload** `StationOwnerActionPayload`:

| Field | Type | Meaning |
|---|---|---|
| `tx_kind` | string | `"station_owner_action_v1"` |
| `station_id` | string | |
| `owner_address` | string | the owner registered NOW (pinned) |
| `owner_pubkey` | hex ed25519 | must derive (`PublicKey::to_address`) to `owner_address` |
| `action` | enum | `report_stolen` \| `unfreeze` \| `deactivate` \| `reactivate` (`StationOwnerActionKind`) |
| `action_nonce` | u64 | must equal the station's next owner nonce (SHARED with #3345 transfers) |
| `expires_at_height` | u64 | ≤ block_height + MAX_ACTION_VALIDITY (proposed 10,000) |
| `owner_signature` | hex | over the preimage below |

**Preimage** (implemented as `transaction::station_owner_action_message_with_domain`, pinned
field-by-field by the test):

```
domain                     "wrfcoin_station_owner_action_v1"
u32le len ‖ chain_id
u32le len ‖ network_id
u32le len ‖ station_id
u32le len ‖ owner_address
u8        action discriminant   (report_stolen=1, unfreeze=2, deactivate=3, reactivate=4; frozen, append-only)
u64le     action_nonce
u64le     expires_at_height
```

No floats, no bincode. The discriminant is signed, so a `deactivate` signature can never be replayed
as `unfreeze`. The nonce is the same per-station counter the #3345 transfer consumes, so a transfer
and an action cannot be replayed or reordered against each other; `owner_address` is pinned, so
a signature dies with a transfer.

**Effects** (`StateTransition::ApplyStationOwnerAction { payload, block_height }`, appended; failures
are inert no-ops, never block-failing errors):

| Action | Precondition | Effect |
|---|---|---|
| `report_stolen` | not frozen | `frozen = Some { since_height }`; station counts as inactive (rewards stop from the next block; `station_counts` is NOT touched — it stays a pure function of the governance registry so a later re-registration's counter bookkeeping cannot double-decrement); cannot be transferred (#3345); hardware commitment frozen (#3346); its reveals are excluded from data-quality slashing/reputation (**Q5 DECIDED**, §3.4) |
| `unfreeze` | frozen | `frozen = None`; activity back to `is_active && !owner_deactivated`. Owner-signed ONLY: no governance path lifts a freeze except the time-locked recovery (**Q1/Q4 DECIDED**) |
| `deactivate` | not frozen, not owner-deactivated | `owner_deactivated = true` |
| `reactivate` | owner-deactivated, not frozen | `owner_deactivated = false` |

All four consume the nonce.

**Why not just flip `is_active`?** Because a governance re-registration is an `upsert` of the whole
record and carries its own `is_active`: a routine metadata correction (or a replayed stale
registration, §9 of the #3345 doc) would silently UNFREEZE a stolen station. Freeze and owner
deactivation therefore live in the owner ledger, which registrations never touch. `is_active` stays
governance's data-quality lever.

**Admission policy (not consensus).** Once active, the mempool/HTTP layer should refuse commits and
reveals from a frozen station (they would earn nothing) and surface `frozen` in
`GET /transactions/station/registrations/<id>` for the explorer and the owner's app.

### 3.3 State

Exactly the `StationOwnerLedgerEntry { owner_nonce, frozen, owner_deactivated }` map specified in the
#3345 design (§3.3), added ONCE for both stories: additive, conditionally folded into `state_root`
(only when non-empty), a checkpoint format bump with a migration defaulting to empty, and the #1682
parity pins with a populated-lifecycle producer/applier/WAL parity test before the pin moves.

### 3.4 Freeze suspends penalties (Q5 DECIDED; not yet implemented — needs the freeze state)

From the owner-actions activation, while `ledger.frozen.is_some()` a reveal of that station neither
earns nor is penalised. Hook points at this branch's head (`shared-protocol/src/blockchain/state.rs`,
`StateTransition::ApplyReveal`):

- ZK-receipt slashing, `verify_receipt_with_slashing` → `apply_oracle_slashing_trigger` (~4240-4254);
- data-quality slashing, `check_weather_data_quality` → #3214 owner identity →
  `apply_oracle_slashing_trigger` (~4299-4338).

Rule: when the station is frozen at `block_height`, neither trigger is built and the reveal is an
inert no-op for penalties (and R2 already refuses its reward). Station **reputation**
(`StationRegistry::update_reputation_at`, `station_registry.rs:199`) has no production caller today,
so the reputation half of Q5 is a rule for when a station-reputation writer lands: it must skip a
frozen station. Test (actions PR): a frozen station's physically-impossible reveal debits nothing
and changes no reputation; the same reveal unfrozen is slashed (anti-vacuity).

### 3.5 Lifting a freeze without the key (Q1 DECIDED)

The #3345 time-locked recovery (`station-ownership-transfer.md` §3.4) is the only non-owner path:
executing a recovery sets the new owner AND clears `frozen` and `owner_deactivated`. Governance has no
other way to lift a freeze (**Q4 DECIDED**). The stateless half (payloads, 34,000-block notice window,
owner cancel) is implemented on the #3345 draft branch in `shared-protocol/src/blockchain/station_owner.rs`.

## 4. Activation

- **Q2 DECIDED (2026-09-27, owner): `station_active_reward_gate` activates alone, in the next #3277
  extension after the first; the read-only pre-check of live `is_active:false` registrations (#3352)
  comes first.** Nothing is scheduled by this PR.
- `station_active_reward_gate` (R2): unscheduled; independent of the ledger. Only stations with
  `is_active = false` are affected. **Pre-scheduling check:** list live registrations with
  `is_active: false` (read-only) — each one stops earning at H.
- Owner actions: a second unscheduled feature, `station_owner_actions`, scheduled together with
  #3345's `station_owner_consent` (they share the ledger and the nonce).
- Both go in a #3277 live schedule extension after the first one (ruling D7 fixes the first as
  #3131 + #3270), with D3 (H ≥ tip + 3000), D2 (runup 2000) and the three-producer hard gate. An old
  binary facing a schedule that names either feature halts cleanly (#3093; asserted for R2 in the test).

## 5. Determinism

Below activation `station_reward_refused_inactive` is constantly `false`, so no reward record,
metadata key or root changes. Run (results in the PR body):

- `cargo test -p test_server --test determinism_differential --test feature_schedule_parity`
- `cargo test -p wrfcoin-shared-protocol` (lib + all integration targets; includes
  `station_registration_production_path_2537` and the new `station_active_reward_gate_3347`)
- `cargo test -p wrfcoin-native-chain` and `--bin launch_api --features p2p` (the P2P applier replays
  the producer's reward records; parity pins)
- when the ledger lands: the full native-chain state-layout set (`storage_migration_2727`, checkpoint
  golden) and an x86 + arm64 determinism run before scheduling.

## 6. Interaction with #3345 and #3346

- #3345: a frozen station cannot be transferred; freeze, unfreeze and transfer share one nonce, so the
  first one mined wins and the others die. If the thief ALSO holds the owner key, they can transfer
  before the owner freezes — that is the lost/stolen-KEY case, handled by #3345 Q1's recovery path, not
  by the freeze.
- #3346: while frozen, the station's hardware commitment is reserved: no other registration (any
  `station_id`, any owner) may carry it. Unfreeze releases the reservation back to this station only.
- R2 makes governance's existing `is_active: false` meaningful for money for the first time; that is
  intended, but see the pre-scheduling check in §4.

## 7. Implementation plan

| Piece | Draft PR | Follow-up |
|---|---|---|
| Characterisation: inactive Real station paid today (production path) | ✅ | |
| `station_active_reward_gate` + R2 in contribution, forecast and deferred lanes | ✅ | |
| Q3 hold-then-release of planned rewards, tested on the production path | ✅ | |
| Owner-action preimage + `StationOwnerActionKind` discriminants, pinned | ✅ | |
| Owner ledger State field (shared with #3345, incl. the recovery) | ✅ owner-ledger state PR | |
| `StationOwnerActionPayload`, auth-surface registration + golden, `ApplyStationOwnerAction`, both extractors | ✅ owner-ledger state PR (`station_owner_actions`, unscheduled) | admission policy: follow-up |
| Freeze suspends slashing/reputation (Q5, §3.4) | ✅ owner-ledger state PR: ZK-receipt, #3214 data-quality and collusion-reputation hooks | |
| Freeze / owner-deactivation feed the R2 predicate (held, then released) | ✅ owner-ledger state PR | |
| Recovery lifts a freeze (Q1, §3.5) | ✅ owner-ledger state PR | |
| #3346 commitment reservation | | #3346 |
| "Report my station stolen" in frontend/mobile | | UI stories under infra#1095 |

## 8. Owner decisions — DECIDED 2026-09-27 (owner chose the recommended option on all five)

| Q | Decision | Implemented where |
|---|---|---|
| Q1 lifting a freeze when the key is lost/stolen | **DECIDED (2026-09-27, owner): the same time-locked, owner-cancellable recovery as #3345 Q1** (one mechanism). | §3.5; stateless half on the #3345 branch; execution needs the ledger. |
| Q2 activation of R2 | **DECIDED (2026-09-27, owner): alone, in the next #3277 extension after the first; the read-only pre-check of live `is_active:false` registrations comes first.** | §4. R2 is independent of the ledger; unscheduled here. |
| Q3 planned-but-unsettled rewards | **DECIDED (2026-09-27, owner): held while inactive, released on unfreeze/reactivation.** | Deferred-scan gate in `proof_of_publishing.rs`; tested (step 3b). |
| Q4 governance and freezes | **DECIDED (2026-09-27, owner): governance cannot lift an owner's freeze; only an owner-signed unfreeze or the Q1 recovery.** | By construction: freeze lives in the owner ledger, which registrations never touch (§3.2). Needs the ledger. |
| Q5 penalties while frozen | **DECIDED (2026-09-27, owner): a freeze suspends both rewards and penalties (#3214 slashing and reputation).** | §3.4 spec with hook points; needs `frozen`. |

Options not chosen (history): Q1 (b) nobody, (c) immediate governance unfreeze; Q2 (b) bundled with
#3345, (c) next genesis cut; Q3 (b) forfeited, (c) paid anyway; Q4 (b) governance with a notice
window; Q5 (b) owner stays accountable.
