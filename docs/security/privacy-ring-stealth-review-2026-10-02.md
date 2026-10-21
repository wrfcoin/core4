# Security review: privacy ring signatures and stealth addresses

- **Date:** 2026-10-02
- **Scope commit:** `58f1cdcc8add48bf39dafb42130b21e15f7e72b0` (`origin/main`)
- **Decision record:** core4#3213 (privacy, ring signatures + stealth addresses only, is to be compiled into testnet builds with a later switch-on; this review gates all implementation). Epic: core4#3400.
- **Prior review:** core4#717 (cryptographer review of range proofs, ring signatures, stealth addresses) was already marked complete on GitHub. This document is a fresh code-level pass at the commit above and does not rely on that outcome.
- **Out of scope:** zk proofs, Pedersen commitments and range proofs (reviewed only where a ring-signature path depends on them), P-256 passkey verification.
- **Type of review:** defensive source review plus three scratch tests. No crypto code was changed; scratch tests were run on an uncommitted working copy and reverted.

## 1. Summary

The MLSAG/stealth primitives themselves are a reasonable LSAG-style construction on Ristretto255 with sound basics: OS CSPRNG, prime-order group (so no torsion or cofactor problems), domain-separated hashing, canonical point decoding, a length-framed signing preimage in the consensus callers, and redacted `Debug` on stealth keys. The problems are mostly **around** the primitives: how the consensus state stores and compares key images, what the verifier does not enforce, and the fact that the privacy paths are **already live in default builds**.

| Severity | Count | Meaning |
|---|---|---|
| P1 | 2 | must be remediated before compile-in |
| P2 | 8 | must be remediated before switch-on |
| P3 | 8 | hardening |

Headline: the brief's premise that "none of this is compiled into today's node builds" does not hold. `wrfcoin-privacy` is an unconditional dependency of `shared-protocol`, and the state machine applies `RewardClaim` and `ConfidentialTransfer` (including MLSAG verification) with activation height 0. One of those paths has a demonstrated double-claim bypass (P1-1).

## 2. Method

1. Read every module in `privacy/src/` (3.5k lines) and every consumer: `shared-protocol/src/blockchain/state.rs`, `shared-protocol/src/consensus/proof_of_publishing.rs`, `shared-protocol/src/consensus/verify_cost.rs`, `native-chain/src/bin/launch_api/routes/privacy.rs`, `state.rs`, `main.rs`, and the Cargo feature wiring.
2. Ran `cargo test -p wrfcoin-privacy --release`: **126 passed, 0 failed** (104 unit, 18 `privacy_e2e`, 4 doc-tests).
3. Wrote scratch tests (uncommitted, reverted) to confirm candidate defects: scalar malleability, zero-key signing, ring-of-1 and duplicate-member rings, verification cost at `MAX_RING_SIZE`, and two consensus-state scenarios on `RewardClaim` (hex-case replay, failed-claim state mutation). The state scenarios ran with `cargo test -p wrfcoin-shared-protocol --lib --features test-support`.
4. Checked existing test coverage against a negative-test checklist (section 8).
5. Not done: no live-network or host access (per rules), no fuzzing campaign, no side-channel measurement on hardware, no review of `wrfcoin-p2p` gossip of these transactions.

Measurements below come from an AMD Ryzen 9 5950X under WSL2, release profile.

## 3. Inventory

### 3.1 Crate `wrfcoin-privacy` (`privacy/`)

| Module | Public surface | Notes |
|---|---|---|
| `ring_signatures` | `MLSAGSignature::{sign,verify}`, `KeyImage::{generate,from_bytes,to_bytes}`, `RingMember`, `RingSignatureParams`, `EntropySource`/`SystemEntropy`, `MIN_RING_SIZE=7`, `MAX_RING_SIZE=1024` | LSAG-style single-layer ring over Ristretto255, SHA3-256 challenge, SHA3-512 hash-to-point. `RingSignatureParams` and `EntropySource` are not used by anything. |
| `stealth_addresses` | `StealthKeys::generate`, `StealthAddress::{new,is_for_recipient,recover_private_key,decrypt_payment_id}` | Monero-style dual-key stealth; no output index. |
| `key_image_registry` | `KeyImageRegistry::{register,is_spent,count}` | In-memory `RwLock<FxHashSet<[u8;32]>>`. |
| `mlsag_budget` | `verify_batch_budgeted`, `verify_deferred`, `MLSAG_VERIFY_BUDGET_MS_DEFAULT=50` | Wall-clock budget. |
| `payload_encryption` | `encrypt_payload`, `decrypt_payload` | ECIES-style, ChaCha20-Poly1305. Out of main scope. |
| `pedersen`, `range_proof` | commitments / range proofs | Out of scope except where noted. |
| `p256_ecdsa` | WebAuthn P-256 verification | Out of scope; used by `shared-protocol` passkeys. |
| `types` | `PrivacySettings` (`min_ring_size: 11` default) | The 11 default is not what consensus enforces (7). |

Dependencies: `curve25519-dalek 4.1` (features `serde`), `sha3`, `rand_core 0.6` (`OsRng`), `zeroize 1.7` (used only in `pedersen.rs`), `chacha20poly1305`, `merlin` (declared; not used by ring/stealth code).

### 3.2 What is compiled into node builds today

| Consumer | Wiring | Compiled by default? |
|---|---|---|
| `shared-protocol` | `wrfcoin-privacy = { path = "../privacy" }` (`shared-protocol/Cargo.toml:84`), **not optional** | **Yes** |
| `shared-protocol` state machine | `apply_confidential_transfer` (`state.rs:7551`, dispatch `:4494`, `:7224`) and `apply_reward_claim` (`state.rs:7720`, dispatch `:4509`, `:7305`) call `MLSAGSignature::verify`. Activation constants: `RING_SIG_VERIFICATION_ACTIVATION_HEIGHT = 0` (`state.rs:46`), `REWARD_CLAIM_ACTIVATION_HEIGHT = 0` (`transaction.rs:2530`). | **Yes** |
| `shared-protocol` block verification | MLSAG budget pre-pass for `RewardClaim` (`proof_of_publishing.rs:7835-7900`) | **Yes** (behind a height constant) |
| `shared-protocol` fee pricing | `verify_cost.rs:693` pins its ring ceiling to `MAX_RING_SIZE` | Yes |
| `native-chain` | `privacy = ["dep:wrfcoin-privacy"]` (`Cargo.toml:66,82`) gates `routes/privacy.rs`, `KeyImageRegistry` in `AppState` | No (off by default) |
| `ShieldedDeposit` / `ShieldedWithdrawal` | rejected at mempool admission (`mempool/pool.rs:358-371`, #1762) | Disabled |

I found no admission-time or `verify_block`-time rejection of `ConfidentialTransfer` or `RewardClaim` comparable to the shielded one, and no `private_transfers` feature anywhere in the workspace. Whether the live testnet currently admits these types was **not verified** (no host access).

**Build gating at the deployed B3.1 commit `fe8b3191`, proven from source.** The shipped node image is built by
`infra/docker/Dockerfile.core` at infra `750298cd` (the revision B3.1 pins), and what each source says is:

| Question | Source at the pinned commit | Answer |
|---|---|---|
| Which crate feature gates privacy code? | `native-chain/Cargo.toml:82` `privacy = ["dep:wrfcoin-privacy"]`; the `#[cfg(feature = "privacy")]` sites in `native-chain/src/bin/launch_api/main.rs` (:37, :66, :1738, :1747, :3147, :3292-3294, :3324) | Only native-chain's HTTP routes and `KeyImageRegistry`. |
| Does the shipped `launch_api` enable it? | `Dockerfile.core:43-45`: `--bin launch_api --features "full-mining,p2p,hsm,upgrade-notice-fetch"`; `native-chain/Cargo.toml` declares no `default =` | **No**: the route layer and registry are not compiled into the shipped `launch_api`. |
| Is the privacy crate in the dependency graph anyway? | `shared-protocol/Cargo.toml:84` `wrfcoin-privacy = { path = "../privacy" }`, not optional; `native-chain/Cargo.toml:53` depends on `wrfcoin-shared-protocol` | **Yes**: the crate is linked into every build, including the shipped node. |
| Is the state machine's privacy handling feature-gated? | `state.rs:7224` (`ConfidentialTransfer`), `:4494`, `:4509` (`ApplyConfidentialTransfer`, `ApplyRewardClaim`) carry no `cfg(feature)`; activation constants are 0 (`state.rs:46`, `transaction.rs:2530`) | **No**: that code is compiled in, and its activation heights are 0 in source. |

The accurate statement is therefore **"the privacy HTTP surface is not built into the shipped `launch_api`, while the
privacy crate and the consensus handling of `ConfidentialTransfer` and `RewardClaim` are compiled into every build."**
This review does **not** claim the privacy feature is compiled out of live builds, and it does **not** claim any live
state: whether the live chain has admitted or applied such a transaction, and what the running nodes' admission
policy is, are **unverified** (no host access, and no query of the live node).

### 3.3 Safe subset

The subset that can be compiled in and exercised by library callers without exposing consensus or custody risk:

1. Pure library code: point/scalar decoding, `KeyImage::generate`, `MLSAGSignature::verify`, `StealthAddress::{new,is_for_recipient,decrypt_payment_id}`, `StealthKeys` (with redacted `Debug`), **behind a strict verifying wrapper** that enforces the P2-2 hygiene rules (canonical scalars, distinct members, non-identity keys, ring size in `[MIN, MAX_NEW]`).
2. Signing helpers for **client-side** use only (SDK/CLI), never inside the node process.

Explicitly **excluded** from the safe subset until their findings are addressed:

- `ConfidentialTransfer` and `RewardClaim` state transitions (P1-1, P1-2, P2-4, P2-8).
- `native-chain` HTTP privacy routes, which hold and return private keys (P2-7).
- The in-memory `KeyImageRegistry` as a double-spend authority (P2-6).
- Anything involving range proofs / commitments for value (zk-adjacent, separate review).

## 4. Findings

Line numbers refer to the scope commit.

| ID | Sev | Location | Finding |
|---|---|---|---|
| P1-1 | P1 | `state.rs:1144,7692,7751,8690-8704`; `transaction.rs:2584` | Key images are tracked as case-sensitive hex **strings**; an upper-case spelling of a spent key image is accepted again. |
| P1-2 | P1 | `shared-protocol/Cargo.toml:84`; `state.rs:4494,4509,7224,7305`; `transaction.rs:2530`; `state.rs:46` | Privacy transitions are live in default builds with activation height 0; no `private_transfers` gate exists. |
| P2-1 | P2 | `ring_signatures.rs:440-443,454-461,473` | Non-canonical `c`/`s` scalars are accepted; JSON signature encoding is not canonical. Signature and txid malleability. |
| P2-2 | P2 | `ring_signatures.rs:286-303,408-475`; `state.rs:7644,7779-7799`; `station_registry.rs:107` | Verifier does not enforce ring hygiene: minimum size, distinct members, non-identity keys. `sign` accepts a zero private key. Station `ring_public_key` is never validated. |
| P2-3 | P2 | `ring_signatures.rs:27-34`; `mlsag_budget.rs:37-52`; `proof_of_publishing.rs:7835-7900` | Verification cost at `MAX_RING_SIZE` is about 3x the documented bound; the budget pre-pass uses wall-clock time inside consensus code. |
| P2-4 | P2 | `state.rs:7644-7656,7686-7700,7751-7810` | Only one of the payload's `key_images` must match the signature; the rest are inserted unverified. A claim records its key image before the signature is verified. |
| P2-5 | P2 | `stealth_addresses.rs:169-187,221-230` | Shared-secret derivation has no domain tag and no output index; payment-id pad reuses the scalar-derivation bytes; no uniqueness requirement on one-time keys. |
| P2-6 | P2 | `key_image_registry.rs:63-72`; `native-chain/.../state.rs:123,772` | Registry `is_spent` fails open on a poisoned lock, has no reorg undo, is not persisted, and is a second source of truth next to `State.privacy_spent_key_images`. |
| P2-7 | P2 | `native-chain/src/bin/launch_api/routes/privacy.rs:181-230,457-502` | HTTP routes accept and generate private keys server-side and sign on the node. |
| P2-8 | P2 | `state.rs:7551-7700,7806-7820` | `ConfidentialTransfer` verifies a ring signature but moves no value and binds the ring to no outputs; `RewardClaim` never verifies its `range_proof`. |
| P3-1 | P3 | `ring_signatures.rs:483-499` | Challenge hash does not bind the ring public keys; 256-bit reduction has a small bias. |
| P3-2 | P3 | `ring_signatures.rs:303-373`; `stealth_addresses.rs:45-113,153-198`; `routes/privacy.rs:181-230` | Secret scalars and key bytes are not zeroized; `StealthKeys` has no `Drop`. |
| P3-3 | P3 | `stealth_addresses.rs:139-300` | No rejection of identity keys; payment id is unauthenticated. |
| P3-4 | P3 | `payload_encryption.rs` | Ephemeral key and nonce are not bound as AEAD associated data. |
| P3-5 | P3 | `routes/privacy.rs:504-534` | Malformed signatures return HTTP 500 and `log::error!`; unbounded ring work per request. |
| P3-6 | P3 | `key_image_registry.rs:32` | `FxHashSet` on attacker-influenced keys (not DoS-resistant). |
| P3-7 | P3 | `ring_signatures.rs` doc comments, `types.rs:26` | Documentation drift: "<100 ms" verify claim, `min_ring_size: 11` vs enforced 7, unused `EntropySource` and `RingSignatureParams`. |
| P3-8 | P3 | `privacy/src/tests/degenerate_inputs.rs:456-496` | Identity tests only mutate an already-valid signature, so they do not exercise a signer who legitimately holds the identity key. |

### P1-1: Key-image double-spend through hex case (REWARD CLAIM and CONFIDENTIAL TRANSFER)

**Description.** `State.privacy_spent_key_images` and `reward_claim_key_images` are `FxHashSet<String>` (`state.rs:1144,1167`). Dedup is `insert(key_image.clone())` on the payload string (`:7692`) or `format!("{epoch}:{key_image}")` (`:7751`). `validate_hex_blob` only checks that the string hex-decodes and is **at least** N bytes (`:8696-8702`), and `hex::decode` accepts both cases. The ring-signature binding compares decoded **bytes** (`:7644-7650`, `:7798`), so a differently-cased spelling of the same 32 bytes passes the signature binding but is a new set member. For the confidential path, `>=32` also lets longer hex strings through for the *extra* entries (see P2-4).

**Evidence.** Scratch test on `apply_protocol_transaction` (uncommitted, reverted), one signer key, ring of 8 registered stations, epoch 0, each claim freshly signed over its own payload:

```
REVIEW claim1(lower)=Ok(()) | claim2(UPPER, same key image)=Ok(()) | claim3(lower again)=Err(... DOUBLE_CLAIM: key image already used in this epoch)
```

So one key can claim twice in one epoch by re-casing the hex. The same mechanism applies to `privacy_spent_key_images` for confidential transfers (code citation only, not separately run).

**Remediation.** Make the consensus identity of a key image its 32 canonical bytes: decode, require exactly 32 bytes, require canonical Ristretto decompression and non-identity, and store `[u8; 32]` (or lowercase-normalised hex) in the set. Reject non-lowercase hex or exact-length mismatches at payload parse. For `reward_claim_key_images` key on `(epoch, [u8;32])`. Add regression tests for upper/mixed-case, 33-byte, and `0x`-prefixed inputs. Because the set is part of the state hash (`push_sorted_string_set`, `state.rs:2715-2719`) and snapshots, changing its type is a consensus/snapshot-format change. It must not alter the validity or state hash of any historical block: it needs a migration plan and an owner-approved future upgrade boundary (see the compatibility rules after P1-2), and replay controls on old valid and invalid fixtures.

### P1-2: Privacy transitions are live in default builds, with no gate

**Description.** The brief assumed privacy is compiled out. In reality `wrfcoin-privacy` is a non-optional dependency of `shared-protocol`; `ConfidentialTransfer` and `RewardClaim` are in the exhaustive `AppliesState` dispatch; activation heights are 0. `native-chain`'s `privacy` Cargo feature only gates the HTTP routes and the registry. There is no `private_transfers` flag. Contrast `ShieldedDeposit/Withdrawal`, which are rejected at admission and in `verify_block` (`mempool/pool.rs:358-371`).

**Evidence.** Code citations in section 3.2. I did not verify whether the live testnet mempool admits these types.

**Remediation.** Two separate recommendations, which must not be merged into one change:

1. *Current-admission containment (no consensus change).* Decide per type whether the running testnet should stop **admitting** `ConfidentialTransfer` and `RewardClaim` into new blocks and mempools until P1-1 lands (admission and block-production policy, in the style of the #1762 shielded rejection at `mempool/pool.rs:358-371`). This changes what a node chooses to include, not what an already-valid block means.
2. *Future consensus gate (an upgrade, unscheduled here).* A `private_transfers` switch (default off) may be introduced only as a consensus change at an owner-approved future upgrade boundary, via the existing activation-schedule mechanism. Until that boundary, the existing rules apply unchanged.

**Historical-compatibility rules (apply to every remediation in this review).**

- Historical validity and state hashes are preserved. Every block accepted under the current rules stays valid, with the same state root, and every block rejected stays rejected. A reviewer must be able to replay from genesis and reach identical hashes.
- The existing activation behavior is retained for all historical heights: the activation heights that are 0 today stay 0 for the heights that exist today. They are not replaced by flag-derived values, and no Cargo feature or its default may change past validity. A new gate takes effect only at the future boundary, and is never evaluated retroactively.
- Note that on the default shared protocol the unmodified `RewardClaim` happy path succeeds at block 0 (independently reproduced by SOL-R, 2026-10-03). An unconditional "off" rejection inside `verify_block` or the state transition would therefore reject transactions the existing rules accept, and is a consensus change, not a containment.
- Any behavior change to validation or state needs: an owner-approved upgrade boundary (this document does not schedule one), a snapshot/state migration where the layout changes (P1-1), and replay controls that run old valid and old invalid fixtures through both the old and new code and require identical results below the boundary.

### P2-1: Malleability

**Description.** `verify` reduces `c` and every `s_i` with `from_bytes_mod_order`, so the encodings `x` and `x + l` verify identically (`ring_signatures.rs:443,461,473`). The signature is carried in consensus as hex-wrapped **JSON** (`state.rs:7628,7770`), whose byte form is not canonical (whitespace, field order, array-of-integer spelling). A relay can therefore re-spell a pending claim without invalidating it, changing `tx.data` and the transaction id.

**Evidence.** Scratch test: `c' = c + l`, `s0' = s0 + l` (same group elements, different bytes) gives `verifies = true; bytes differ = true`.

**Remediation.** Use `Scalar::from_canonical_bytes` for `c` and all `s_i` and reject otherwise; replace the JSON signature carriage with a constant-layout binary format (`key_image || c || s_0..s_{n-1} || ring`), with strict length and trailing-byte checks and `deny_unknown_fields` if JSON must remain. Never include a bincode form in a preimage (the consensus preimage in `transaction.rs:2574-2593` is already hand-framed and length-prefixed, which is correct and should be kept). Add the negative tests listed in section 8.

### P2-2: Verifier ring hygiene

**Description.** `verify` checks `n > 0` and `n <= MAX_RING_SIZE` only. Minimum size is enforced by `sign` (`:296`) and by two consensus callers, but not inside `verify`. Duplicate members are accepted, so a "ring of 7" can be seven copies of one key and meet the minimum with an anonymity set of 1. `sign` accepts a zero private key (the identity public key and identity key image), whereas `KeyImage::generate` rejects it (`:144`); the resulting signature verifies and anybody can produce it because the key is publicly known. `StationRegistration.ring_public_key` is copied into the registry without checking that it decodes or is non-identity (`transaction.rs:3368`, `station_registry.rs:107`).

**Evidence.** Scratch tests: ring-of-1 hand-built signature `verify = Ok(true)` (`MIN_RING_SIZE = 7`); six identical decoys sign and verify `Ok(true)`; `sign(&[0;32], ...)` returns `Ok`, `verify = Ok(true)`, `key_image_is_identity = true`.

**Remediation.** In a single strict verifier: ring size in `[MIN_RING_SIZE, MAX]`, pairwise-distinct members, no identity member, non-identity key image, canonical encodings. In `sign`: reject zero/non-canonical private keys and duplicate or identity decoys. Validate `ring_public_key` at station registration. Align `PrivacySettings::min_ring_size` with the enforced value.

### P2-3: Verification cost and a wall-clock budget in consensus code

**Description.** The doc comment at `ring_signatures.rs:29-33` claims worst-case verification "< 100 ms on constrained hardware (N100)". Measured on a 5950X: **320 ms for a 1024-member ring** (about 0.31 ms per member). An N100 is substantially slower per core, so the real bound is a multiple of this. The budgeted pre-pass in `mlsag_budget.rs` uses `Instant::now()` to decide which signatures to verify early; the remainder are re-verified in the state transition, so the final verdict should not depend on the clock, but the work done, the point of failure, and the log/metric output do vary by machine speed, and the apply-phase verification has no budget at all.

**Evidence.** Scratch test: `verify ring=1024 ok=true elapsed=319.9ms`.

**Remediation.** Lower `MAX_RING_SIZE` to what the fee schedule and block-time budget can actually carry (a small constant ring size such as 16 or 32 is common for this construction), keep `verify_cost.rs` pricing tied to measured cost, cap ring-signature transactions per block by a deterministic count rather than wall time, and delete or confine the wall-clock pre-pass so no consensus decision reads a clock. Re-measure on the target N-class hardware and record the numbers.

### P2-4: Unbound key images; state mutation before verification

**Description.** For `ConfidentialTransfer`, the signature's key image must appear **somewhere** in `payload.key_images` (`state.rs:7644-7656`), but every other listed image is inserted into the spent set without any proof of ownership (`:7686-7700`). A transaction that is otherwise valid can therefore mark arbitrary 32-byte values as spent, which is a grief/DoS primitive against any key image whose value is known or guessed in advance (and, with P1-1, arbitrary-length hex strings are accepted for those extra entries). For `RewardClaim`, the key image is inserted at step 4 (`:7751`) before the ring-signature check at step 10 (`:7810`).

**Evidence.** Scratch test at the transition level: a claim with an invalidated signature fails with `SIG_VERIFY_FAILED`, and an immediately following valid claim for the same key image then fails with `DOUBLE_CLAIM`. Whether this is reachable on a live node depends on whether callers discard the state when a transaction in a block fails; I did not verify that and the mempool path should be checked too.

**Remediation.** Require `key_images.len() == 1` (one signature, one image) or one signature per image; verify first and mutate last; make transitions atomic (apply to a scratch copy or record an undo log). Add a test that a failing claim leaves the state byte-identical.

### P2-5: Stealth derivation

**Description.** The shared secret is `SHA3-256(compress(rA))` with no domain tag and no output index (`stealth_addresses.rs:169-173`). Two outputs to the same recipient in one transaction that reuse `R` would derive the same one-time key, hence the same key image, and only one of them could ever be spent. The 8-byte payment-id pad is the first 8 bytes of the same secret that becomes the spend-key offset (`:181-187`), and the payment id has no authentication. Nothing requires one-time keys to be unique at the consensus layer.

**Remediation.** Derive `s = H("wrfcoin-stealth-v1/scalar" || rA || output_index)` and the payment-id pad from a separately domain-separated hash. Require unique one-time keys per output (and per chain) at consensus. Reject identity `A`, `B`, `R`. Document that a view key can detect and decrypt but cannot spend (this property **does hold** in the current code: spending needs `b`).

### P2-6: Key-image registry

**Description.** `is_spent` returns `false` when the lock is poisoned (`key_image_registry.rs:63-68`), which is the unsafe direction for a double-spend check. There is no `remove`, so a reorg cannot un-spend; the registry is in-memory only, so a restart forgets everything. `native-chain` keeps it in `AppState` while consensus keeps its own `privacy_spent_key_images` in `State`, so there are two sources of truth.

**Remediation.** Make consensus `State` the only authority (a canonical `[u8;32]` set, height-indexed for reorgs and pruning, covered by the state hash). Either delete `KeyImageRegistry` or make it a read-through cache that rejects by default.

### P2-7: HTTP routes hold keys

**Description.** `register_key` accepts a private key over HTTP; `generate_stealth_keys` returns both private keys in the response; `create_ring_signature` signs on the node using a server-held key (`routes/privacy.rs:181-230,457-502`). Keys sit in a plain `FxHashMap<String,(i64,[u8;32])>` with a TTL and are not zeroized on prune. Auth is the `Contributor` role only.

**Remediation.** Do not enable these routes in testnet builds under `private_transfers`. Signing and key generation belong in client tooling. If any route stays, it should only verify.

### P2-8: Value semantics

**Description.** `apply_confidential_transfer` inserts output commitments into a set; it debits nothing and credits nothing, and its ring is not required to consist of real outputs (the `RewardClaim` path does require members to be active stations; the confidential path does not). `RewardClaim` includes `range_proof` in the signed preimage but never verifies it (`state.rs:7806-7820`), and `amount_commitment` is only length-checked. This is acceptable for an inert feature but it means the ring signature currently proves membership of an arbitrary set, not entitlement to funds. This is the gap #1762 already identified for the shielded pool.

**Remediation.** Out of this review's cryptographic scope (commitments and range proofs); recorded here because switch-on requires an explicit ownership-and-value design: ring members bound to real unspent outputs, commitment balance verification, and range-proof verification on every output.

### P3 findings (summary)

- **P3-1.** Bind the ring public keys (or a hash of them) into every challenge, and reduce a 64-byte hash (`from_bytes_mod_order_wide`) instead of a 32-byte one. The signing preimage `tag || message || L || R || I` is unambiguous today because everything after the message has constant length.
- **P3-2.** Zeroize `x`, `alpha`, the `s_i`, ephemeral scalars, the shared secret and key arrays; implement `Drop` for `StealthKeys`; the crate already depends on `zeroize` with the `derive` feature. Dalek scalar multiplication is constant-time; there are no secret-dependent branches in sign, but `random_index` rejection-samples and the self-check compares challenges with `!=` (not secret-bearing, fine).
- **P3-3.** Reject identity keys in stealth APIs; authenticate the payment id or drop the field.
- **P3-4.** Bind `ephemeral_public_key` and `nonce` as AEAD associated data.
- **P3-5.** Return 400 (not 500) for malformed signatures and rate-limit/bound ring size on the verify route.
- **P3-6.** Use a keyed or `BTreeSet`-based structure for attacker-influenced keys.
- **P3-7.** Correct the "<100 ms" claim, align `min_ring_size`, and remove or use `EntropySource`/`RingSignatureParams` (an unused trait that suggests a pluggable RNG is a hazard if it is later wired in).
- **P3-8.** Add tests where a legitimate holder of the identity key signs (see P2-2); remediation of each item is described above.

## 5. Cryptographic construction notes (no defect found)

- **Scheme.** A single-layer LSAG-style linkable ring (the docs call it MLSAG). Signing computes `L_i = s_iG + c_iP_i`, `R_i = s_iHp(P_i) + c_iI`; verification recomputes the same chain and compares to `c_0`. Sign completes the ring correctly (`s_j = alpha - c_j x`) and self-checks before returning. Reviewed algebra is consistent, and a unit test covers closure at every signer position.
- **Group.** Ristretto255 (`curve25519-dalek 4.1`). Decoding rejects non-canonical and torsion encodings; the identity decodes successfully, so identity handling needs explicit checks (P2-2).
- **Hash-to-point.** `SHA3-512("wrfcoin-hash-to-point-v1" || compressed_P)` into `RistrettoPoint::from_uniform_bytes`: appropriate, domain-separated.
- **Key image.** `I = x * Hp(P)`, deterministic, and `KeyImage::generate` and `sign` produce the same value (tested). Uniqueness per key holds; the problem is how consensus **stores** it (P1-1).
- **Randomness.** All nonces, decoy positions, ephemeral keys and AEAD nonces come from `OsRng` (64-byte wide reduction for scalars; unbiased rejection sampling for the signer index). No `thread_rng`, no deterministic path, no nonce reuse observed.
- **Preimage.** The reward-claim authorization message is length-framed and chain-domain-bound (`transaction.rs:2574-2593`). No bincode appears in a preimage in the paths reviewed.
- **Stealth key separation.** `P = H(rA)G + B`, `x = H(aR) + b`; the view key alone cannot recover `x`.

## 6. Integration and determinism

- Floats: none in the privacy crate. HashMap iteration: sets are only used for membership; the consensus state hash sorts string sets (`push_sorted_string_set`). The one platform-dependent element is the wall-clock budget (P2-3).
- Platform-dependent behavior: dalek backend selection (u32/u64/AVX) must produce identical results; this is a library invariant, but add a golden-vector test across the CI architectures.
- DoS bounds: ring size capped at 1024 (too large, P2-3); no per-block cap on ring-signature transactions beyond fee pricing (`verify_cost.rs`); no cap on the JSON signature byte length before deserialization was observed.
- Reorgs: `privacy_spent_key_images_height` exists for pruning; confirm reorg rollback semantics when the state type changes (P1-1, P2-6).

## 7. Recommendation: compile-in

**Conditional NO-GO today; GO once P1-1 and P1-2 are merged.**

1. Compile-in as defined (feature built, behavior off behind a new unscheduled `private_transfers`) is only meaningful if "off" actually turns the existing live paths off for new activity. Today it would not (P1-2). Achieving that must preserve historical validity and state hashes: containment of current admission is separate from any consensus gate, which needs an owner-approved future upgrade boundary and replay controls (P1-2 compatibility rules). No privacy schedule exists in this document or its recommendations; this review changes no live behavior. What the live chain currently admits is unverified (section 3.2).
2. P1-1 is a demonstrated double-claim in code that is already in default builds; it should be treated as urgent regardless of the privacy schedule.
3. The primitives themselves are sound enough to compile in as a library; everything else is containable through the gate and the safe-subset rule.

### Prerequisites before switch-on

1. P1-1 and P1-2 merged, with the snapshot/state-hash migration for the key-image set, the P1-2 compatibility rules satisfied (historical validity, state hashes and activation-0 behavior preserved for existing heights), and replay controls on old valid and invalid fixtures passing.
2. All P2 items addressed or explicitly accepted by the owner: canonical encodings (P2-1), ring hygiene (P2-2), measured and re-bounded cost (P2-3), atomic state transitions and a single key-image authority (P2-4, P2-6), stealth derivation change (P2-5), no server-side key custody (P2-7), ownership/value design (P2-8).
3. A canonical binary signature format with golden vectors checked on every CI architecture.
4. Negative-test suite from section 8 landed and in CI.
5. A cost measurement on the target hardware class, with a per-block ring-signature cap.
6. An external cryptographer review of the final construction (core4#1617 tracks audit scope); this review is not a substitute.

## 8. Test coverage

Existing (all pass): sign/verify across every signer position, wrong message, tampered response, empty ring, response-count mismatch, short and long scalar lengths, off-curve and order-8 torsion ring members, identity member and identity key image on a **mutated valid signature**, ring above `MAX_RING_SIZE`, key-image determinism and registry double-register, concurrent registry use, stealth round trip, view-key scan, and wrong-recipient rejection.

Missing negative tests (recommended additions, none committed here):

| Gap | Relates to |
|---|---|
| Non-canonical `c` / `s_i` (`x + l`) must be rejected | P2-1 |
| Upper-case / mixed-case / over-long key-image hex must not create a second spent entry | P1-1 |
| Ring below `MIN_RING_SIZE` rejected by `verify` itself | P2-2 |
| Duplicate ring members rejected | P2-2 |
| Zero private key rejected by `sign` | P2-2 |
| Identity key image from a **legitimate** zero-key signer rejected | P2-2, P3-8 |
| Signature valid for ring A is not valid after substituting a different ring | P3-1 |
| Wrong ring member (real key swapped for a decoy) rejected | coverage |
| Failed claim leaves state unchanged | P2-4 |
| Reorg removes spent key images | P2-6 |
| Stealth: two outputs to one recipient yield distinct one-time keys | P2-5 |
| Cross-architecture golden vectors for key image and challenge | determinism |

## 9. Limits of this review

- Live behavior of the running testnet (whether `RewardClaim`/`ConfidentialTransfer` are admitted there) was not checked.
- Caller behavior on a failed transition (state discard vs partial mutation) was not traced end to end.
- Timing figures are from one workstation, not the N-class target.
- No fuzzing and no side-channel measurement were performed; constant-time claims rest on dalek's documented guarantees plus source inspection.

## 10. Tracking issues

| Finding | Issue |
|---|---|
| P1-1 | core4#3407 |
| P1-2 | core4#3408 |
| P2-1 | core4#3409 |
| P2-2 | core4#3410 |
| P2-3 | core4#3411 |
| P2-4 | core4#3412 |
| P2-5 | core4#3413 |
| P2-6 | core4#3414 |
| P2-7 | core4#3415 |
| P2-8 | core4#3416 |

P3 items are listed here only and have no issues yet.
