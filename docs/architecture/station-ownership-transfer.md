# Station ownership transfer — owner consent (core4#3345)

**Status:** DRAFT design, 2026-09-27. **Owner decisions Q1–Q5 DECIDED 2026-09-27 (owner; the
recommended option on every question, §8).** Epic: wrfcoin/infra#1095 (station ownership & theft
protection). Sibling stories: #3346 (hardware commitment), #3347 (report-stolen freeze,
self-deactivation, `is_active` in rewards; design in
[`station-freeze-and-deactivation.md`](station-freeze-and-deactivation.md)).

All citations are at core4 `main` = `9ef92a235` (the deployed commit), re-checked 2026-09-27.

## 1. Problem

A station's owner can be replaced without the owner's consent.

- Consensus applies a `StationRegistration` through `StateTransition::ApplyStationRegistration`
  (`shared-protocol/src/blockchain/state.rs:4633-4638`) →
  `State::apply_station_registration` (`state.rs:1787-1821`) → `StationRegistry::upsert`
  (`shared-protocol/src/station_registry.rs:140-168`), which replaces the WHOLE record, including
  `owner_address`. The rejecting `StationRegistry::register` (`station_registry.rs:126-138`) is not
  on the consensus path. (The issue cites these lines as "native-chain state.rs"; the file is
  `shared-protocol/src/blockchain/state.rs`.)
- The authorization for a registration (`Transaction::verify_station_registration_authorization_with_domain`,
  `shared-protocol/src/blockchain/transaction.rs`, and `_at_height` right after it) is a governance
  signature over the record plus a possession proof by the NEW owner key over
  `wrfcoin_station_ownership_v1 ‖ chain ‖ station_id ‖ owner_address`
  (`station_ownership_message_with_domain`). The CURRENT owner never signs.
- From h3,000 on chain 888 (`STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT`; 11,000 before the 2026-10-23 owner ruling,
  `shared-protocol/src/consensus/proof_of_publishing.rs:431`) the reward payee is the registered owner
  (`reward_recipient_for_station`, `proof_of_publishing.rs:6264-6313`). So a governance-signed
  re-registration naming a new owner redirects the station's earnings.

**Evidence (production path, not inferred):** `shared-protocol/tests/station_owner_consent_3345.rs`
step 1 mines two governance-signed registrations of the same `station_id` with the real
`ProofOfPublishing::create_next_block`; after the second block the registry names the second owner.
The first owner signed nothing in that block.

Two further observations (not fixed here; tracked in §9):

- The possession message has **no nonce, height or expiry**. It proves "this key once agreed to own
  station X", and is reusable indefinitely.
- The governance registration signature has no nonce either. Whether a byte-identical, already-mined
  registration can be re-mined is **not verified** by this design (it depends on duplicate-txid
  handling below the mempool). If it can, a stale registration could revert a metadata change;
  owner-consent (§3.1) already stops it from reverting an owner change.

### Threat model

| Actor | Today | After this design |
|---|---|---|
| Holder of a governance key (compromise, insider, or ceremony mistake) | Can reassign any station and its rewards | Cannot reassign; can still correct metadata, re-classify provenance, or set `is_active` (see Q3) |
| Thief of the physical device | Cannot redirect earnings (Ambient account + owner key); can try a new `station_id` (#3346) | Unchanged here; #3346/#3347 |
| Thief of the owner's KEY | Can do anything the owner can | Can transfer the station → the lost/stolen KEY case is the recovery question (Q1) |

## 2. Goals and non-goals

Goals (from the issue's acceptance criteria):

1. Behind a new activation (not retroactive), a registration that changes the `owner_address` of an
   existing `station_id` is refused unless the change carries the CURRENT owner's signature.
2. Same-owner re-registrations (metadata corrections) keep working with the governance signature.
3. A dedicated owner-signed transfer with a replay-safe preimage (no floats, no bincode).
4. The owner can voluntarily transfer; governance alone cannot reassign.
5. The lost-owner-key path is an explicit owner decision (Q1), not a silent capability.
6. Pre-activation behaviour and every `state_root` below activation are byte-identical.

Non-goals: hardware identity (#3346); freeze/deactivation (#3347); UI.

## 3. Design

### 3.1 Rule R1 — governance cannot change the owner (IMPLEMENTED in the draft PR)

New consensus feature `ConsensusFeature::StationOwnerConsent` (`"station_owner_consent"`),
**unscheduled** (`shared-protocol/src/crypto/chain_binding.rs`; paired in
`protocol_support::KNOWN_FEATURES`).

- One shared constructor, `Transaction::station_registration_transition(payload, block_height)`,
  is now used by BOTH extractors: the PoP producer arm (`proof_of_publishing.rs`,
  `TransactionType::StationRegistration`) and the P2P/WAL applier
  (`native-chain/src/bin/launch_api/transition_extractor.rs`). Below activation it returns the legacy
  `ApplyStationRegistration { station }`; from activation it returns the new, APPENDED variant
  `ApplyStationRegistrationWithOwnerConsent { station, block_height }`. One function, so producer and
  applier cannot pick different variants at the same height.
- Applying the new variant: a first registration, or one naming the same owner, is exactly
  `apply_station_registration`. One that would change the owner is an **inert, deterministic no-op**
  (logged). Not an error: a failing transition fails the whole block and stalls the chain, which is
  the reason `upsert` exists (#2537).
- "Same owner" = byte-identical string, or both parse (after one `0x`/`wrf` prefix) to the same
  20 raw bytes (`state::same_station_owner`), so re-spelling an address is not a transfer.
- The new variant is refused with `InvalidTransition` below activation, so it cannot be smuggled into
  pre-activation state.

R1 needs **no new State field**: it reads the existing record.

Policy layer (follow-up, not consensus): once R1 is active, the mempool/HTTP admission path
(`native-chain/src/bin/launch_api/routes/station.rs`) should reject an owner-changing registration
with a clear error instead of letting it be mined as a no-op.

### 3.2 The owner-signed transfer (stateless half IMPLEMENTED in the draft; application needs the ledger)

Implemented in `shared-protocol/src/blockchain/station_owner.rs`: `StationOwnerTransferPayload`,
`verify_stateless_with_domain` (both key bindings, the owner signature, the MANDATORY new-owner
countersignature — ruling Q4 — and the block-height validity window),
`Transaction::parse_station_owner_transfer`, and `transferred_station_record` (ruling Q5: only
`owner_address` changes). Tested in `shared-protocol/tests/station_owner_transfer_recovery_3345.rs`.
No extractor emits a transfer yet: the nonce check and "not frozen" need the owner ledger (§3.3).

**Carrier.** Reuse `TransactionType::StationRegistration` with a new `tx_kind` discriminant
`"station_owner_transfer_v1"`. No new `TransactionType` means no wire-layout change
(`blockchain/wire_layout_fixtures.rs`), no new verify-cost row, no new dispatch intent. An old binary's
`parse_station_registration` requires `tx_kind == "station_registration_v1"`, so it parses a transfer
as `None` and emits no transition (inert) — which is only safe because activation requires every
producer to run the new binary (the #3277 hard gate), and a new binary emits nothing for this kind
below activation either.

**Payload** (`StationOwnerTransferPayload`, JSON in `tx.data`):

| Field | Type | Meaning |
|---|---|---|
| `tx_kind` | string | `"station_owner_transfer_v1"` |
| `station_id` | string | station being transferred |
| `from_owner_address` | string | the owner registered NOW (pinned into the preimage) |
| `from_owner_pubkey` | hex ed25519 | must derive (`PublicKey::to_address`, the #2742 rule) to `from_owner_address` |
| `to_owner_address` | string | new owner |
| `to_owner_pubkey` | hex ed25519 | must derive to `to_owner_address` |
| `transfer_nonce` | u64 | must equal the station's next owner-action nonce in state (§3.3) |
| `expires_at_height` | u64 | last height at which the transfer may be applied |
| `from_owner_signature` | hex | over the preimage with `wrfcoin_station_owner_transfer_v1` |
| `to_owner_signature` | hex | over the SAME body with `wrfcoin_station_owner_transfer_accept_v1` (Q4) |

No governance signature: the whole point is that the owner, not governance, moves the station. The
station keeps `provenance_class`, reputation, location, hardware and `is_active` (**Q5 DECIDED**).
`to_owner_signature` is **mandatory (Q4 DECIDED)**.

**Preimage** (implemented as `transaction::station_owner_transfer_message_with_domain`, byte-pinned by
the test, blake3 `53de3d80…3e032` for the test vector):

```
domain                      "wrfcoin_station_owner_transfer_v1"   (or "…_accept_v1")
u32le len ‖ chain_id        (e.g. "777")
u32le len ‖ network_id      (e.g. "testnet")
u32le len ‖ station_id
u32le len ‖ from_owner_address
u32le len ‖ to_owner_address
u32le len ‖ to_owner_pubkey (hex)
u64le transfer_nonce
u64le expires_at_height
```

Every field is explicitly length-prefixed or fixed-width: **no floats, no bincode** (bincode is a
dev-dependency of shared-protocol and positional, #3163). Chain and network are bound through the same
`push_chain_domain` every other station preimage uses.

**Replay safety:**

- `transfer_nonce` is consumed on success, so a transfer can apply at most once.
- `from_owner_address` is pinned, so after A→B the old signature is dead even if the nonce were
  somehow reused; after A→B→A the nonce has moved on.
- `expires_at_height` bounds a signed-but-unsubmitted transfer. Consensus also requires
  `expires_at_height ≤ block_height + MAX_TRANSFER_VALIDITY` (proposed 10,000 blocks, ≈4 days at the
  observed ~1.7 blocks/min) so a signature cannot be parked forever.
- Distinct domain tags for the sender and the acceptor: neither signature can stand in for the other,
  or for any other signed object (tags must be added to the
  `signed_type_inventory::AUTHORIZATION_MESSAGE_SURFACES` list with money-path golden cases when the
  payload struct lands; see §7).

**Application** (`StateTransition::ApplyStationOwnerTransfer { payload, block_height }`, appended):
stateless checks (parse, both key bindings, both signatures, expiry window) in the shared extractor
constructor; stateful checks in `apply` — station exists, registered owner `same_station_owner` as
`from_owner_address`, nonce matches, station not frozen (#3347), `to != from`. Any failure is an
inert no-op, never a block-failing error. Success: `owner_address := to_owner_address`,
`owner_nonce += 1`. `station_counts` is untouched (location and active flag do not change).

### 3.3 State: one per-station owner ledger, shared with #3347

```rust
pub struct StationOwnerLedgerEntry {
    pub owner_nonce: u64,                 // next nonce any owner-signed action/transfer must carry
    pub frozen: Option<StationFreeze>,    // #3347 report-stolen
    pub owner_deactivated: bool,          // #3347 self-deactivation (distinct from governance is_active)
    pub pending_recovery: Option<PendingStationOwnerRecovery>, // #3345 Q1 time-locked recovery (§3.4)
}
// State: station_owner_ledger: BTreeMap<String /*station_id*/, StationOwnerLedgerEntry>
```

- The nonce is **per station**, not per owner, so it survives transfers and orders transfers and
  #3347 actions against each other.
- Additive and empty until the first owner action. It is folded into `state_root` **only when
  non-empty** (the #3277 `feature_schedule_extensions` pattern), so every root is byte-identical until
  an owner action is mined, which cannot happen before activation.
- It is a new persisted `State` field ⇒ **checkpoint format bump** + frozen `legacy/state_vN.rs` +
  `MigrationVNToVN+1` (default empty) + regenerated golden (never hand-edited), per the #3069 pattern;
  and a `state_root_material` field ⇒ the #1682 producer/applier/WAL parity pins in
  `native-chain/src/bin/launch_api/p2p_events.rs` with a populated-lifecycle parity test BEFORE the pin
  is updated. It should land ONCE, in a dedicated state PR covering both #3345 and #3347, not twice.

### 3.4 Time-locked owner-key recovery (Q1 DECIDED; stateless half IMPLEMENTED)

The ONE mechanism for a lost owner key (#3345 Q1) and for lifting a report-stolen freeze when the key
is gone (#3347 Q1). Governance can never move a station immediately (Q3).

| Step | Carrier (`tx_kind` on `TransactionType::StationRegistration`) | Signed by | Effect when applied (owner ledger) |
|---|---|---|---|
| File | `station_owner_recovery_file_v1` (`StationOwnerRecoveryFilingPayload`) | an allowlisted governance key over `wrfcoin_station_owner_recovery_file_v1`, AND the new owner over `…_recovery_accept_v1` (same body) | Requires: registered owner `same_station_owner` as `from_owner_address`, `recovery_nonce` == ledger nonce, no recovery already pending. Stores `PendingStationOwnerRecovery { from, to, recovery_nonce, filed_at_height }`. Owner unchanged. |
| Cancel | `station_owner_recovery_cancel_v1` (`StationOwnerRecoveryCancelPayload`) | the CURRENT owner, over `wrfcoin_station_owner_recovery_cancel_v1 ‖ chain ‖ station_id ‖ owner_address ‖ u64le recovery_nonce` | Only while `height < filed_at_height + STATION_OWNER_RECOVERY_NOTICE_BLOCKS` (`is_cancelled_by`). Clears the pending recovery; consumes the nonce. |
| Execute | `station_owner_recovery_execute_v1` (`StationOwnerRecoveryExecutePayload {station_id, recovery_nonce}`) | nobody (permissionless: the filing is already authorized) | Only from `filed_at_height + 34,000` (`is_executable_at`). `owner_address := to` via `transferred_station_record` (provenance and reputation kept, Q5); clears any #3347 freeze and owner-deactivation; consumes the nonce. |

- `STATION_OWNER_RECOVERY_NOTICE_BLOCKS = 34,000` (≈14 days at ~1.7 blocks/min). Block heights only:
  no wall-clock, no floats. `executable_at_height` saturates.
- A pending recovery is visible on chain from the filing block; the explorer/app should surface it
  (UI story) so the owner sees the notice.
- Any owner action or transfer that consumes the ledger nonce first voids a pending recovery (its
  `recovery_nonce` no longer matches), so a live owner can also defeat it by acting.
- The recovery's new owner also countersigns (`…_recovery_accept_v1`), for the Q4 reason (no typo'd
  destination). This extends Q4 to recovery; flagged for owner confirmation in the PR report.
- Implemented (stateless): payloads, preimages, governance allowlist + both signatures,
  `PendingStationOwnerRecovery::{filed, executable_at_height, is_executable_at, is_cancelled_by}`.
  Stateful half (owner-ledger state PR): filing stores the pending recovery WITHOUT consuming the
  nonce; cancel and execute consume it; any nonce-consuming owner step (transfer, owner action,
  cancel) also clears a pending recovery, so a stale filing never blocks a new one.

## 4. Activation

- **Q2 DECIDED (2026-09-27, owner): `station_owner_consent` activates in the 2nd #3277 schedule
  extension, after the transfer and the owner ledger are merged.** Nothing is scheduled by this PR.
- R1 and the transfer ship under `station_owner_consent`, **unscheduled**: on the live 777/testnet
  chain (protocol v1, no schedule) it is inactive at every height and never triggers the #3093 halt.
- Activation is by a #3277 live feature-schedule extension (`ApplyFeatureScheduleExtension`,
  `shared-protocol/src/governance/schedule_extension.rs`): owner rulings D3 (H ≥ tip + 3000) and
  D2 (MIN_RUNUP 2000) apply, and the hard gate that all three producers (N5, workstation, Mango) run the
  binary before H. Ruling D7 fixes the FIRST extension as #3131 + #3270 only, so this goes in a later
  extension (Q2).
- An old binary facing a schedule that names `station_owner_consent` reports the #3093 upgrade and
  halts cleanly (asserted in the test).

## 5. Determinism

Below activation nothing changes: the shared constructor emits the legacy variant, no new State field
exists in the R1 draft, and the owner-consent variant is refused. Tests to run (all green on the draft
head except where noted — see the PR body for counts):

- `cargo test -p test_server --test determinism_differential` (full-sequence root parity against the
  committed `expected_roots.json`, restart parity, mutation control) and
  `--test feature_schedule_parity`.
- `cargo test -p wrfcoin-shared-protocol` (lib + every integration target, including
  `station_registration_production_path_2537`, `signed_type_inventory`, and the new
  `station_owner_consent_3345`).
- `cargo test -p wrfcoin-native-chain` and `cargo test -p wrfcoin-native-chain --bin launch_api
  --features p2p` (the #1682 parity pins; the transition extractor lives there).
- When the ledger (§3.3) lands: the full native-chain target set including
  `--test storage_migration_2727` and the checkpoint golden, per the state-layout rule.
- Before scheduling on the live chain: an x86 + arm64 determinism run at the release commit (the
  72a993552 procedure) with the feature scheduled in a test genesis above and below H.

## 6. Interaction with #3346 (hardware commitment) and #3347 (freeze)

- The hardware commitment is a property of the STATION. A transfer moves the station with its
  commitment (the buyer gets the device). The transfer preimage does not include the commitment:
  `station_id` plus the nonce already identifies exactly one record.
- A **frozen** station (#3347 report-stolen) cannot be transferred: freeze exists precisely so the
  thief who also holds the key cannot move it (and #3346 stops the device being registered under a new
  `station_id`).
- #3346's duplicate-hardware rule compares commitments across ACTIVE registrations of DIFFERENT
  stations; a transfer does not create a second registration, so it never trips that rule.
- After R1, governance can still change a station's metadata, provenance and `is_active` without the
  owner. Changing the hardware commitment on an existing station is effectively a device swap and
  should also require the owner (recommended rule for #3346, owner decision Q3).

## 7. Implementation plan (draft PR vs follow-ups)

| Piece | Draft PR | Follow-up |
|---|---|---|
| `station_owner_consent` feature, unscheduled | ✅ | |
| R1: shared constructor + appended guarded variant + `same_station_owner` (Q3) | ✅ | |
| Transfer preimage builder + byte-pinned test | ✅ | |
| Characterisation of today's reassignment (production PoP path) | ✅ | |
| `StationOwnerTransferPayload` + stateless verify incl. mandatory countersignature (Q4) + validity window | ✅ | |
| `transferred_station_record` keeps provenance/reputation (Q5) | ✅ | |
| Recovery payloads, preimages, notice-window/cancel state machine (Q1) | ✅ | |
| Owner ledger State field (incl. `pending_recovery`) + checkpoint bump + migration + parity pins | ✅ owner-ledger state PR (shared with #3347) | |
| `AUTHORIZATION_MESSAGE_SURFACES` entries + money-path goldens for the 5 new tags | ✅ owner-ledger state PR | |
| `ApplyStationOwnerTransfer` / recovery file-cancel-execute transitions + both extractors | ✅ owner-ledger state PR (`Transaction::station_owner_ledger_transition`, one shared constructor) | |
| Mempool/HTTP admission policy for owner-ledger kinds | | follow-up (policy, not consensus) |
| CLI (`wrfcoin station transfer`), frontend/mobile flows, explorer notice of pending recovery | | UI stories under infra#1095 |

## 8. Owner decisions — DECIDED 2026-09-27 (owner chose the recommended option on all five)

| Q | Decision | Implemented where |
|---|---|---|
| Q1 lost owner key | **DECIDED (2026-09-27, owner): time-locked governance recovery.** Governance files a recovery naming the new owner; it executes only after a public notice window of ~14 days (~34,000 blocks); the current owner key can cancel it before then. | §3.4. Stateless half in `station_owner.rs` (`STATION_OWNER_RECOVERY_NOTICE_BLOCKS`, filing/cancel payloads, `PendingStationOwnerRecovery`). Storage/execution: ledger follow-up. |
| Q2 activation | **DECIDED (2026-09-27, owner): the 2nd #3277 schedule extension, after the transfer and the owner ledger are merged.** | §4. Feature stays unscheduled in this PR. |
| Q3 governance override | **DECIDED (2026-09-27, owner): no owner override except the Q1 path.** Governance keeps metadata correction, provenance and `is_active`. | R1 (§3.1), implemented. |
| Q4 countersignature | **DECIDED (2026-09-27, owner): the new owner must countersign.** | `StationOwnerTransferPayload::verify_stateless_with_domain`, implemented. |
| Q5 provenance/reputation | **DECIDED (2026-09-27, owner): a transfer keeps Real provenance and reputation.** | `transferred_station_record`, implemented; applied by the transfer PR. |

The options the owner did not choose (recorded for history): Q1 (b) no recovery, (c) guardian recovery
via `social_recovery` #2172, (d) immediate governance reassignment; Q2 (b) R1 alone ASAP, (c) next
genesis cut; Q3 (b) quorum emergency override, (c) owner consent for everything; Q4 (b) sender
signature only; Q5 (b) reset to Unclassified.

## 9. Open items

- Verify whether a byte-identical, already-mined `StationRegistration` can be re-mined (duplicate-txid
  handling below the mempool). If it can, add a per-station registration nonce too.
- The owner possession proof has no nonce/expiry (§1); a `…_ownership_v2` bound to the ledger nonce
  would make a fresh owner signature necessary for every registration, closing metadata replay.
