# #1993 — ConfidentialTransfer selective-disclosure dual-verifier mismatch

**Date:** 2026-06-20  
**Branch:** bob/1993-confidential-transfer-verifier  
**Status:** Design split — requires human sign-off before any code change

---

## What is broken

The `SelectiveDisclosureProofPayload` has two incompatible implementations of the
binding digest and attestation model.  Any real client that produces a disclosure
via the public API will **always** fail consensus validation.  The state-layer tests
pass only because the test helper bypasses the public API.

---

## The two implementations

### Model A — producer side (transaction.rs)

**`SelectiveDisclosureProofPayload::binding_digest()`**  
`shared-protocol/src/blockchain/transaction.rs:1106–1113`

```rust
let scope_joined = self.scope.join(",");   // scope NOT sorted
let preimage = format!(
    "{}||{}||{}||{}",                      // separator: ||
    self.disclosure_id,
    scope_joined,
    self.expires_at_block,                 // as decimal string
    self.disclosed_fields_hash             // as hex string
);
hash_bytes(preimage.as_bytes())            // SHA3-256 over a single formatted string
```

`verifier_attestation` is then a **64-byte ed25519 signature** over that hash.  
Verification: `has_valid_attestation()` at `transaction.rs:1122–1138` does
`ed25519_dalek::VerifyingKey::verify_strict()`.

---

### Model B — consensus verifier side (state.rs)

**`selective_disclosure_binding_digest()`**  
`shared-protocol/src/blockchain/state.rs:6959–6989`

```rust
canonical_scope.sort_unstable();           // scope SORTED
let mut hasher = Sha3_256::new();
hasher.update(b"wrfcoin_selective_disclosure_v1");  // domain tag — absent in A
hasher.update(disclosure.disclosure_id.as_bytes());
hasher.update(b"|");                       // separator: |  (double-pipe in A)
// sorted scope keys joined by ","
hasher.update(b"|");
hasher.update(disclosure.expires_at_block.to_le_bytes()); // 8 binary LE bytes (decimal string in A)
hasher.update(b"|");
hasher.update(disclosed_fields_hash);      // raw 32 bytes (hex string in A)
```

`verifier_attestation` is treated as a raw bytes blob; validation checks that
**its first 32 bytes == this hash**.  
Validation: `validate_selective_disclosure()` at `state.rs:5508–5598`,
called from `validate_selective_disclosures()` at `state.rs:5477–5506`,
called from `apply_confidential_transfer()` at `state.rs:5038–5044`.

No ed25519 check is performed by the state verifier.

---

### Why the state tests pass despite the mismatch

The test helper `selective_disclosure_proof()` at `state.rs:8113–8132` directly calls
`selective_disclosure_binding_digest()` and hex-encodes the raw 32-byte output as
`verifier_attestation` — it never touches `SelectiveDisclosureProofPayload::binding_digest()`
or the ed25519 path.

---

### Differences summarised

| Dimension | Model A (transaction.rs) | Model B (state.rs) |
|---|---|---|
| Domain tag | none | `"wrfcoin_selective_disclosure_v1"` |
| Separator | `\|\|` | `\|` |
| Scope ordering | insertion order | sorted |
| `expires_at_block` encoding | decimal string | 8-byte LE binary |
| `disclosed_fields_hash` encoding | hex string | raw 32 bytes |
| Attestation format | 64-byte ed25519 sig | first 32 bytes = binding hash |
| Cryptographic check | ed25519 `verify_strict` | byte equality |

Any one of these differences alone causes a validation failure; all five are present.

---

## 4 open design questions (required before any fix)

1. **Attestation model** — Should `verifier_attestation` be a 64-byte ed25519
   signature (Model A) or a 32-byte binding digest prefix (Model B)?  These are
   mutually exclusive.

2. **Ed25519 key provenance** — If the sig model is chosen, state.rs needs a
   trusted attestor public key to verify against.  No such key or registry exists
   today.  Where does it live (governance params? per-transfer field? hardcoded
   compliance-verifier key?)?

3. **Canonical preimage** — Which format becomes canonical: the domain-tagged
   binary-encoded preimage (Model B) or the bare format-string (Model A)?  This
   determines what all historical proofs commit to and affects serialization
   compatibility.

4. **Consensus activation height** — Any change to `validate_selective_disclosure`
   (`state.rs:5508`) is a consensus-format change.  A hard-fork activation height
   is required for any live network; the fix must be gated identically to other
   privacy-feature activations (cf. `RING_SIG_VERIFICATION_ACTIVATION_HEIGHT`).

---

## Recommended immediate mitigation

Gate or disable the selective-disclosure feature at the API/route layer until the
design questions are resolved, to prevent silently-rejected transactions from
reaching users.  The `ConfidentialTransfer` base feature (without selective
disclosures) is unaffected and remains functional.
