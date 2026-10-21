# Cryptographer Review of Privacy Primitives — Scope

**Status:** Draft scope for human review
**Target:** Pre-mainnet domain-expert review of `wrfcoin-privacy` primitives
**Refs:** #717 (launch blocker, human-required)
**Sibling scopes:** #716 / `docs/audit/2026-04-privacy-zk-audit-scope.md` (broader privacy + ZK firm engagement), #720 / `docs/consensus/bft-safety-analysis-scope.md` (BFT safety)

---

## 1. Why a separate cryptographer engagement

Issue #716 commissions a generalist audit-firm engagement covering the
privacy crate **and** the Groth16 ZK layer. Issue #717 is narrower and
deeper: a single domain-expert cryptographer (or 2-3 person team) should
review the **mathematical soundness** of the four privacy primitives that
were rewritten or substantially modified by AI agents:

1. Schnorr OR-proof range proof (`range_proof.rs`, rewritten in PR #713,
   replaces the abandoned `bulletproofs` crate)
2. MLSAG linkable ring signatures (`ring_signatures.rs`, hardened in
   #708 — 22 reachable `unwrap()`s removed)
3. Dual-key stealth addresses (`stealth_addresses.rs`)
4. Pedersen commitments (`pedersen.rs`)

The #716 firm engagement is application-security flavored: input
validation, transcript binding, side-channel hygiene, RPC boundary,
state-root determinism. The #717 cryptographer engagement is
construction-flavored: **does the math hold?** Are the schemes the
correct sigma-protocol / hash-to-point / OR-proof / DLEQ constructions
the literature describes? Was anything subtly mis-translated when the
AI rewrites happened? The two engagements overlap deliberately — #717
should produce a written security argument for each primitive that
#716 can lean on.

This doc enumerates the surface, the threat model, the precise
mathematical properties that need to be argued, and the deliverables.

---

## 2. In-scope files

All paths relative to `wrfcoin/core4`. The `privacy/` crate is
`#![forbid(unsafe_code)]`; total in-scope LOC ~2,100.

| File | LOC | Construction |
|------|-----|--------------|
| `privacy/src/pedersen.rs` | 493 | Pedersen commitments over Ristretto255: `C = vG + rH`, generators derived via SHA-512 nothing-up-my-sleeve hash-to-point |
| `privacy/src/range_proof.rs` | 310 | 64-bit range proof via per-bit Schnorr OR-proof (`b ∈ {0,1}`), Fiat-Shamir over `merlin::Transcript`, label `wrfcoin-range-proof-v1` |
| `privacy/src/ring_signatures.rs` | 797 | MLSAG linkable ring signature over Ristretto255 + SHA3, key image `I = x · Hp(P)`, `MIN_RING_SIZE = 7` |
| `privacy/src/stealth_addresses.rs` | 498 | Monero-style dual-key stealth addresses (spend key + view key), ECDH shared secret, address derivation via SHA3 |
| `privacy/src/payload_encryption.rs` | — | ChaCha20-Poly1305 AEAD wrapper (in scope only as the consumer of stealth shared secrets) |
| `privacy/src/key_image_registry.rs` | — | In-memory `FxHashSet` double-spend registry (in scope for replay-soundness argument only) |
| `privacy/src/mlsag_budget.rs` | — | Wall-clock-budgeted batch MLSAG verify (in scope for "fast path cannot accept invalid" argument) |
| `privacy/src/types.rs`, `error.rs` | — | Type definitions, error enum (in scope for serialization soundness) |

Pinned dependencies (`privacy/Cargo.toml`):
- `curve25519-dalek 4.1` — Ristretto255 group operations (constant-time at the library level)
- `merlin 3.0` — Fiat-Shamir transcript
- `chacha20poly1305 0.10` — AEAD
- `sha3` — keccak family (used inside MLSAG and stealth derivation)
- `rand_core 0.6` — RNG abstraction (production code uses `OsRng`)
- `zeroize 1.7` — secret-material zeroization

`Cargo.lock` will be provided at engagement kickoff for exact pinning.

---

## 3. Threats in scope

The cryptographer is asked to argue, in writing, that each of the
following holds for each primitive. Negative findings — anything that
*doesn't* hold — are the engagement deliverable.

### 3.1 Pedersen commitments
- **Binding**: under the discrete-log assumption on Ristretto255, no
  PPT adversary can produce two openings `(v, r) ≠ (v', r')` of the
  same commitment.
- **Hiding**: commitments are uniformly distributed in the group given
  uniform `r`, so they statistically hide `v`.
- **Generator independence**: `H` is derived via SHA-512 hash-to-curve
  in a way that makes `log_G(H)` unknown — verify the construction is a
  valid nothing-up-my-sleeve (NUMS) generator and that the
  hash-to-curve map preserves uniformity.

### 3.2 Schnorr OR-proof range proof (`range_proof.rs`)
- **Soundness**: a verifier accepts only if every bit commitment opens
  to a value in `{0, 1}` — i.e. the OR-proof for each bit is sound.
- **Bit-decomposition correctness**: the verifier enforces
  `Σ_{i=0..63} C_i · 2^i == C`, so the committed value is in `[0, 2^64)`.
- **Zero-knowledge**: the proof leaks nothing about the value beyond
  its range. Argue this for the simulator: given the public commitment
  and the challenge, can the simulator produce indistinguishable
  transcripts?
- **Fiat-Shamir transcript binding**: every public input — bit
  commitments, the value commitment `C`, generators, and the protocol
  label `wrfcoin-range-proof-v1` — must be absorbed into the Merlin
  transcript before the challenge is squeezed. Forgery via challenge
  reuse must be impossible.
- **Translation correctness**: PR #713 rewrote this from `bulletproofs`
  to a hand-rolled Schnorr OR-proof. Confirm the rewrite is a faithful
  implementation of the textbook construction (cite the paper or HAC
  chapter). Flag any deviation, however small.

### 3.3 MLSAG linkable ring signatures (`ring_signatures.rs`)
- **Unforgeability**: under the discrete-log assumption, no PPT
  adversary lacking any of the ring's secret keys can produce a valid
  signature.
- **Anonymity**: signature does not reveal which ring member signed it
  beyond probability `1 / |ring|`.
- **Linkability**: two signatures from the same secret key under the
  same key-image domain produce the same key image `I = x · Hp(P)` —
  argue this enforces double-spend protection at the cryptographic
  layer (independent of the application-level `KeyImageRegistry`).
- **Hash-to-point soundness**: `Hp(P)` must be a proper hash-to-point
  (e.g. Elligator2 or "try-and-increment" with rejection), not
  hash-then-decompress. Verify the implementation does not accept
  invalid points or skip the cofactor clearing.
- **Ring well-formedness**: verify the signer rejects rings that
  contain duplicate public keys, the identity element, or low-order
  points. `MIN_RING_SIZE = 7` is the policy floor — confirm it is
  enforced on every code path that constructs a signature.
- **Key-image uniqueness across rings**: a signer holding key `x`
  must produce the *same* key image regardless of which ring they
  sign in.
- **Constant-time signing**: the signing routine must not branch on
  secret bits (signer index, secret scalar). Verify the signer-index
  loop is data-oblivious, or document why it's not exploitable.

### 3.4 Stealth addresses (`stealth_addresses.rs`)
- **One-time-address unlinkability**: given the recipient's public
  spend and view keys, an attacker cannot link two distinct stealth
  outputs to the same recipient.
- **Address-derivation soundness**: the per-output key
  `P_i = H(rA, i)·G + B` (or whatever formula the implementation
  uses — verify it) must be controllable only by the recipient.
- **Shared-secret security**: ECDH shared secret `rA` cannot be
  scalar zero, identity, or any low-order point. Verify the
  implementation rejects these explicitly.
- **View-key segregation**: the view key alone must NOT permit
  spending. Verify there is no code path that derives a spend key
  from a view key.
- **Subaddress space (if implemented)**: each subaddress must be a
  pseudo-random function of the master view key and an index, with
  no collision or distinguishing attack feasible from the public
  data.

### 3.5 Cross-primitive
- **Domain separation**: every Merlin transcript label and every
  SHA3 prefix should be globally unique within the workspace.
  Cryptographer should grep workspace-wide and flag any collision.
  (Currently observed labels: `wrfcoin-range-proof-v1`,
  `wrfcoin-mlsag-v1`, `wrfcoin-stealth-v1` — confirm uniqueness.)
- **Randomness sources**: production code uses `rand_core::OsRng`
  exclusively. Verify there is no seeded RNG, no `thread_rng()`,
  no test-only RNG reachable from production paths.
- **Scalar reduction and canonical encoding**: every byte → scalar
  conversion must reduce mod `l`; every byte → group point must
  reject non-canonical encodings.
- **Zeroization completeness**: every secret-key, ephemeral-scalar,
  blinding-factor type must `zeroize` on drop. Currently only
  `pedersen.rs` uses `Zeroize` — flag the gap and propose minimal
  additions.

---

## 4. Threats out of scope

- **Implementation-level timing inside `curve25519-dalek` / `sha3` /
  `chacha20poly1305`** — pinned versions, treated as audited-elsewhere.
  Cryptographer should still flag non-constant-time *usage* in
  `privacy/` code (e.g. `==` on byte slices, `HashMap` lookups keyed
  on secret bytes).
- **Groth16 / BN254 ZK circuit layer** — covered by #716. The
  cryptographer engagement may *opt in* to reviewing the
  in-circuit range proof in `zk-circuits/src/circuits/range_proof.rs`
  but it is not required.
- **Solidity contracts** — separate contract audit.
- **Post-quantum schemes** (KYBER, DILITHIUM, FALCON, SPHINCS+, XMSS) —
  archived in `wrfcoin/blockchain-core`, not deployed.
- **HSM integration** and key custody at the hardware boundary —
  separate review track.
- **Consensus economic security** — #720.
- **Application logic** that consumes the primitives (state apply,
  reward distribution, mempool admission) — overlap with #716, not
  the cryptographer's primary scope. Cryptographer should call out
  *contractual* misuses (e.g. an application that calls a primitive
  outside its security parameters) but not chase down every consumer.

---

## 5. Reference cryptosystems and security parameters

Auditor should treat these as the reference baseline; deviations are
findings.

| Scheme | Reference | Security parameter | Notes |
|--------|-----------|---------------------|-------|
| Pedersen commitments | Pedersen 1991, "Non-Interactive and Information-Theoretic Secure Verifiable Secret Sharing" | DL on Ristretto255 (~128 bits) | Binding under DL, statistically hiding |
| Schnorr OR-proof | Cramer-Damgård-Schoenmakers 1994, "Proofs of Partial Knowledge and Simplified Design of Witness Hiding Protocols"; standard in HAC §10.4 | ~128 bits (Ristretto255 + Merlin/keccak) | OR for `b ∈ {0, 1}` per bit |
| MLSAG | Noether-Mackenzie 2016, "Ring Confidential Transactions"; Monero implementation | ~128 bits | Linkable spontaneous anonymous group sig |
| Stealth addresses | van Saberhagen 2013, CryptoNote whitepaper §4.4 | ~128 bits | Dual-key spend/view variant |
| Hash-to-point | Should be Elligator2 or RFC 9380 hash-to-curve; "try-and-increment" acceptable if cofactor cleared | n/a | Implementation choice — verify, do not assume |
| ChaCha20-Poly1305 | RFC 8439 | 256-bit key, 96-bit nonce | Used only as a sealed-payload primitive after stealth-derived shared secret |
| Fiat-Shamir | Merlin transcripts (keccak-based STROBE) | n/a | Transcript binding is the audit's load-bearing assumption |

If the implementation diverges from any of the above (different
challenge derivation, different key-image construction, different
shared-secret formula), the cryptographer should produce a written
security argument or flag it as a finding.

---

## 6. Required deliverables

From the cryptographer engagement:

1. **Per-primitive security argument.** A short (~2-5 page) written
   argument for each of `pedersen`, `range_proof`, `ring_signatures`,
   `stealth_addresses` covering: scheme identification, deviation
   (if any) from the reference construction, security claim,
   reduction sketch, and a list of underlying assumptions
   (DL, RO, etc.).
2. **Findings report.** Severity-graded (Critical / High / Medium /
   Low / Informational), with reproduction steps for Critical/High
   (Rust test, transcript log, or proof-of-concept exploit).
   Remediation guidance must be code-level (line-number specific
   when possible).
3. **Test vectors.** Independently-derived test vectors (at least 3
   per primitive) to be checked into `privacy/tests/audit_vectors/`
   and run as part of `cargo test -p wrfcoin-privacy`.
4. **Rewrite review note for `range_proof.rs`.** Explicit confirmation
   (or refutation) that PR #713's rewrite is a faithful Schnorr OR-proof
   construction. This is the highest-load-bearing single item in the
   engagement.
5. **`unwrap()` regression check.** Issue #716 originally cited 22
   reachable `unwrap()`s in `ring_signatures.rs` (PR #708). Confirm
   that no panic-on-secret-data path remains anywhere in the privacy
   crate.
6. **Disclosure coordination.** 30-day embargo default, CVE assignment
   where applicable.
7. **Re-test pass.** After the team lands remediations, the
   cryptographer re-runs the test vectors and confirms each Critical
   and High finding is closed.

---

## 7. Reference artifacts for the cryptographer

- `privacy/Cargo.toml`, `Cargo.lock` — exact pinning
- `privacy/tests/privacy_e2e.rs` (417 LOC) — end-to-end test (entry
  point for understanding intended use)
- `privacy/src/tests/mod.rs` — current unit-test module (minimal;
  cryptographer is expected to expand it as a deliverable)
- PR #713 — Schnorr OR-proof rewrite of `range_proof.rs`
- PR #708 — `unwrap()` cleanup in `ring_signatures.rs`
- Issue #716 — broader privacy/ZK firm audit (sibling engagement)
- Issue #717 — this engagement (launch blocker, human-required)
- `docs/audit/2026-04-privacy-zk-audit-scope.md` — sibling scope doc
- `docs/security/SECURITY.md` — platform security posture

---

## 8. Open questions for human review

- [ ] **Reviewer selection.** Solo academic cryptographer (e.g. via
      consultant marketplace) vs. small specialist firm (e.g. Cure53,
      X41, Trail of Bits cryptography practice, Least Authority,
      NCC Group cryptography services). Solo is cheaper, faster, and
      adequate for this surface; a firm gives a second pair of eyes
      and easier disclosure handling.
- [ ] **Budget envelope.** Estimate: 4-6 weeks of one senior
      cryptographer's time for a focused review of ~2,100 LOC of
      Rust crypto code. Pricing varies wildly with reviewer; human
      decision.
- [ ] **Overlap with #716.** Should the cryptographer be hired
      *before* the #716 audit firm so that #716 can rely on the
      written security arguments? Default: yes — sequence is
      #717 cryptographer → #716 audit firm → #716 fix-verification.
- [ ] **Test-vector publication.** Test vectors should ship in the
      repo. Confirm no IP / NDA blocker on republishing them.
- [ ] **Remediation budget.** A finding rate of 5-15 medium-or-worse
      issues across 4 primitives is plausible. Reserve 1-2 weeks of
      Alice/core-protocol persona time after the report lands.
- [ ] **Pre-engagement self-review.** Should the team run an internal
      pass before kickoff to clean obvious nits? Decision: yes — file
      as a separate sub-task; do NOT delay the engagement on it.

---

Refs #717
