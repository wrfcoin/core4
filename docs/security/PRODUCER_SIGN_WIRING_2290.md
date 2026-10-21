# #2290 producer-SIGN wiring — implementation contract (Cindy)

Companion to Bob's `BLOCK_PRODUCER_AUTHENTICATION.md` (VERIFY side + authority set).
This documents the **SIGN** side only. **Consensus-breaking** — RED-first, owner sign-off before merge.

## Live sign sites (PoP)

Traced from callers, not the issue's line pointers (which name the legacy path):

1. **`shared-protocol::consensus::proof_of_publishing::ProofOfPublishing::create_next_block`**
   (`:1158`, hashes at `:1653`). Reached by the engine path:
   `consensus-engines::engine` → `pop_adapter::create_next_block` (`:186` delegates to `pop.create_next_block`).
2. **`consensus-engines::algorithms::pop_adapter::PopConsensusAdapter::create_block`**
   (`:266`, hashes at `:341`). Reached by `/blocks/mine` with pre-assembled txs (the live testnet path —
   the N5 collector drives this).

**`native-chain/src/legacy/proof_of_publishing.rs:179` is a PARITY REFERENCE**, not the live producer
(`native-chain/src/parity.rs` pulls it in as `legacy_pop`). Its blocks never reach the wire, so it is
out of the live sign set. **⚠️ CONFIRM with Bob** (his #2395 made the same legacy≠live correction for verify).

## Mechanism (uses the existing primitives — no preimage reimplementation)

`producer_pubkey: [u8;32]` is **in** the block hash (folded into `<Block as DomainSeparated>::canonical_body`);
`producer_sig: Vec<u8>` is **outside** it. So:

```
1. block.producer_pubkey = producer_signer.public_key()   // BEFORE calculate_hash
2. block.calculate_hash(); block.set_hash(..)             // pubkey now bound into the hash
3. att = attest_block_producer(&block, &*producer_signer, ChainIdentity::active(), SigningProtocolVersion::active())
4. block.producer_sig = att.producer_sig.to_vec()         // AFTER hash (sig is outside it)
```

- New helper `attest_block_producer(block, signer: &dyn BlockSigner, identity, version) -> Result<ProducerAttestation>`
  in `block_producer_auth.rs`: `signer.public_key()` + `block_producer_preimage(..)` (the existing public
  primitive) + `signer.sign(preimage)`. **HSM-capable** (BlockSigner), unlike `sign_block_producer(&PrivateKey)`
  which stays the in-process/test path.
- Signer storage: `ProofOfPublishing.producer_signer: Option<Arc<dyn BlockSigner>>` + `set_producer_signer()`,
  mirroring the existing `zk_verifier`/`set_zk_verifier`. `pop_adapter::create_block` reads it through `self.pop`.
- **No signer configured ⇒ block stays unsigned** (dev/local, genesis, pre-activation). Bob's verify must
  admit unsigned below the activation height / for genesis — a cut-level (#2034 rider) gate.

## Key provisioning (P4)

`load_producer_signer()` in `launch_api/main.rs`, mirroring `load_snapshot_signer()`:
`protocol_signer::load_block_signer(WRFCOIN_PRODUCER_SIGNER_PRIVATE_KEY)` — HSM in release
(N5 yubikey/softhsm), env/file key for local/dev. Injected at startup via `pop_adapter.set_producer_signer(..)`.
The **producer pubkey is exposed** (startup log + accessor) so Alice seeds it into the genesis authorised set
and Bob's verify can check it. Distinct key from the snapshot signer.

## Owner decision §R2 (LOCKED) — coinbase is DECOUPLED from the signing key

The producer signs the block for **authentication** (`producer_pubkey` + `producer_sig`). The block's
**coinbase recipient is the producer member's `payout_address`**, seeded by Alice at genesis — **NOT**
the signing-key-derived address (`pubkey.to_address()`). Member shape is locked:
`ProducerMember { producer_id, payout_address, public_key }` (Bob #2398 rev2); my signer supplies
`public_key`, the `payout_address` comes from the seeded member.

**SIGN side is already compliant and needs no change:** the coinbase recipient is `miner_address`, a
parameter threaded from the caller (`/blocks/mine` reads it from the request body; `Transaction::new_coinbase(miner_address, ..)`).
My sign path only sets `producer_pubkey`/`producer_sig` and never touches the coinbase — verified, and
locked by `producer_signature_is_decoupled_from_coinbase_payout_address_2290_r2` (a block signed by K
pays a coinbase to an address ≠ `K.to_address()` and still authenticates under K).

## Coordination items for Bob (VERIFY owner) — agree before merge

1. **Live sign-path set** above (legacy is parity-only?).
2. **PoP authority under R2** — ⚠️ Bob's current `producer_authority.rs` still ties
   `authorize_producer_pop(key, coinbase_recipient)` (i.e. `key.to_address() == coinbase`). Under R2 that
   coupling is **wrong**: the authenticated key must be an authorised `ProducerMember`, and the coinbase
   recipient must equal that **member's `payout_address`**, not `key.to_address()`. Needs updating on the
   verify side to the member-payout binding.
3. **Unsigned/activation handling**: below activation height / genesis, verify admits `producer_sig` empty.

## RED-first

A block built through the real create path with a producer signer set is accepted by
`verify_block_producer_on`; reverting the `attest_block_producer` call makes it fail (`ProducerAuthError::Unsigned`).

## Flagged depth

- Two live create paths must sign **identically** or producer/applier `state_root` parity breaks
  (coordinate with the parity lane / #2387).
- HSM `BlockSigner` vs the `&PrivateKey` primitive — resolved by `attest_block_producer` over the trait.
- Activation-height gate is a cut-level decision, not wired here beyond "unsigned when no signer".

---

# Round 2 — Elsa CHANGES on PR #2399 (2026-07-24)

Elsa's cross-provider review confirmed the signing core (both builders stamp before the hash and
attach after; the shared `block_producer_preimage` is reused; RED-on-revert genuinely fails) and
returned four P1 blockers on atomicity, rotation, the §R2 binding, and provisioning. All four are
addressed below. Every guard was proven RED against the pre-fix code before being fixed.

## P1-1 — block production is now atomic across the signature

**The bug.** `create_next_block` committed governance transitions, launch-state transitions, rewards
and `apply_block_finalization` directly into `chain.state`, and only *then* called the fallible
`attach_producer_signature`. A signer failure returned `Err`, so `add_block` never ran and
`height`/`tip` stayed put — but the post-block `state_root` was already installed. The node's state
disagreed with its own chain tip, and everything it produced or applied afterwards diverged from its
peers. Elsa reproduced it independently (root moved from all-zero to `c75f4478…`).

**The fix.** The whole post-block transition batch runs against a **staged** `State` copy. Nothing
touches `chain` until the block is built, hashed *and* signed; then `add_block` (which validates
before it mutates) runs first and the staged state is installed after. State commit and block commit
are one atomic step in both directions.

This required `apply_governance_transitions` and `process_contribution_rewards` to take `&mut State`
rather than `&mut Blockchain` — both were already state-only, delegating through
`Blockchain::apply_state_transitions_batch` to `State::apply_transitions_batch`. The same staging was
applied to `replay_received_block_for_test_support`, the applier-shaped path, so a rejected
state-root commitment leaves a follower's state untouched instead of half-applied.

The fix is not signature-specific: a mid-sequence failure in rewards or finalization now also
discards the earlier transitions rather than half-applying them.

Guard: `failing_producer_signer_leaves_consensus_state_untouched_2290_p1_1` — asserts `state_root`,
`height` and `tip` are all unchanged after a deliberately failing signer. RED on `state_root` before
the fix.

## P1-2 — the producer path is rotation-aware

`attest_block_producer` now signs via `sign_at_height(&preimage, block.height)`. `BlockSigner`
documents that callers holding a height MUST use the height-aware form, and a block always has one.
This was not a style point: `load_block_signer` wraps every production signer in
`FailClosedRemoteSigner`, which **refuses** heightless signing during an active rotation — so the
heightless call both lost rotation enforcement on the block path and made a rotating node unable to
produce blocks at all.

Guards (both RED before the fix):
- `producer_signs_at_block_height_during_an_active_rotation_2290_p1_2` — a producer whose key is
  authoritative at the block height can sign during an active rotation, and the result verifies.
- `producer_is_refused_when_its_key_is_not_authoritative_at_the_height_2290_p1_2` — a rotated-out key
  is refused *with the boundary named*. Asserting on the reason is what keeps this non-vacuous: the
  pre-fix code also failed, but with the generic heightless refusal.

## P1-3 — §R2 is load-bearing on the production path

**The gap.** The verify side (#2398) reads `block.transactions[0].outputs[0].address` and compares it
to the authenticated member's canonical `payout_address`. The producer took the coinbase recipient
straight from the caller — `/blocks/mine` passes the request body's `miner_address` through
unvalidated — so a node could authenticate correctly, pay any address, commit locally, and be
rejected by every peer: a silent self-fork carrying a valid signature.

**The fix.** `ProofOfPublishing::enforce_producer_payout_binding` refuses to build a block whose
actual coinbase output is not the producer's declared canonical payout. Both builders call it, and
`create_next_block` calls it **before the write lock and before any state mutation**. No binding
configured ⇒ no check, so chains that never seeded a producer set are unaffected.

The binding is set at startup from `WRFCOIN_PRODUCER_PAYOUT_ADDRESS`, which must equal this node's
`ProducerMember::payout_address` in the genesis-seeded set. A malformed value is fatal, not ignored —
silently skipping the binding would restore the fail-open behaviour this closes.

**Interim source of truth.** Once #2398 merges, the payout is resolvable on-chain from
`chain.state.producer_set_history` keyed by the producer public key, and the env var becomes a
startup cross-check rather than the source. `producer_set_history` does not exist in this branch, so
the env var is currently the only way the producer can know its own declared payout.

The old R2 test was replaced. It asserted `metadata["miner_address"]` against a
`PublicKey::to_address()` (`wrf…`) value and called only the authentication-only verifier, so it
could not fail for the reason that mattered. The replacements:
- `coinbase_pays_the_canonical_member_payout_not_the_signer_2290_r2` — canonical `Address` (`0x…`,
  round-tripped through `parse::<Address>()`), payout `!= K.to_address()`, block authenticates under
  K, and the **actual coinbase output** equals the payout: the exact comparison the gate performs.
- `producing_a_block_that_pays_the_wrong_address_is_refused_2290_p1_3` — RED without the binding:
  production succeeds and emits a validly-signed block paying the wrong address.
- `payout_binding_is_inert_when_unconfigured_2290_p1_3` — scope guard.

**Still open with Bob:** the combined test cannot call `verify_block_producer_authorized` from this
branch — it lives in #2398. The property asserted here is mechanically the same comparison, but the
literal cross-head assertion belongs in whichever PR merges second.

## P1-4 — provisioning fails closed

- **Fail-closed:** `WRFCOIN_PRODUCER_AUTH_REQUIRED` declares that this chain enforces producer auth.
  When set, a missing *or broken* signer is a startup refusal with a diagnostic, not a warning
  followed by unsigned production. Policy is isolated in the pure `producer_signer_decision` so it is
  directly testable; four guards cover it, two RED against the old fail-open policy. Unsigned
  production stays legal when the flag is unset, so legacy and local-dev nodes are unaffected.
- **Dev file path added:** `WRFCOIN_PRODUCER_SIGNER_KEY_FILE`, resolved ahead of the hex env var
  because a file can be mode 0600 and a process environment cannot. Permissions are enforced via
  `security::key_file` *before* the bytes are read. Three guards; the loose-permission one is RED
  without the check.
- **HSM key reuse resolved:** `WRFCOIN_HSM_PRODUCER_KEY_LABEL` selects a producer-specific PKCS#11
  key object. When unset the shared `WRFCOIN_HSM_KEY_LABEL` is still used — some deployments have one
  slot — but it now logs a warning naming the reuse. Key reuse across roles remains possible, never
  silent. **Owner sign-off is still required** if the testnet intends to run with one key for both
  the snapshot and producer roles.

## Verification

- `cargo check --workspace --all-targets` clean; `launch_api` clean in default and `--features hsm`.
- shared-protocol lib 1482/1482 (1484/1484 with `hsm`); consensus-engines lib 439/439;
  `launch_api` bin 280/280.
- Golden/integration: `block_producer_signing` 12/12, `block_producer_authority` 4/4,
  `validator_key_rotation` 18/18.

---

# Round 3 — Elsa CHANGES on `a2d23103` (2026-07-24)

Round-2 fixes all held under her independent RED-on-revert probes. Three blockers
remained. A **manager correction** also landed: reading the genesis-seeded producer set to
resolve the payout is *not* covered by the owner's §P5 deferral — that deferral is only the
governance **mutation** path. So the on-chain lookup is in scope, and the interim env source is not
acceptable.

## Blocker 3 — the false rotation acknowledgement is gone

`WRFCOIN_ROTATION_SCOPE_ACK=payload-signing-only` forced the operator to affirm that rotation does
**not** cover block production, "because a Block carries no producer signature". True under #2130;
false here. Blocks carry `producer_pubkey`/`producer_sig` and `attest_block_producer` signs at the
block height, so a rotated-out key is refused at signing time.

A startup gate demanding assent to a false statement is worse than no gate — it trains operators to
affirm text they should be reading. Removed rather than reworded: the constants are deleted, the
loader warning now states that block production **is** covered, and the leftover variable is inert.

Three guards, all RED with the gate restored. The third asserts the claim as **behaviour**, not
prose: the loaded signer refuses to sign a block-producer preimage at a height where its key is not
authoritative. That is what makes the deletion honest — the warning went because what it warned
about stopped being true, not because it was inconvenient.

## Blocker 1 — activation comes from the chain, and the switch parses fallibly

**Activation.** `producer_auth_active(chain_has_producer_set, env_override)` derives from
`!state.producer_set_history.is_empty()` — the same field `verify_block_producer_authorized` treats
as authoritative, so the producer and the verifier cannot disagree about whether this chain
enforces. `WRFCOIN_PRODUCER_AUTH_REQUIRED` can only *tighten*; setting it false on a chain with a
seeded set is a loud refusal, not a silent override.

**The vacuous parser guard.** Elsa replaced `producer_auth_required_from_env()` with `false` and the
old test still passed, because it re-typed `matches!(v, "1" | "true" | …)` inline instead of calling
the parser. Startup logic is now one seam —
`producer_startup_decision(chain_has_producer_set, raw_override, resolution)` — which the tests drive
with **raw strings**, so a broken parser, a broken activation source or a broken policy all surface.

`parse_security_switch` is fallible: recognised on/off spellings are accepted, and any other explicit
value is an `Err` that aborts startup. This is Frank's `WRFCOIN_REQUIRE_PRODUCER_AUTH=tru` class from
#2397 — a typo that reads as *false* lets an unauthenticated chain start while its operator has
evidence they enabled enforcement.

Both of Elsa's mutations were reproduced and now fail: parser→constant breaks two guards,
activation→env-only breaks the seeded-set guard.

## Blocker 2 — the payout is resolved from the chain

`resolve_expected_payout` reads `State::producer_set_history` through **Bob's own**
`GovernanceProducers` resolver. Reusing his type rather than re-deriving the active member is
deliberate: two independent resolutions of "which member is active at this height" are two things
that can disagree, and a producer/verifier disagreement is the fork this epic exists to prevent.

- Not a member at this height ⇒ refuse to produce (every block would be `NotAuthorized`).
- Invalid history ⇒ refuse (the verifier fails closed on the same history).
- The operator env value is demoted to a **fatal equality cross-check**: if it disagrees with the
  chain, production stops rather than silently preferring either. A stale config that quietly lost to
  the chain would mean the deployment's stated intent and the node's behaviour differ with nobody
  informed.

Four guards including the combined test Elsa specified: signer K, canonical `0x…` payout
`!= K.to_address()`, a seeded `ProducerMember` mapping K → that payout, the **actual coinbase
output** equal to it, and **`verify_block_producer_authorized` accepting the produced block**. The
producer is given no payout configuration at all — it learns the address from the chain.

Three of the four go RED when the on-chain resolution is removed. The combined test stays green by
design: it asserts cross-head *agreement*, and enforcement is proven by its siblings.

One assertion was re-targeted after a mutation exposed it: the stale-config test originally asserted
only that both addresses appeared in the error, which the generic coinbase-mismatch message also
satisfies. It now pins the cross-check wording, so it fails when the chain stops being authoritative.

## Sequencing — where blocker 2 lives

Blocker 2 needs `State::producer_set_history`, which lands with #2398. It is implemented and verified
on **`cindy/2290-integration-probe`** (`cindy/2290-producer-sign` merged with Bob's `a64f2bf4`), not
carried on this PR's head, because it cannot compile without his field. That branch is pushed and
reviewable; it drops onto this PR once #2398 is in main. `chain_has_seeded_producer_set()` on this
head is the single wiring point, returning `false` until the field exists — correct rather than
fail-open, since no verifier in this tree enforces producer authority yet.

## Owner decision implemented

Producer-specific HSM key label is the intended path; the shared-label fallback stays a loud warning.
The `pkcs11.rs` warning and this document no longer frame reuse as an open owner question.

## Pre-existing flake (not from this PR)

`consensus::proof_of_publishing::tests::low_score_verification_never_uses_pow_fallback` and siblings
fail intermittently under the concurrent lib run — roughly 2 in 5. Reproduced on **`origin/main`**
and on Bob's branch independently of this work; passes in isolation, so it is shared-global test
pollution rather than a defect in the code under test. Flagged for its own issue, not chased here.

A genuine test-isolation bug **was** fixed: two `load_block_signer` tests read the `ROTATION_*` vars
without taking the module's process-env lock. The new blocker-3 guard widened that latent window
enough to fail consistently; both now take the lock.

---

# Round 4 — rebased onto #2398, B2 folded in (2026-07-25)

#2398 merged to `main` as `54eed056`, so this branch was rebased onto it and Elsa's ordering (a) was
executed: **B2 lands after #2398 and before any cut.**

## Rebase

Six commits replayed onto `54eed056`. One was **dropped**: the rotation env-lock fix, which had
already landed independently as #2402 — exactly the duplicate that PR's body said to drop here. The
only conflict was its comment text; the lock lines themselves were already on main, and main's
wording was kept.

The consensus surfaces (`pop_adapter.rs`, `launch_api/main.rs`, `block_producer_auth.rs`,
`proof_of_publishing.rs`) merged without textual conflict. That is not by itself evidence of
correctness, so the semantic checks below were run rather than assumed.

## Resolved toward Bob's canonical types

`ProducerMember`, `ProducerSetChange`, `GovernanceProducers` and `verify_block_producer_authorized`
are now on main and are **the** definitions. Verified by grep that this branch introduces **no**
duplicate of any of them — the drift Elsa's ordering was chosen to prevent.

The sign side reuses his `GovernanceProducers::try_from_history` +
`authorized_producers_at(height)` rather than re-deriving the active member, so the producer and the
verifier resolve membership through one code path and cannot disagree.

## B2 — the payout comes from the chain

- `resolve_expected_payout(producer_set_history, height)` looks up this node's producer key in the
  seeded set and returns that member's canonical `payout_address`.
- `enforce_producer_payout_binding` binds the **actual coinbase output** to it, before any state
  mutation. Non-membership or an invalid history is a refusal to produce.
- `WRFCOIN_PRODUCER_PAYOUT_ADDRESS` is demoted to a **fatal equality cross-check**.
- Activation derives from `!state.producer_set_history.is_empty()` — the verifier's own source. The
  `chain_has_seeded_producer_set()` stub that returned `false` is now wired to the real field.

Elsa's combined test passes with the producer given **no** payout configuration: it learns the
address from the chain, and `verify_block_producer_authorized` accepts the block it builds.

## Verification at the new head

| Suite | Result |
|---|---|
| `cargo check --workspace --all-targets` | clean |
| same, `--features hsm` | clean |
| shared-protocol lib | 1495/1495 (1497/1497 with `hsm`) |
| consensus-engines lib | 445/445 |
| `launch_api` bin | 308/308 |
| `#2290` tests (consensus-engines) | 9/9 |
| **Bob's `producer_auth_enforcement_e2e_2290`** | **5/5** |
| `block_producer_signing` / `block_producer_authority` | 12/12 / 4/4 |
| `validator_key_rotation` / `state_root_commitment_2219` | 18/18 / 3/3 |

Bob's e2e passing unchanged on this branch is the load-bearing cross-side signal: his verify gate
accepts what this producer builds.

Mutation re-verified post-rebase — removing the on-chain resolution turns 3 of the 4 B2 guards RED.

## Review status

**The `c7114fa3` APPROVE does not carry over.** The rebase plus B2 moved the head, so this needs a
full re-review at the new head; nothing merges on the old verdict.
