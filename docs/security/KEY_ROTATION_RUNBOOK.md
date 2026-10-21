# Validator key rotation runbook

Operator procedure for #2130. **Rehearse this on the testnet before you need it on mainnet** — an
untested rotation procedure is discovered to be broken during the incident that requires it.

## HSM path health — what is executed, and what is NOT

The story requires confirming the remote-signing path is real before relying on it. Be precise
about what the HSM CI proves. (Historically "HSM job green" could mean the `HSM_TEST_ENABLED`
runtime gate early-returned without running anything; #2748 removed that gate — the tests are
`#[ignore]`d, requested explicitly, and executed-count-checked.)

| Check | Result |
|---|---|
| `.github/workflows/hsm-signer-ci.yml` run | executes on a real hosted runner — **not** skipped |
| Gate | `--features hsm` only; the tests are `#[ignore]`d and CI requests them with `-- --ignored`, wrapped in `scripts/cargo_test_nonvacuous.py --min-executed 2` (#2867), so a run that executed fewer than both tests is red |
| Job | `cargo test --features hsm (softhsm2)` |
| Executed | `cargo test -p wrfcoin-native-chain --features hsm --test hsm_signer -- --ignored` under **both** `ONCE` and `ALWAYS_AUTHENTICATE` PIN policies |

**Classified honestly (per the #2241 review):**

- **What it establishes:** the PKCS#11 backend (`Pkcs11Signer`) signs under SoftHSM2 and its
  signature verifies. The step is *executed*, not compiled-and-skipped — the `--features hsm` gate
  plus the `--min-executed 2` wrapper is what makes that true, and a green job with the feature
  off or the tests unexecuted would prove nothing.
- **What it does NOT establish:** the HSM test creates a raw `Pkcs11Signer`, signs twice, and
  verifies. It does **not** load a `RotationPlan`, wrap through `FailClosedRemoteSigner`, switch
  HSM key labels mid-run, pull the token, or touch physical hardware. So **rotation through the
  HSM is not tested here** — only that the backend signs.
- Real-HSM / network / mid-run key-switch rotation is **#2262**, not #2130.

### How the signer/key is switched (runbook §6 step)

Switching the configured signer at the boundary is an **operator action on the environment**, not
something the code performs. `load_block_signer` reads `WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY` for the
env-key path; on the HSM path the `Pkcs11Signer` reads `WRFCOIN_HSM_KEY_LABEL` (with
`WRFCOIN_HSM_TOKEN_LABEL`). Point the relevant variable at the new key and restart/reload so
`load_block_signer` resolves the new signer. The
production tests prove the reload adopts the new key end-to-end
(`production_new_signer_becomes_authoritative_after_the_operator_swaps_it`); `load_block_signer`
itself never swaps keys — it loads whichever signer is currently configured and wraps it.

## The three rules

1. **The new key proves possession before it is effective.** A self-signed, domain-separated proof
   over `ROTATION_POP_DOMAIN || chain_id || network_id || old || new || effective_height`.
   Without it a plan can name a key whose private half is not available to the signer.
2. **Both keys are accepted during an overlap window, then the old key is rejected.** No overlap →
   in-flight messages signed just before the boundary are rejected and you miss blocks. No end to
   the overlap → the rotation revokes nothing.
3. **Double-signing across the boundary is slashable, not exempt.** See below.

## Double-signing across the boundary is NOT exempt

The tempting exemption: the two signatures are under different keys and the operator "was
rotating", so it looks like an artefact of the procedure.

Exempting it turns rotation into an **equivocation loophole** — announce a rotation, then sign two
conflicting blocks at the same height, one with each key, and claim the boundary as cover. Signing
two *different* payloads at one height is equivocation regardless of which key produced them.
`is_equivocation_across_rotation` implements this; `double_sign_across_the_rotation_boundary_is_slashable`
pins it.

Retransmitting the *same* payload under both keys is not equivocation — that is the overlap window
working as designed.

## Parameters

| Parameter | Value | Why |
|---|---|---|
| `MIN_OVERLAP_BLOCKS` | 16 | Long enough that a block signed just before the boundary still verifies on arrival; short enough that revocation is prompt. |
| `effective_height` | must be **strictly** in the future | A rotation effective now or in the past has no window in which to distribute the new key. |
| `WRFCOIN_ROTATION_SCOPE_ACK` | `payload-signing-only` | Mandatory. Acknowledges that rotation does not cover block production — see "What rotation covers" below. The node refuses to start without it. |

## Procedure

### 1. Generate the new key in its final home

Generate **inside** the HSM / remote signer. A key that exists as bytes on an operator's laptop,
even briefly, is a hot key for the rest of its life.

Confirm the file permissions on any key file are `0600` — the node refuses to load anything looser
(`security::key_file`).

### 2. Produce the proof of possession

Sign `rotation_pop_message(old_pubkey, new_pubkey, effective_height)` **with the new key**. The
function resolves the node's active chain and network identity and binds both along with the three
plan values. Produce and validate it under the same `WRFCOIN_CHAIN_ID` /
`WRFCOIN_NETWORK_ID` used by the target chain; changing any bound field makes validation reject
the mismatched plan.

### 3. Choose the effective height

Pick a height far enough ahead that every peer sees the plan before it activates. Rule of thumb:
current height + (time to distribute the plan and restart signers) + `MIN_OVERLAP_BLOCKS`.

### 4. Validate the plan before publishing

`RotationPlan::validate(current_height)` must return `Ok`. It rejects, in order: unchanged key,
effective height not in the future, overlap window below minimum, missing or invalid proof.

### 5. Publish the plan and confirm peers have it

Every verifying node needs the plan before `overlap_start()`, or it will reject the new key when it
first appears.

### 6. Cross the boundary

This is **signing-side** behaviour — what this node's signer will *issue*. It is enforced by
`FailClosedRemoteSigner` on the `sign_at_height` path and proven end-to-end through the production
constructor in `tests/validator_key_rotation.rs`
(`production_signer_enforces_the_rotation_boundary_at_height` for old-key revocation,
`production_new_signer_becomes_authoritative_after_the_operator_swaps_it` for new-key adoption).

- Before `overlap_start()`: the signer will sign only under the **old** key; the new key is refused.
- Between `overlap_start()` and `effective_height`: either key will sign. **Switch the configured
  signer to the new key here** (see §HSM for how the label/signer is switched).
- At/after `effective_height`: the signer will sign only under the **new** key. It refuses to sign
  under the old key.

**Receiver-side is NOT governed by this plan — see the scope section below.** A peer verifying a
snapshot-sync manifest or a slashing report does not consult `RotationPlan`, so "the old key is
rejected after the boundary" is true of *what this node signs*, not of *what peers accept*.

### 7. Verify

- No missed blocks in the window — check the validator produced at every height across the
  boundary.
- Confirm convergence on `state_root`, not block hash.
- Confirm this node's **signer** now refuses the old key (deliberately call `sign_at_height` with a
  payload past `effective_height` and see it refused). This confirms issuance, not receiver
  acceptance — peers do not yet enforce the boundary (#2262).

### 8. Destroy the old key

Only after the old key has been rejected for a comfortable margin of blocks. Destroying it early
leaves no way back if the rotation must be aborted.

## If the remote signer fails mid-rotation

**It refuses to sign. It does not fall back to a local key.** This is enforced by type: the only
`SignerFallbackPolicy` variant is `RefuseToSign`; there is no `FallBackToLocalKey` to select.

A validator that silently degrades from an HSM to a hot key during an outage has lost the property
the HSM was bought for, at the worst possible moment. Missing blocks is recoverable; signing with
an unintended key is not.

If this happens: stop, restore the remote signer, and only then resume. Do not extend
`effective_height` on a plan that is already published — publish a new plan.

## What rotation covers — and the part it does not

**Rotation is authoritative for the payloads the signer signs: snapshot-sync manifests and
slashing reports. It is not authoritative for block production.**

A `Block` carries no producer signature and no producer public key. Under PoP the only record of
who produced a block is the coinbase recipient, and nothing binds a key to it. So there is no
producer key for a verifier to consult, and `RotationPlan::accepts_key_at` has no caller on the
block path — not because it was forgotten, but because there is nothing to call it about.

The consequence, stated plainly: **rotating a block-producing key does not stop the old key from
producing blocks that peers will accept.** Not after the effective height, and not before the
overlap window either — no height-scoped key authority is applied to blocks in either direction.
`shared-protocol/tests/block_producer_authority.rs` proves this against the real producer and the
real verifier, and is written to fail if that ever changes.

Closing it means giving blocks a producer signature, which changes the block format and is
therefore consensus-visible. That is tracked separately as **#2290**, not in #2130.

### And within the in-scope payloads, rotation is signing-side only

Even for the payloads rotation *does* cover — snapshot-sync manifests and slashing reports — the
plan governs **issuance**, not **acceptance**:

- **Signing side (governed):** `FailClosedRemoteSigner` consults `RotationPlan::accepts_key_at` and
  refuses to sign under a key that is not authoritative at the given height. This is what §6/§7
  above verify.
- **Receiver side (NOT governed):** the verifying paths do not consult `RotationPlan`. Snapshot
  verification checks the signer against the static `snapshot_trusted_signers` allow-list
  (`shared-protocol/src/blockchain/snapshot_sync.rs`); slashing-report verification checks the
  embedded reporter key/signature and reporter authorization
  (`native-chain/src/bin/launch_api/slashing_submitter.rs`). Neither reads the effective height or
  the overlap window.

The consequence, stated plainly: an old-key-signed manifest or report is accepted or rejected by a
peer according to that **static** configuration, **not** the 16-block overlap / effective-height
rule. If the old key is still in the allow-list, a peer accepts it after the boundary; if it was
removed, a peer rejects it before the boundary. Rotating the signer changes what *this* node emits,
not what its peers will honour.

Making receivers plan-aware — distributing the plan to verifiers and having them enforce the
overlap/effective-height boundary on these payloads — is the multi-node transport work tracked in
**#2262**, not #2130.

Because this is not visible from the configuration, the runtime now requires you to say you know:

```bash
export WRFCOIN_ROTATION_SCOPE_ACK=payload-signing-only
```

Without it the node **refuses to start** when a rotation is configured. This is deliberate — an
earlier round of this story documented the limitation and the documentation did not reach the
person who needed it.

## DO NOT ROTATE ON A LIVE NETWORK YET

**Do not perform a validator key rotation on the incentivized testnet or on mainnet until the
multi-node transport test in #2262 exists and passes.** This runbook is a rehearsal aid, not a
substitute for it. Rotation on a throwaway local testnet, with an operator watching and a rollback
path, is fine — that is how #2262 should be developed.

The reason is the one this whole document is about: an untested rotation procedure is discovered
to be broken during the incident that requires it, and rotation is a deliberate, scheduled version
of the #2076 asymmetry.

## What the automated tests cover, and what they do not

`shared-protocol/tests/validator_key_rotation.rs` runs a deterministic 4-validator fixture across
130 blocks with one validator rotating at height 100, asserting every block is produced and
accepted by every validator. It covers the **key-acceptance rules**.

Peers are **independently configured**: a peer that never received the plan rejects the rotated
key, and a peer holding a skewed `effective_height` rejects a block the correctly-configured peer
accepts. That is the one-node-diverges class, and the shared-plan version of this fixture could not
reach it.

Fail-closed is enforced on the **real** signer path, not asserted about a fake:
`load_block_signer` refuses when HSM configuration is present on a binary built without
`--features hsm` rather than falling through to the software signer, and `FailClosedRemoteSigner`
holds no local key material to fall back to.

It still does **not** cover the transport: no sockets, no processes, no real PKCS#11 token, no
running block producer. That gap is **#2262**, and the paragraph above says what must not happen
until it closes. It is not closed by this document.

## Related

- #2128 — key-role separation; consensus and withdrawal keys must already be distinct
- `docs/security/SECRET_HANDLING.md` — never log key material during a rotation
- `docs/security/PREIMAGE_MIGRATION.md` — the other coordinated-cutover procedure
