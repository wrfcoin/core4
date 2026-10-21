# Stake / Unstake submission: implementation map and slice order

Refs #3430, #3241. Companion to `docs/decisions/stake-submission-round11.md` (merged in #3431), which owns the
funds design and the executed preflight. This file maps the design's consensus prerequisite to code sites on core4
`1b908b384` and sets the slice order. Docs only: no behavior, schedule, genesis or live-schedule file changes, and
nothing here is scheduled, activated or deployed. Line numbers are at that commit and will drift.

## Gate-OFF contract (applies to every slice)

- New consensus feature `stake_submission_nonce`: NEW, UNSCHEDULED, absent from every genesis string and from
  `docs/deployment/live-feature-schedule/*`. With it off, the B3.1 behavior is the behavior: mempool admission refuses
  Stake/Unstake at the `local_admission_disabled` guard (`shared-protocol/src/mempool/pool.rs:376`, tests `:3685`, `:3711`; the
  `:567` area is the separate stake-authorization check), historical blocks still
  verify and apply exactly as before (`proof_of_publishing.rs:8401` authorization check; extractor Stake/Unstake arms
  `native-chain/src/bin/launch_api/transition_extractor.rs:155-190`).
- Proof required in the PR that first touches an applier or producer path: a full native-chain test set on the merged
  tree, plus state_root and block-hash equality between main and the gated-off build for a fixture chain that contains
  signed Stake and Unstake blocks. Any root difference with the gate off is a P1.

## Code sites the gated slice touches

| Concern | Site (core4 `1b908b384`) | Today | Gated change |
| --- | --- | --- | --- |
| Feature registry | `shared-protocol/src/crypto/chain_binding.rs:652` (`ConsensusFeature`), `:797` (`ALL`), `name()`, `schedulable()` (arms are exhaustive on purpose) | 14 variants | Append `StakeSubmissionNonce` at the tail; `name() = "stake_submission_nonce"`; `schedulable()` stays `false` until a B4 release decides otherwise. Also `crypto/protocol_support.rs` `KNOWN_FEATURES` pairing (a binary must list what it implements, else the schedule halts cleanly, #3093). |
| Stake/Unstake apply | `shared-protocol/src/blockchain/state.rs:3632` `LockToStake`, `:3648` `ReleaseToSpendable` | Balance split only; no nonce, no timestamp | Unchanged. The gated path is a new transition, not an edit. |
| Protocol dispatch | `state.rs:7461` inventory arm lists Stake/Unstake as no-op | no-op | Route Stake/Unstake through a new shared handler when the feature is active at the inclusion height. The inventory comment forbids `_ => Ok(())`; the new arm must be named in the ratchet. |
| Nonce primitive | `state.rs:7085-7120` `check_and_reserve_nonce` / `commit_nonce` | Used by Transfer and governance | Reuse verbatim so Stake, Unstake and Transfer share the account nonce. Overflow at `u64::MAX` returns `NonceOverflow` before any mutation. |
| New transition | `state.rs:9425-9770` `StateTransition` enum | tail variant is the last positional discriminant | Append `ProcessStakeTransaction { tx, block_height, block_timestamp }` at the TAIL. No reordering; positional discriminants of every deployed variant stay. No new state section, checkpoint format or transaction type. |
| Producer path | `state.rs:2329` `governance_transitions_for_block` (match at `:2340` lists Transfer, governance, privacy, RewardClaim; Stake/Unstake fall to `_ => {}`) | producer omits staking | When active, simulate and emit `ProcessStakeTransaction` for Stake/Unstake with the same drop-loudly handling as `:2358-2375`. Reached from `proof_of_publishing.rs:3164` `create_next_block`. |
| Peer applier | `transition_extractor.rs:155-190` | emits `LockToStake` / `ReleaseToSpendable` | When active, emit `ProcessStakeTransaction` and suppress the legacy split transitions for the same tx. Same function serves WAL replay. |
| Authorization | `transaction.rs:2824` `parse_stake_auth_payload`, `:3413` `verify_stake_authorization_with_view` | account-bound ed25519 over `account\|amount\|action\|nonce`; state-aware view at inclusion height | Reuse. The handler checks authorization first, then `tx.nonce == account.nonce`, then amount, then applies and commits nonce+1 atomically. |
| Mempool replay key | `pool.rs:1302` key `action:account:amount:nonce`, removed on confirmation `pool.rs:723`, `:975`, `:1032` | memory-only; confirmation re-opens the key | With the feature active the persistent nonce is the replay barrier; the key stays only as pending-pool dedup. Admission when active must also reject a pending same-nonce operation across Stake/Unstake/Transfer. |
| Gossip entry | `native-chain/src/bin/launch_api/p2p_events.rs:1924` | inbound gossip reaches pool admission | No change; B3.1 refusal stays while the feature is off. |
| Enqueue helper | `native-chain/src/bin/launch_api/helpers.rs:1121` `enqueue_protocol_transaction_with_nonce` | builds the protocol transaction with fee 0 (per the design) | No change in the prerequisite slices. |

## Slice order (one slice per PR, at most two awaiting review)

1. **This map** (docs only). Records the cooldown sequencing below.
2. **Gate-OFF baseline (test only).** A fixture chain with signed Stake and Unstake blocks applied through the
   producer-less peer path; record state roots and block hashes as goldens. Passing on unmodified main proves the
   goldens describe today's behavior; every later slice must keep them identical with the gate off.
3. **Feature registration, OFF.** Append `StakeSubmissionNonce` to `ConsensusFeature`, `ALL`, `name()`,
   `schedulable() = false`, `KNOWN_FEATURES`. Behavior-neutral; the slice 2 goldens and the full native-chain set prove it.
4. **Gated consensus prerequisite.** `ProcessStakeTransaction` tail variant, shared handler, producer emission,
   extractor emission and legacy-split suppression, nonce consumption, replay refusal across storage reload.
   RED first: the two executed preflight failures in the design become passing tests with the gate on and stay
   byte-identical with it off. Controls: forged authorization, wrong action/amount/domain/nonce, overflow, insufficient
   funds (each changes nothing), same-nonce contention with Transfer, producer==peer==WAL roots, a caught replay mutation.
   Signed-type rules (no bincode in a preimage, golden vectors, inventory ratchet) apply only if a signed type is
   added; none is proposed.
5. Node routes, backend relay, CLI: the design's later PRs, unchanged.

## Unstake cooldown (~1,080 blocks, owner ruling 10-02): decided sequencing

The design (`stake-submission-round11.md`, "Existing funds and authorization") preserves today's rule: release is allowed
up to all locked stake with no cooldown and no new state. A cooldown has to remember when each unstake becomes
spendable, which needs an account field and a checkpoint format bump that the design excludes from the nonce prerequisite.

Decision (LEAD, agreed on this PR): the nonce prerequisite lands first, in slices 2-4 above, with no cooldown. The
cooldown is a separate, later gated consensus feature `unstake_cooldown`, OFF by default and OFF on chain 777, built
only after the nonce prerequisite merges. It implements the 10-02 ruling in its own slice, with its own gate-OFF
byte-equality proof. Its checkpoint format number is taken at rebase time (open #3433 holds format 11). The nonce
prerequisite stays checkpoint-neutral so that its gate-OFF proof remains a byte-equality proof of existing state.
The owner-facing line for this is an FYI, not a blocker.

Overlap noted for ordering: open PRs #3433, #3435, #3422 and #3356 also edit `chain_binding.rs`, `state.rs`,
`transaction.rs` or the extractor. Rebase after each of them merges; keep hunks tail-appended to limit conflicts.
