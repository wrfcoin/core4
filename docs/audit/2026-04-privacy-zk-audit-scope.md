# Privacy & ZK Third-Party Security Audit — Scope

**Status:** Draft scope for human review
**Target:** Pre-mainnet external audit of privacy subsystem and Groth16 ZK layer
**Refs:** #716

---

## 1. Audit target and motivation

WRFCoin's privacy subsystem (Pedersen commitments, Schnorr OR-proof range
proofs, MLSAG ring signatures, stealth addresses, payload encryption) and its
Groth16 / BN254 ZK circuit layer (confidential transfer, selective disclosure,
weather model) are currently under a **single-party trusted setup** and have
been reviewed only by AI agents and internal contributors. Launch blocker #716
requires a professional audit by a firm with deep cryptography experience
before mainnet. This scope doc enumerates the exact code surface, entry
points, assumptions, and deliverables so a firm can quote the engagement.

---

## 2. In-scope crates and modules

All paths are relative to the `wrfcoin/core4` workspace root.

### `privacy/` — `wrfcoin-privacy` crate

Standalone primitives with `#![forbid(unsafe_code)]`.

| File | Purpose |
|------|---------|
| `privacy/src/pedersen.rs` | Pedersen commitments over Ristretto255 |
| `privacy/src/range_proof.rs` (310 LOC) | 64-bit range proof via Schnorr OR-proof bit decomposition with `merlin::Transcript` Fiat-Shamir; **rewritten in PR #713** to drop the `bulletproofs` crate |
| `privacy/src/ring_signatures.rs` (797 LOC) | MLSAG over Ristretto255 + SHA3, key image derivation, `MIN_RING_SIZE = 7` |
| `privacy/src/stealth_addresses.rs` (498 LOC) | Monero-style dual-key stealth addresses (spend + view) |
| `privacy/src/payload_encryption.rs` | ChaCha20-Poly1305 AEAD wrapper |
| `privacy/src/key_image_registry.rs` | In-memory `FxHashSet` double-spend registry |
| `privacy/src/mlsag_budget.rs` | Wall-clock-budgeted batch MLSAG verify (50 ms default, remainder deferred to state apply) |
| `privacy/src/error.rs`, `types.rs` | Error enum + privacy level config |

Pinned deps (`privacy/Cargo.toml`): `curve25519-dalek 4.1`, `merlin 3.0`,
`chacha20poly1305 0.10`, `sha3`, `rand_core 0.6`, `zeroize 1.7`.

### `zk-circuits/` — `wrfcoin-zk-circuits` crate

| File | Purpose |
|------|---------|
| `zk-circuits/src/proving.rs` | Groth16 `generate_keys` / `prove` / `verify` / `batch_verify` helpers, randomized linear combination for batch check |
| `zk-circuits/src/circuits/range_proof.rs` | In-circuit 64-bit range check |
| `zk-circuits/src/circuits/confidential_transfer.rs` | Balance-preserving transfer circuit: Poseidon commitment openings, per-value range checks, blinding conservation |
| `zk-circuits/src/circuits/selective_disclosure.rs` | 3-field selective disclosure circuit |
| `zk-circuits/src/circuits/weather_model.rs` | 2-variable weather model inference proof |
| `zk-circuits/src/circuits/poseidon_config.rs` | BN254 Poseidon sponge config (width, capacity, rounds) used by circuits |

Dependencies of note (`zk-circuits/Cargo.toml`): `ark-groth16`, `ark-bn254`,
`ark-relations 0.4`, `ark-r1cs-std 0.4`, `ark-crypto-primitives 0.4`, `ark-snark`,
`ark-serialize`. All arkworks crates are pinned via workspace dependencies.

### `native-chain/src/zk_service.rs`

Runtime loader for embedded Groth16 key bundles. Uses `OnceLock` caches per
circuit (`RANGE_PROOF_KEYS`, `RANGE_PROOF_32_KEYS`, `CONFIDENTIAL_TRANSFER_KEYS`,
`SELECTIVE_DISCLOSURE_KEYS_3`, `WEATHER_MODEL_KEYS_2`) and a SHA-256
`bundle_fingerprint()` for on-disk cache filenames.

### `native-chain/zk-params/`

Compile-time-embedded Groth16 proving/verifying key bundles. `PROVENANCE.md`
documents SHA-256 hashes and the single-party trusted-setup method.

| File | Circuit |
|------|---------|
| `range-proof-64.bin` | 64-bit range proof |
| `confidential-transfer-1-1-64.bin` | 1-input 1-output confidential transfer |
| `selective-disclosure-3.bin` | 3-field selective disclosure |
| `weather-model-2.bin` | 2-variable weather model proof |

### `shared-protocol/src/blockchain/state.rs`

Shielded pool state machine. Fields relevant to privacy/ZK (see `state.rs:985-1006`):

- `privacy_commitments: FxHashSet<String>` (+ height map for pruning)
- `privacy_spent_key_images: FxHashSet<String>` (+ height map)
- `privacy_spent_nullifiers: FxHashSet<String>` (+ height map)
- `privacy_disclosure_ids: FxHashSet<String>` (+ height map)

State transitions that touch the privacy/ZK layer:
`StateTransition::ApplyConfidentialTransfer`,
`StateTransition::ApplyShieldedDeposit`,
`StateTransition::ApplyShieldedWithdrawal`,
`StateTransition::ApplyRewardClaim`
(all defined around `state.rs:2799-2819`). All four pools participate in the v5
state-root material feed via `push_sorted_string_set` (`state.rs:1943-1946`).

### `shared-protocol/src/consensus/proof_of_publishing.rs` (11k LOC)

Reward distribution, slashing, liveness. Listed in #716 because `ApplyRewardClaim`
is a major MLSAG consumer and nullifier uniqueness must hold for slashing to be
sound. Treat PoP only at the boundary where it reads/writes privacy state —
full PoP review overlaps with #720 and is out of scope here.

### `shared-protocol/src/consensus/zk_verification.rs`

`ZkVerifier` trait consumed by `native-chain::zk_service`.

---

## 3. Entry points

### HTTP API (`native-chain/src/bin/launch_api/routes/privacy.rs`, 539 lines)

| Handler | Function |
|---------|----------|
| `register_key` | Register a privacy keypair in the runtime keystore |
| `generate_stealth_keys`, `create_stealth_address`, `check_stealth_address` | Stealth address lifecycle |
| `create_commitment`, `verify_commitment` | Pedersen commitments |
| `create_range_proof`, `verify_range_proof` | Range proofs (privacy-crate Schnorr OR variant) |
| `create_ring_signature`, `verify_ring_signature` | MLSAG ring signing |
| `encrypt_payload`, `decrypt_payload` | ChaCha20-Poly1305 AEAD |
| `get_key_images` | Inspect spent key images |

### HTTP API (`native-chain/src/bin/launch_api/routes/zk.rs`, 354 lines)

| Handler | Function |
|---------|----------|
| `generate_range_proof`, `verify_range_proof` | Groth16 64-bit range proof |
| `generate_confidential_transfer`, `verify_confidential_transfer` | ConfidentialTransfer circuit |
| `generate_selective_disclosure`, `verify_selective_disclosure` | SelectiveDisclosure circuit |
| `generate_weather_model_proof`, `verify_weather_model_proof` | WeatherModel circuit |
| `get_zk_stats`, `list_circuits` | Introspection |

### Transaction path

Privacy-bearing transactions enter the chain via the normal mempool → block
inclusion → state application path. `state.rs::apply_state_transition` dispatches
to `apply_confidential_transfer`, `apply_shielded_deposit`,
`apply_shielded_withdrawal`, `apply_reward_claim`. Pre-block MLSAG verification
uses the budgeted pre-pass in `mlsag_budget.rs`; final verification happens inside
the apply functions.

### CLI

`tools/cli/WrfcoinCLI/` exposes privacy-related subcommands (out of scope for code
review but in scope for **input-validation review** at the CLI → HTTP boundary).

---

## 4. Threat model assumptions

Any assumption below that fails in practice is a finding.

1. **Adversary.** Network-level attacker can observe, reorder, drop. Can
   collude up to `f` validators (`f` bound comes from consensus layer, #720).
   Cannot compromise OS CSPRNG.
2. **Randomness.** All ephemeral scalars, blinding factors, and challenges
   come from `rand_core::OsRng`. Confirm no `rand::thread_rng()` or seeded
   RNG in production code paths.
3. **Trusted setup.** Embedded Groth16 keys are single-party `OsRng` setup
   (see `PROVENANCE.md`). This is an **acknowledged** pre-mainnet gap
   tracked by #717 — the audit should NOT treat single-party setup itself
   as a finding, but MUST verify that rotation is cleanly possible
   (key-hash wiring, hard-fork activation hooks).
4. **Curves.** Ristretto255 (`curve25519-dalek 4.1`) for non-ZK primitives;
   BN254 (`ark-bn254`) for Groth16. Flag any scalar mixing and any use that
   relies on BN254's ~100-bit post-advancement security for long-term secrecy.
5. **Side channels.** Attacker can time RPC responses and may share
   hardware with a validator. `curve25519-dalek` is constant-time at the
   library level; higher-level code (byte comparisons, hash-set lookups)
   may leak.
6. **Transcript domain separation.** Each proof uses a distinct Merlin
   label (e.g. `wrfcoin-range-proof-v1`). Verify uniqueness workspace-wide.
7. **Determinism.** State-root v5 hashes privacy sets via
   `push_sorted_string_set`. Verify every privacy mutation is captured and
   no platform-dependent iteration order leaks in.

---

## 5. Out of scope

- **Solidity contracts** in `wrfcoin/smart-contracts` — separate contract audit.
- **Post-quantum crypto** (KYBER/DILITHIUM/FALCON/SPHINCS+/XMSS) — lives in archived `wrfcoin/blockchain-core`, not deployed.
- **HSM / key-management hardware** — separate review track.
- **Non-privacy consensus** (PoW, PoS selection, partition recovery, BFT safety) — #720 / separate engagement.
- **Upstream crypto libraries.** `curve25519-dalek`, `ark-*`, `chacha20poly1305`, `merlin`, `sha3` are pinned and treated as audited-elsewhere; verify **usage**, not internals. `Cargo.lock` provided at kickoff.
- **L2** (`layer2/`) and **EVM compat** (`evm/`) — no privacy interaction.
- **Weather IoT ingestion** except where it crosses `weather_model.rs`.
- **Single-party setup being single-party** — acknowledged gap (see §4.3).

---

## 6. Review checklist

Starting ammunition, not exhaustive. Auditor should produce findings against
at least these items.

### 6.1 Cryptographic soundness
- [ ] Schnorr OR-proof in `range_proof.rs` matches sigma-protocol literature; Fiat-Shamir binds all public values.
- [ ] Each bit commitment `C_i` opens only to 0 or 1 (OR-proof soundness).
- [ ] Bit-decomposition sum `Σ C_i · 2^i == C` enforced on every verification path.
- [ ] MLSAG key image `I = x · Hp(P)` uses a proper hash-to-point (not hash-then-decompress).
- [ ] Ring signature verification rejects malformed / out-of-order public keys.
- [ ] Stealth-address shared secret cannot produce scalar 0 / identity point for any valid ephemeral key.
- [ ] Pedersen blinding factors sampled uniformly from the full scalar field.
- [ ] Groth16 `verify` uses `PreparedVerifyingKey` correctly; no pairing skips.
- [ ] `batch_verify` RLC uses fresh randomness; cannot be biased by attacker proofs.
- [ ] Poseidon sponge config in `poseidon_config.rs` matches a published BN254 parameter set.
- [ ] ConfidentialTransfer circuit enforces value balance AND blinding conservation (`confidential_transfer.rs:21-25`).
- [ ] SelectiveDisclosure leaks nothing about undisclosed fields.
- [ ] WeatherModel does not let prover choose inputs after seeing public inputs.

### 6.2 Implementation-level hardening
- [ ] No `unsafe` in `privacy/` (check `#![forbid(unsafe_code)]` is respected; no workarounds).
- [ ] No `unwrap()` / `expect()` / `panic!()` reachable from RPC handlers. Issue #716 originally cited 22 `unwrap()` calls in `ring_signatures.rs`; current grep shows 0 — re-verify and extend to rest of privacy crate and `zk_service.rs`.
- [ ] All public-input comparisons use constant-time equality. (Grep for `subtle::ConstantTimeEq` = 0 hits — explicit verify-or-flag item.)
- [ ] `Zeroize` / `ZeroizeOnDrop` on every private-key / scalar type that touches the heap. (Currently only in `pedersen.rs`.)
- [ ] Serde deserialization enforces length and curve-point validity before any cryptographic op.
- [ ] Merlin transcript labels globally unique and versioned (`wrfcoin-range-proof-v1`, etc.).
- [ ] MLSAG budget pre-pass (`mlsag_budget.rs`) cannot skip validation — every signature verified in pre-pass or state apply, no silent drops.
- [ ] `KeyImageRegistry` handles concurrent writes; no TOCTOU between `is_spent` and `register`.

### 6.3 State and consensus interaction
- [ ] Double-spend detection via key-image and nullifier sets is correct and persistent across restarts.
- [ ] State-root material (`push_sorted_string_set`) is deterministic regardless of insertion order, host OS, or allocator.
- [ ] Pruning via `_height` maps cannot remove a live nullifier or commitment.
- [ ] ApplyRewardClaim validates MLSAG signatures before crediting rewards.
- [ ] Shielded deposit/withdrawal balance checks cannot underflow or double-credit across the transparent ↔ shielded boundary.

### 6.4 Trusted setup and key rotation
- [ ] Embedded `.bin` files match SHA-256 hashes in `PROVENANCE.md` at build time.
- [ ] `bundle_fingerprint()` in `zk_service.rs` correctly detects tampering and invalidates cache.
- [ ] Key-rotation hard-fork plan in `PROVENANCE.md` is actually wired — a code path exists to load a post-MPC verifying key at an activation height without breaking historical verification.
- [ ] Proving keys never cross an RPC boundary from untrusted sources.

### 6.5 RPC / API boundary
- [ ] Every handler in `privacy.rs` and `zk.rs` validates input sizes, ring-size bounds (`MIN_RING_SIZE = 7`), and curve-point validity before calling into the privacy crate.
- [ ] No handler returns a cryptographic error string verbatim in a way that leaks secret state.
- [ ] Rate limiting / quota on proof-generation endpoints (link to #1244 token quota work) is adequate against resource exhaustion.

---

## 7. Known weaknesses / self-flagged concerns

Items the core team already worries about — treat as prompts to dig deeper.

1. **Single-party trusted setup.** See `PROVENANCE.md`. MPC tracked in #717.
2. **BN254 security margin.** Post-advancement effective bits ~100; adequate short-term, not for multi-decade secrecy.
3. **No `subtle` crate usage.** High-level code may leak via non-constant-time byte comparisons.
4. **Hand-rolled range proof.** `range_proof.rs` Schnorr OR construction (PR #713) has not been independently reviewed.
5. **MLSAG budget pre-pass semantics.** Two verification paths — audit must confirm a malformed signature cannot take the fast path.
6. **In-memory key-image registry.** Crash recovery replays state machine; audit for replay-vs-ingest races.
7. **`unwrap()` regression risk.** #716 originally cited 22 unwraps in `ring_signatures.rs` (PR #708); current grep = 0. Confirm and flag any that migrated elsewhere.
8. **Transcript uniqueness is unenforced.** Labels like `wrfcoin-range-proof-v1` exist but no workspace-wide check.
9. **Poseidon parameters.** Using `ark-crypto-primitives 0.4` defaults — not independently validated for BN254.

---

## 8. Deliverables

From the audit firm:

1. **Written report** covering every checklist item. Severity (Critical / High / Medium / Low / Informational), reproduction steps for Critical/High (Rust tests or curl transcripts), code-level remediation guidance.
2. **Executive summary** suitable for public release post-remediation.
3. **Fix-verification pass** after Alice's team lands remediations — re-test of all Critical/High items.
4. **Supply-chain attestation.** Record of in-scope crate versions (pinned via `Cargo.lock`) with CVE notes.
5. **Disclosure coordination.** 30-day embargo default, CVE assignment where applicable.

---

## 9. Reference artifacts

- `privacy/Cargo.toml`, `zk-circuits/Cargo.toml` — dependency pinning
- `privacy/tests/privacy_e2e.rs` (417 LOC) — end-to-end privacy flow test
- `zk-circuits/src/tests/weather_test.rs` (74 LOC) — ZK weather circuit smoke test
- `privacy/src/tests/mod.rs` — unit test module (currently minimal; auditor
  should expand as part of finding reproductions)
- `native-chain/zk-params/PROVENANCE.md` — trusted setup provenance
- PR #713 — Schnorr OR-proof rewrite (range_proof.rs)
- Issue #708 — historical `unwrap()` cleanup in ring_signatures.rs
- Issue #716 — this audit (launch blocker, human-required)
- Issue #717 — MPC ceremony planning (follow-on)
- Issue #964 — ZK trusted setup uses single-party `OsRng`
- `docs/security/SECURITY.md` — platform-level security posture

---

## 10. TBD / open questions for human review

- [ ] **Firm selection.** Trail of Bits / NCC Group / OpenZeppelin / Consensys Diligence / Halborn / Least Authority. Human decision, weighted against budget + calendar.
- [ ] **Budget and timeline.** TBD. A focused sub-scope of just the range-proof rewrite (PR #713) + confidential transfer circuit is tractable if full-scope budget is unavailable.
- [ ] **MPC ceremony coordination.** Could be bundled with this audit (#717) or kept separate. Human call.
- [ ] **Disclosure policy.** Default 30-day embargo — confirm with legal/PR.
- [ ] **Economic model scope split.** Boundary between this audit, #720 (consensus safety), and a potential economic-security engagement must be drawn at kickoff. #716 lists `proof_of_publishing.rs` and shielded-pool state paths as in-scope here.
- [ ] **Verifier upgrade path.** `PROVENANCE.md` calls for a hard fork on key rotation. Confirm the code actually supports this today, or mark as pre-audit blocker.

---

Refs #716
