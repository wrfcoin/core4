# Block-producer signing — TDD scaffold (#2290 / #2220, Round 88)

**Status:** scaffold only. No block-format change ships here. This records what the
Round-88 scaffold delivered, the design authority it followed, and the wiring
follow-ups it deliberately leaves open.

## Design authority

The sprint contract referenced `docs/design/2026-07-block-producer-signing.md`,
which **does not exist**. The authoritative, more-complete design is the pair of
merged security docs, and this scaffold is built against them:

- `docs/security/BLOCK_PRODUCER_AUTHENTICATION.md` (#2291) — what #2290 must do:
  preimage (§3.3), per-engine authority predicate (§5), the eight paths and the
  snapshot bound (§4).
- `docs/security/GENESIS_CUT_PLAN.md` (#2293) — how #2290 rides the #2034 cut:
  composed frame (§3.1), block-preimage reconciliation (§3.2), ordering (§4).

Grounding: `PROTOCOL_SPEC.md` §3/§4 (divergence D3 = #2220) and `THREAT_MODEL.md`
A1/A2.

## What shipped (this PR)

All new, additive, and **off every live path**:

- `shared-protocol/src/consensus/block_producer_auth.rs`
  - `verify_block_producer(block, attestation, identity, version) -> Result<PublicKey>`
    — authenticates the producer signature over the domain-separated
    `wrfcoin.block_header.v1` preimage (`verify_strict`, #2043). **No new tag.**
  - `sign_block_producer(..)` — the signing side (un-wired: `create_next_block`
    does not call it).
  - Per-engine authority predicates: `authorize_producer_pop` (coinbase
    recipient), `authorize_producer_in_set` (validator/delegate set membership by
    key), `authorize_producer_rotation` (`RotationPlan::accepts_key_at`, #2241).
  - `ProducerAttestation { producer_pubkey: [u8;32], producer_sig: [u8;64] }` —
    the two fields #2290 will add to `Block`, modeled standalone so the seam can
    be tested before the struct is mutated. Fixed-width, +96 bytes/block (§6.3).
- `shared-protocol/tests/block_producer_signing.rs` — the four canonical
  negatives (**unsigned / wrong-key / rotated-out / cross-set**), cross-block and
  cross-chain rejection, the honest path, and a **golden-vector** that pins the
  preimage bytes (cross-repo signer-parity anchor per `GENESIS_CUT_PLAN.md`
  §3.4/§7). Guard-revert verified: defeating the authentication guards turns the
  authentication negatives RED.

## Coordination — why the block struct is NOT touched here

Per `GENESIS_CUT_PLAN.md` §4 and sprint coordination, the real block-format
mutation is sequenced, not done here:

1. **#2219 (Alice) lands first.** It edits the same block hash (adds a `state_root`
   metadata key + a checked finalization sink) but does **not** restructure
   `Block::calculate_hash_for` or `<Block as DomainSeparated>::canonical_body`.
   #2290 rebases on top.
2. **#2290 folds inside #2226 (Cindy)'s `Block` frame-migration** — one preimage,
   one type, edited once (not twice).

So this scaffold mutates neither `Block`, `calculate_hash`, `domain_impls.rs`, nor
the existing characterization suite `tests/block_producer_authority.rs`.

## Follow-ups (not in this PR)

- Add `producer_pubkey`/`producer_sig` to `Block`; fold `producer_pubkey` into
  `Block::canonical_body` (and, at the cut, into `calculate_hash`) — inside #2226,
  after #2219. The golden vector then locks parity between this scaffold's replica
  preimage and the real `Block::canonical_body`.
- Wire `verify_block_producer` into the live `verify_block` at **path 1** (gossip,
  `p2p_events.rs` `NewBlock`) and **path 2** (IBD, `p2p/src/sync.rs`), via one
  shared helper; call the per-engine predicates.
- Sign in `create_next_block` via `load_block_signer` (node-signs; HSM path).
- Producer==applier `state_root` + WAL-replay parity across a signed block
  (needs the fields on `Block`, so it rides the wiring follow-up).
- #2256 (`FinalityVote` epoch/validator-set binding) and #2227 (validator-set-change
  tx type) — later in the contract's scope queue.

## Merge gate

Consensus-breaking surface → **owner (Tony) sign-off is the merge gate; no
self-merge.** Hold for adversarial multi-lane review. Local N5 builds only; never
push images.
