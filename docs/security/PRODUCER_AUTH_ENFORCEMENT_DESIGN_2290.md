# Block-Producer Authentication — Enforcement Design (#2290, Option B) — rev 2

**Owner:** Bob (verify side + authorized-set state/resolver) · **Sign side:** Cindy ·
**Genesis seeding + key ceremony:** Alice · **Base:** `main` @ `e3b396e48`
**Status:** design rev 2, addressing Dan's DESIGN-CHANGES review of `dce3e254`. **The verify gate is
NOT wired — consensus-breaking, held for owner sign-off. Two items (§R1, §R2) carry recommendations
for the owner's call.**

## Owner decisions (Round 106)
- **PARAM-1 = governance-managed, on-chain authorized-producer set.**
- **PARAM-2 = enforce-at-genesis** — every block at height ≥ 1 must be signed by a currently
  authorized producer. **There is no genesis *block*:** height 0 is the empty-chain *state*, and the
  first block is height 1. The seed is written into genesis **state**, not carried by a block (see §4
  for the *correct* trust boundary).

## Owner decisions (Round 108) — §R1 + §R2 APPROVED, member shape LOCKED
- **§R1 = DEDICATED `ProducerSetChange` type/domain** — reuse #2227's *pattern* (canonical hash,
  `DomainSeparated`, full-replacement change with `epoch`/`effective_height`) but a **distinct
  privilege object**, NOT `ValidatorSetChange`. Finality committee and PoP producer-authority stay
  independently governed.
- **§R2 = STABLE `payout_address`** on the member, decoupled from the signing key. Authority check:
  authenticate via `producer_pubkey` → resolve the member with that key → require
  `coinbase_recipient == member.payout_address`.
- **LOCKED member shape:** `ProducerMember { producer_id: String, payout_address: String,
  public_key: [u8; 32] }`. `payout_address` is the coinbase target (NOT a signing-key-derived
  address). `public_key` is the rotatable signer; `producer_id` is the stable operator identity.

> **Sequencing:** implementation (the type, `State` field, root-version bump, shared transition, gate
> wiring, genesis hook, e2e RED→GREEN) is **HELD until Dan's re-review of rev 2 (`241ffeee`) clears
> the 7 fixes**. This doc finalizes the design; code lands on `APPROVES-DESIGN`. Nothing merges
> without owner sign-off.

---

## Revisions from Dan's review

### §1 (was blocking #1) — Acceptance boundary must cover gossip AND IBD

`pop_adapter::validate_block` is the **gossip** boundary only. **IBD is a separate path**:
`p2p/src/sync.rs::validate_block` (checks hash/height/parent/time) → `chain.add_block` directly,
never touching the consensus adapter. Wiring only the adapter leaves IBD accepting unsigned/non-member
blocks. Both paths do sink to `Blockchain::add_block`, but `add_block` also runs for the node's own
produced block and for deterministic genesis init, and lacks chain-identity/version context.

**Design:** one shared gate function
```rust
fn authorize_block_producer(block: &Block, state: &State, identity: &ChainIdentity, version: SigningProtocolVersion) -> Result<(), ProducerAuthorityError>
```
called from **both untrusted acceptance validators** — `pop_adapter::validate_block` (gossip) and
`sync.rs::validate_block` (IBD) — each passing the **same `chain.state` snapshot, chain identity, and
signing version** (a stated determinism contract; a divergence between the two would itself fork).
The node's own block is created + `add_block`ed locally (trusted, and signed), so it is not gated
there; peers gate it on receipt. Genesis never reaches either validator (§4). **Tests are required
for BOTH consumers** (§7). A follow-up may unify the two validators behind a single
`accept_external_block` to remove the drift risk structurally; for now the shared function + a parity
test between the two call sites is the contract.

### §R1 (owner-decision #2) — Committee coupling → **APPROVED: dedicated producer-set type**

Reusing `ValidatorSetChange` couples two distinct privileges: the **stake-weighted BFT-finality
committee** (`validator_id`, `stake_weight`, `public_key`; #2227/#2256) and **PoP producer-authority**
(who may create testnet blocks). The resolver only wants keys and discards `validator_id`/`stake_weight`
— evidence the semantics differ. Coupling means every finality-set update silently becomes a
producer-auth update and vice versa: an unintended privilege-escalation surface, and the finality
gadget is not even driven from this proposed history today. Field-shape similarity ≠ privilege
equivalence.

**Recommendation: use a DEDICATED `ProducerSetChange` type + its own domain tag + registry row +
its own `State` history**, reusing #2227's *pattern* (canonical `validator_set_hash`-style hash,
`DomainSeparated` envelope, full-replacement change with `epoch`/`effective_height`) but not its
privilege object. Member schema `ProducerMember { producer_id: String, payout_address: String,
public_key: [u8;32] }` (see §R2). This keeps finality and production independently governed, minimal
blast radius, and lets the PoP set carry a payout field the finality member lacks.

*If the owner instead wants a single committee to govern both*: make it explicit, preserve **and
validate** the full member semantics (stake_weight included), share ONE canonical transition consumed
by both the finality gadget and producer-auth, and actually drive finality from this state history.
That is strictly more work and more coupling; recommended only if the testnet genuinely wants one
committee for both roles. Given the testnet is PoP with no active finality committee driving
production, **dedicated type is recommended.**

### §R2 (was owner-decision #5) — Payout identity → RECOMMENDATION: **stable `payout_address`, decoupled from the signing key**

`producer_pubkey.to_address() == coinbase_recipient` ties the payout to the **signing** key, so an
HSM key rotation (#2241) would redirect a producer's rewards — operationally wrong. The member record
already can separate stable identity from the rotatable signer.

**Recommendation: the authorized member owns an explicit stable `payout_address`.** The PoP authority
check, **after the coinbase structure is established**, becomes:
1. authenticate the block via `producer_pubkey` (the signer) — `verify_block_producer_on`;
2. resolve the **full** authorized member whose `public_key == producer_pubkey` at this height;
3. require `coinbase_recipient == member.payout_address`.

This decouples signing-key rotation (change `public_key` on the member) from payout (stable
`payout_address`). The alternative — signer-derived payouts — is simpler but forces payout↔signer
coupling and needs a mining-API/`/blocks/mine` + key-rotation contract change to re-point payouts on
rotation. **Stable `payout_address` recommended.** (Both §R1 and §R2 point to the same dedicated
`ProducerMember { producer_id, payout_address, public_key }`.)

### §3 (was blocking #3) — At-height causality: `effective_height > containing_block_height`

`effective_height <= H` is safe **only if the change was committed before H**. A change carried *in*
block H declaring `effective_height = H` forks: pre-state verification of H resolves the old set,
replay over accumulated history resolves the new set for H.

**Rules (enforced by the set-change STATE TRANSITION, not the resolver):**
- `effective_height > containing_block_height` (activation strictly after the carrying block; normally
  H+1 or later).
- `epoch` strictly increasing vs the last applied change; `effective_height` strictly increasing.
  Reject duplicate/non-monotonic epoch or effective_height → `StateError`.
- Member set non-empty; no duplicate `producer_id`/`public_key`; every `public_key` a well-formed
  ed25519 point. **These FAIL the state transition** — no silent `filter_map`/drop. The resolver then
  reads only validated history and maps members directly (a malformed key in state is an invariant
  violation, not a silently-skipped row).
- Resolution at height H reads state as of blocks `< H` (H is not yet applied during its own
  verification), picks the change with the greatest `effective_height <= H`.

**Tests:** a change committed at K activates strictly after K (K-carried change with
`effective_height = K` is rejected); resolver returns old set at K, at the activation height − 1, and
the new set at the activation height (H-1 / H / H+1 coverage); duplicate/non-monotonic/empty/malformed
changes fail the transition.

### §4 (was blocking #4) — Genesis trust boundary: pinned init, NOT the untrusted height field

The height-0 exemption keyed on attacker-controlled `block.height == 0` is removed. The generic
chained verifier does not establish expected height, and `Blockchain::add_block` permits an arbitrary
first block, so an attacker could present a height-0 block to bypass the gate.

**Design:** genesis is established by **deterministic, pinned initialization** (both nodes construct
the identical genesis locally; identity is the pinned `genesis_hash`). Genesis never flows through the
untrusted acceptance validators: gossip `apply_p2p_block` handles received blocks, and IBD
`sync.rs::validate_block` uses `expected_height = chain.height + 1 >= 1`, so the first *accepted* block
is height 1. **The gate therefore enforces on every block it sees, with no height exemption.** A
forged height-0 (or any) block presented to the gate authenticates → unsigned/non-member → REJECTED.
If a defensive exemption is ever needed at the gate, it must be bound to `block.hash == pinned
genesis_hash`, never to the height field. **Test:** a forged height-0 block is rejected by the gate,
not exempted.

### §6 (was blocking #6) — State-root: version bump + one shared transition + extended parity proof

Adding the producer-set history is a consensus-state change, not merely additive:
- **Root-version BUMP** on the fresh-genesis cut — the new history is a new `state_root_material`
  section, so the root version must increment (the material is hand-encoded in
  `state.rs::state_root_material` / `state_root_material_field_names`).
- **Canonical encoding + order** for the history section (sorted, length-prefixed) so producer,
  applier, and WAL replay hash identical bytes.
- **ONE shared `StateTransition`** (e.g. `ApplyProducerSetChange`) applied on **all** paths —
  producer block-assembly, P2P applier, and WAL replay — via the shared finalization sink, so the set
  mutates identically everywhere (this is the producer/applier parity discipline from #1776/#2381).
- **Extend the producer/applier parity proof across a set-change**: a scenario that applies an
  `ApplyProducerSetChange` and asserts byte-identical `state_root` pre-images producer vs applier,
  **plus a one-leg mutation** (mutate the set on one side only) that MUST make the parity test
  diverge — proving the new section is actually in the root and load-bearing (the RED-on-revert
  discipline from #2381).

### §7 (was blocking #7) — RED spec through the real boundaries with seeded state

The current e2e tests call `verify_block_chained` directly (below the future gate, no chain state),
never seed an authorized set, and the non-member case is confounded (coinbase built from one key's
`Display`, signed by another → can fail on payout, not membership). Rewrite the spec to run through
**each real acceptance boundary** with **seeded producer-set state**:
- **signed seeded member → ACCEPTED** (both gossip and IBD boundaries);
- **unsigned block paying the member's payout_address → REJECTED** (authentication);
- **signed stranger paying ITS OWN valid payout identity → REJECTED solely for non-membership** (the
  coinbase/payout is valid, so the only reason to reject is set-membership — removes the confound);
- **IBD equivalent** of each via `sync.rs::validate_block`.
These fail today (no enforcement) and turn green when the gate wires — a valid load-bearing spec.
Because they require the seeded `State.producer_set_history` field, they land **with** the state
schema + wiring (gated on sign-off); until then they are marked `#[ignore]` with the exact assertions
so Dan can review the spec shape now.

---

## State schema for Alice (LOCKED — Round 108)

Dedicated type (§R1) with stable payout (§R2). The types to implement (on Dan approval):
```rust
// shared-protocol/src/consensus/producer_set.rs (new — dedicated privilege object)
pub struct ProducerMember { pub producer_id: String, pub payout_address: String, pub public_key: [u8; 32] }
pub struct ProducerSetChange { pub epoch: u64, pub effective_height: u64, pub members: Vec<ProducerMember> }
// + canonical hash (ProducerSetChange::producer_set_hash), DomainSeparated impl, domain-registry row
//   — modeled on validator_set.rs / #2227 but a SEPARATE domain tag.
```
```rust
// shared-protocol/src/blockchain/state.rs, in `struct State`
/// Authorized block-producer set-change history (#2290). Ordered full-replacement
/// changes; the authorised set at height H is the change with the greatest
/// effective_height <= H (all committed in blocks < H). Consensus state — in
/// state_root_material (root-version bumped, §6). Seeded at genesis; mutated only
/// by a validated ApplyProducerSetChange transition (§3).
pub producer_set_history: Vec<ProducerSetChange>,
```
**Genesis seed (Alice) — full `ProducerMember`:**
```rust
state.producer_set_history = vec![
    ProducerSetChange {
        epoch: 0,
        effective_height: 0,   // authoritative from height 1
        members: vec![ProducerMember {
            producer_id:    "<stable operator id>".to_string(),
            payout_address: "<coinbase target address>".to_string(),  // NOT signing-key-derived
            public_key:     <32-byte ed25519 producer pubkey from the key ceremony>,
        }],
    },
];
```
Invariants: ≥1 member; `public_key` == the key Cindy's signer holds; `payout_address` == the address
the producer's coinbase pays (stable across signing-key rotation); `effective_height 0`.

## Sign-side contract for Cindy (unchanged, still correct)
Set `block.producer_pubkey` **before** `calculate_hash()` (folded into the V2 body); then
`sign_block_producer`; then set `producer_sig` **after** (outside the preimage). Both producer paths
(`create_next_block:179`, `pop_adapter::create_block`). Signer pubkey MUST equal Alice's seeded
member `public_key`.

## Implementation checklist — HELD until Dan `APPROVES-DESIGN` on rev 2 (`241ffeee`)
On approval, implement in this order (each RED-first; nothing merges without owner sign-off):
1. **`ProducerSetChange` / `ProducerMember`** (new `consensus/producer_set.rs`) + canonical hash +
   `DomainSeparated` impl + domain-registry row (dedicated tag, §R1).
2. **`State.producer_set_history: Vec<ProducerSetChange>`** + `validate_producer_set_history`
   (fail-closed, §3) + the causal `effective_height > containing_block_height` check in the transition.
3. **root-version BUMP** + canonical encoding/order in `state_root_material` (§6).
4. **`ApplyProducerSetChange` StateTransition** applied identically on producer + applier + WAL replay
   (the shared finalization sink); **extend the parity proof** across a set-change with a one-leg
   divergence (§6).
5. Migrate the resolver/gate to resolved `ProducerMember`s + `coinbase == member.payout_address`
   (§R2); `Enforced(Vec<ProducerMember>)`.
6. **Shared gate at BOTH boundaries** — gossip `pop_adapter::validate_block` + IBD
   `sync.rs::validate_block`, same snapshot/identity/version (§1); tests for both.
7. **Genesis seed hook** for Alice (full `ProducerMember`).
8. **Flip the e2e RED→GREEN** through both boundaries with seeded state (§7).

## Status — STACK ASSEMBLED (branch `bob/2290-producer-auth-enforcement`); gate HELD, no merge
- ✅ item 1 dedicated `ProducerSetChange`/`ProducerMember` + canonical hash + `DomainSeparated` +
  registry row.
- ✅ item 2 `State.producer_set_history` in `state_root_material` (validated order) + **root v8→v9** +
  field-pin guards (shared-protocol + native-chain).
- ✅ item 3 shared `ApplyProducerSetChange` `StateTransition` (fail-closed via `validate_candidate_change`;
  producer/applier/replay).
- ✅ item 4 ONE shared gate `verify_block_producer_authorized` wired at **both** boundaries
  (`pop_adapter::validate_block` gossip + `p2p::sync::validate_block` IBD); `SyncError::ProducerUnauthorized`.
- ✅ item 5 `State::seed_genesis_producer_set` (Alice hook).
- ✅ item 6 **e2e RED→GREEN** (5/5) through the shared gate with seeded state.
- ✅ item 7 **parity extension** — one-leg set-change diverges `state_root`, same change converges.
- ✅ Dan's 3 blockers folded (original-order validation, fallible no-drop resolver, executable causal
  rule) + §R2 canonical payout-address invariant.
- **Activation:** enforce IFF a producer set is seeded (fresh genesis seeds the set into genesis
  *state* with `effective_height 0` → authority live for the first real block, height 1;
  legacy/test chains unaffected; set is in `state_root` so it cannot be unseeded).
- **Regression-clean under v9 + gate:** shared-protocol lib **1487**, consensus-engines **434**, p2p
  **238**, native-chain launch_api **335**, producer/applier parity **14/14** converge. Workspace
  builds clean.
- ⛔ **Nothing merges without owner sign-off.** Pending: Dan re-review of the assembled stack; Cindy
  sign-side reconcile (payout_address = 0x-hex `Address`).
