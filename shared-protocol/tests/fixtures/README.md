# Adversarial crypto vectors (#2126)

Vendored — **not fetched at test time**. A network-dependent crypto test is a flaky crypto test.

Three sources, each named for what it is — the labels in the harness output match
these rows exactly, so a CI reader can tell which coverage came from where.

| Path | Algorithm | Source | Harness label | Vectors |
|---|---|---|---|---|
| `wycheproof/local/ed25519_project_controls.json` | Ed25519 | **Project-authored.** Generated with `python-cryptography` (an implementation **independent** of `ed25519-dalek`); malleability and identity-point cases derived arithmetically. **NOT the published Project Wycheproof corpus** | `[vectors:ed25519-local-controls]` | 16 |
| `wycheproof/upstream/ed25519_test.json` | Ed25519 | **Published Project Wycheproof**, vendored by #2282 at C2SP `b61843a9`. **Optional** — absent, the run records it in the coverage ledger rather than passing quietly | `[vectors:ed25519-wycheproof-upstream]` | 150 |
| `nist/sha2_sha3_fips.json` | SHA-256, SHA3-256 | **NIST** FIPS 180-4 / FIPS 202 published test messages. Not Wycheproof, and no longer filed under a `wycheproof/` path | `[vectors:hash-nist]` | 9 |

The local controls are **not** a placeholder for the upstream corpus. Their
`malleable_S_plus_L_*` and `identity_public_key` cases target this project's
`verify_strict` behaviour (#2043) and were built deliberately. They are a
complement; both run when both are present.

Each vector carries a `result` flag: `valid`, `invalid`, or `acceptable`. The harness
(`shared-protocol/tests/wycheproof.rs`) drives each through the **project's own wrapper** —
`PublicKey::verify_signature`, `sha256_hash`, `sha3_hash` — never the upstream crate. The question
is whether *this codebase* uses the primitive correctly, not whether the primitive is correct.

## The adversarial cases that matter

- `malleable_S_plus_L_*` — `S' = S + L` (S mod L unchanged). A non-strict verifier accepts these; WRFCoin's `verify_strict` (#2043) must reject them. This is the case an independent non-strict library (including the one that generated the corpus) would *accept*, which is exactly why it is here.
- `identity_public_key` — the all-zero (identity / small-order) public key must not verify an unrelated signature.
- `tampered_msg_*`, `tampered_sig_*`, `wrong_key_*` — ordinary negative controls.

The NIST file also contains one `invalid`-flagged vector (`wrong_expected_sha256`) with a
deliberately wrong expected digest, so the harness proves its comparison is live rather than
vacuously passing.

## Coverage is accounted for, not assumed

This corpus does **not** cover everything #2126 asked for. The harness emits an explicit ledger
(`UNSUPPORTED_CLASSES`) on every run naming each gap and why:

- **k256/ECDSA** — no project wrapper exists to drive; testing upstream directly is forbidden.
- **published Wycheproof corpus** — recorded as a gap only while `wycheproof/upstream/` is absent (#2264 / #2282). The ledger is computed from what is on disk, so it cannot go stale in either direction.
- **`acceptable`-flagged vectors** — zero in this corpus, so `ACCEPTABLE_POLICY` is defined but
  unexercised.

A harness printing a pass count while silently skipping a class overstates what it verified. The
reconciliation test fails if a row is removed while the gap remains, or left behind once it closes.

## `acceptable` policy

WRFCoin verifies ed25519 with `verify_strict`, so `acceptable` collapses to `invalid`
(`ACCEPTABLE_POLICY` in the harness). This is a recorded project decision, not an ad-hoc skip.

## k256 ECDSA — out of scope, recorded

The story lists k256, but there is **no k256 wrapper in `shared-protocol`** to test, and the story
forbids testing the upstream crate directly. Adding a k256 dependency here purely to test upstream
would violate that. Recorded as `K256_NO_PROJECT_WRAPPER` in the harness; fold k256 vectors in
wherever a project wrapper eventually lands.

## Regenerating

The generators live in this PR's description. Regeneration is deterministic (fixed seeds). Do
**not** regenerate the hash expected-values from the project's own hasher — they are independent
NIST constants, and deriving them from the code under test would defeat the point.
