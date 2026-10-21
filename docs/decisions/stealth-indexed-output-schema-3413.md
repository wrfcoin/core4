# Stealth indexed-output schema (Option A) for core4#3413

Refs #3413, Refs #3213, Refs #3400. **Design only.** Nothing here changes consensus, state, a wire format, a
checkpoint, a route or any schedule. Facts are read from `origin/main` `b3c1e1033`; line numbers drift, so each is
paired with the symbol it names.

Owner ruling (core4#3413, option A): a **new, unscheduled, indexed-output schema** with domain separation and an
output index. The legacy verifier is **retained**. The signed-type rules apply (no bincode in a signing preimage,
golden vectors, inventory ratchet).

## 1. What is wrong today (P2-5, restated against current code)

- `privacy/src/stealth_addresses.rs`: the shared secret is `SHA3-256(compress(rA))` with no domain tag and no output
  index, computed identically in `StealthAddress::new` (`:172`), `is_for_recipient` (`:224`),
  `recover_private_key` (`:254`) and `decrypt_payment_id` (`:281`). Two outputs to one recipient that reuse `R`
  get the same one-time key and the same key image; only one can be spent.
- The 8-byte payment-id pad is `shared_secret[0..8]` (`:184`, `:285`), the same bytes that become the
  spend-key offset, and the id is unauthenticated.
- Consensus never looks inside `stealth_data`: `State::apply_confidential_transfer`
  (`shared-protocol/src/blockchain/state.rs:7581-7593`) only requires it for Medium/High/Maximum and calls
  `validate_hex_blob(.., PRIVACY_PROOF_MIN_BYTES = 16 (:68), ..)` (`:8690`). So **no consensus rule can enforce
  one-time-key uniqueness today**: the field is an opaque hex blob of unspecified layout
  (`ConfidentialTransferPayload.stealth_data`, `transaction.rs:1826`).
- Off-chain only callers: `native-chain/src/bin/launch_api/routes/privacy.rs` (`StealthAddress::new` `:266`,
  literal construction `:325`) and `privacy/tests/privacy_e2e.rs`.
- The v2 authorization message
  (`confidential_transfer_authorization_message_with_domain`, `transaction.rs:1864`, tag
  `wrfcoin_confidential_transfer_auth_v2`) covers `stealth_data` as an opaque optional string. It cannot cover any
  new field, so a new field needs a **new tag**, not an edit.

## 2. Gate and compatibility rules

- New `ConsensusFeature` `stealth_indexed_outputs` (name proposed; the owner may rename before S1). Registered
  **OFF and unschedulable** exactly as core4#3468 registers `stake_submission_nonce`: named in
  `crypto/chain_binding.rs` (`ConsensusFeature`, `:652`), `protocol_support.rs` `KNOWN_FEATURES`, parsed by name,
  refused in a genesis schedule until a later slice gives it behaviour. No height, no schedule edit, no genesis edit.
- **Legacy verifier retained.** The v2 message, the `stealth_data` path and legacy `StealthAddress::{new,
  is_for_recipient, recover_private_key, decrypt_payment_id}` stay byte-for-byte as they are, also after the gate is
  ON. A payload with no indexed outputs is verified exactly as today at every height. Retiring the legacy path is a
  separate, later owner decision and is not proposed here.
- The new schema is an **additive optional field** on `ConfidentialTransferPayload`:
  `#[serde(default, skip_serializing_if = "Vec::is_empty")] stealth_outputs: Vec<IndexedStealthOutput>` (the same
  idiom `selective_disclosures` uses at `transaction.rs:1836`). With the field absent or empty the payload's **JSON**
  and the v2 preimage are unchanged, which is the gate-OFF equivalence property. This is a JSON-boundary claim only:
  `skip_serializing_if` does **not** make a derived direct bincode round-trip backward-compatible (positional
  decoding still expects the element; `default` does not repair that), and the repository's
  `.github/workflows/bincode-boundary-guard.yml` flags exactly this shape at a direct bincode boundary. The payload
  is never decoded that way in production: it is JSON stored in transaction data
  (`Transaction::parse_confidential_transfer_payload`, `transaction.rs:2775`), and that JSON string is what the
  outer transaction/checkpoint encoding carries. The new field must therefore never be passed to a direct bincode
  (de)serializer; if one is ever wanted, it needs a separately reviewed versioned or hand-written binary
  representation, not this attribute.
- Gate OFF + non-empty `stealth_outputs` is refused at admission and in the applier with one fixed error code
  (proposed `SP_PRIVACY_STEALTH_INDEXED_NOT_ACTIVE`), never silently ignored.

## 3. The schema

```
IndexedStealthOutput {
    output_index: u32,            // position in output_commitments; must equal the position, no gaps, no repeats
    transaction_key: [u8; 32],    // R, canonical Ristretto, not identity
    one_time_public_key: [u8; 32],// P, canonical Ristretto, not identity
    payment_id: Option<[u8; 8]>,  // pad from its own domain; see below
}
```

Derivation (privacy crate, new functions `StealthAddress::new_indexed` / `*_indexed`; the legacy ones untouched):

- `s_i = H_scalar("wrfcoin-stealth-v2/scalar" || len(compress(rA)) || compress(rA) || output_index_le32)` reduced
  from a 64-byte hash (`Scalar::from_bytes_mod_order_wide`), so `P_i = s_i G + B`; recipient recovers
  `x_i = s_i + b` with `s_i` from `aR`.
- Payment-id pad: `H("wrfcoin-stealth-v2/payment-id" || same framed inputs)[0..8]`, an independent hash, so the pad
  bytes are no longer the spend-key offset bytes.
- Payment-id authenticity (P3-3): the id is sender-chosen metadata; it is **authenticated by the transaction
  authorization**, because `stealth_outputs` are covered by the new v3 preimage (section 4). No separate MAC and no
  new key; a view-key holder can read it but a third party cannot alter it without breaking the ring signature.
  (Decision for the owner: alternatively drop the field.)
- Reject identity and non-canonical `A`, `B`, `R`, `P` in every new function (P3-3).
- All framing is explicit length-prefixed bytes. No `bincode` anywhere in a hashed or signed input.

## 4. Signing: a new versioned preimage, never an edit of v2

Per the signed-type rules (`signed_type_inventory.rs`, `AUTHORIZATION_MESSAGE_SURFACES`):

1. New tag `wrfcoin_confidential_transfer_auth_v3` and builder
   `confidential_transfer_authorization_message_v3_with_domain(chain_id, network_id, payload)`. It starts from the v2
   field order (domain tag, `push_chain_domain`, commitments, range proof, stealth_data, key images, encrypted
   payload, privacy level, disclosures) and **appends** the stealth outputs: count (u32 LE), then per output
   `output_index` (u32 LE), `transaction_key` and `one_time_public_key` as length-prefixed fields, and a presence
   byte plus the 8 bytes for `payment_id`. No bincode.
2. v2 stays and its pinned golden case `confidential_transfer_authorization_v2` is **not edited**. A payload with
   non-empty `stealth_outputs` must be signed over v3; the applier selects the message by `stealth_outputs.is_empty()`
   and the gate, so a v2 signature can never validate a payload that carries indexed outputs (no downgrade).
3. `signed_type_inventory.rs`: a **second** `AuthSurface` entry
   `{ rust_type: "ConfidentialTransferPayload", domain_tag: "wrfcoin_confidential_transfer_auth_v3", golden_kind:
   "confidential_transfer_authorization_v3" }` (one entry per domain tag; tag unique and disjoint from
   `SIGNED_OBJECT_REGISTRY`).
4. Byte-pinned `money_path_v2` golden case of kind `confidential_transfer_authorization_v3` with `tampered_input`
   twins that each flip exactly one bound field (output_index, R, P, payment_id, an unchanged v2 field). Add
   `build_*_case()` and the `money_path_preimage` arm; regenerate with `WRFCOIN_REGEN_GOLDEN_VECTORS=1` and confirm
   the fixture diff is **purely additive**.
5. detect-secrets: the new golden's hex fields need hand-spliced `.secrets.baseline` entries (never
   `detect-secrets scan --baseline`). Any new `tests/*.rs` file must be added to the integration ratchet inventory
   (`primitives/src/integration_inventory.rs`) or `every_integration_target_under_tests_is_in_the_ratchet_inventory`
   goes red.

## 5. Consensus rules once the gate is ON (all behind the gate)

R1. `stealth_outputs.len() == output_commitments.len()` when non-empty; `output_index` equals the position.
R2. Every `R`, `P` canonical, non-identity, 32 bytes; `P` unique **within the transaction**.
R3. `P` unique **per chain**: a new state set `stealth_one_time_keys`. It must **not** be age-pruned: today
    `privacy_spent_key_images` is pruned by `prune_set_by_height` against its `*_height` map (`state.rs:1144-1147`,
    `:6077`), which would let an old one-time key be reused after the cutoff and is the opposite of what P2-5 needs.
    The set follows the "intentionally NOT pruned" precedent noted at `state.rs:1337`. A height record may be kept
    for reorg rollback only; S5 must prove rollback with a test through the real reorg path rather than assume it.
R4. Failure leaves state unchanged (P2-4): validate everything before the first mutation.
R5. v3 preimage required; v2 refused for a payload carrying indexed outputs.

R3 is a **state-layout change**: a new field in `State`, in the root material and in the checkpoint. It follows the
same discipline as slash-evidence S2 (core4#3465): the section is **absent from the root material while the set is
empty and the gate OFF**, so every pre-existing root is byte-identical; a checkpoint format number is taken only at
rebase time (LEAD ruling 2026-10-03: core4#3433 takes 11); and the slice includes a migration test from the previous
format. Because it is a state change it also needs the full native-chain test set, not a focused run.

## 6. Slices (one PR each, each independently gate-OFF-equivalent)

| Slice | Content | Touches consensus/state? |
|---|---|---|
| S0 | Pin gate-OFF baseline: the JSON and v2 preimage of legacy confidential transfers, the state root after a legacy confidential transfer, existing `privacy_e2e` behaviour. Tests only. | no |
| S1 | Privacy crate only: `new_indexed` / `*_indexed` derivation, identity/non-canonical rejection, vectors. Legacy functions untouched. | no (library, uncalled by consensus) |
| S2 | Register `stealth_indexed_outputs` OFF and unschedulable (the #3468 pattern). | registration only |
| S3 | `IndexedStealthOutput` type, optional payload field, v3 preimage builder, signed-type inventory entry, golden case, baseline splice. Applier still refuses the field (gate OFF). | wire type only |
| S4 | Gate behaviour R1, R2, R4, R5 (per-transaction uniqueness, no new state). Producer and applier share one validator. | consensus, no state layout |
| S5 | R3 per-chain uniqueness: state set + height map, conditional root section, checkpoint migration, reorg removal. | state layout |
| S6 | `launch_api` route accepts an optional `output_index` and returns indexed stealth data; default stays legacy. | route, no consensus |

S4 and S5 are separable on purpose: S4 closes the in-transaction collision named by the finding without a state
change; S5 is the larger, owner-scheduled step. Activation, scheduling and any default-on decision are out of scope.

## 7. Gate-OFF equivalence proof plan

For each of S2 to S6, with the feature absent from the schedule:

- Serialization: a legacy payload round-trips to identical **JSON** (the new field is skipped when empty), and a
  transaction carrying it has identical outer transaction bytes, because the payload travels as a JSON string in
  `Transaction.data`. Pin both with a byte-compare against the S0 fixture. No direct-bincode round-trip of
  `ConfidentialTransferPayload` is claimed or tested, and bincode-boundary-guard must stay green.
- Preimage: `authorization_message()` for a legacy payload is byte-identical to the S0 pin (v2 untouched).
- State: apply a fixed sequence of legacy confidential transfers and compare `state_root` and the checkpoint bytes
  with the S0 pins for S2 to S4 (no state or checkpoint change). S5 allocates a new checkpoint version, so its
  checkpoint **bytes differ by design** (new version number). For S5 prove instead: unchanged `state_root` (the new
  section contributes zero bytes to the root material while empty and the gate is OFF), a logically identical
  state after loading an old-format checkpoint (migration test), and that re-encoding writes the new version.
- Behaviour: a payload with non-empty `stealth_outputs` is refused with `SP_PRIVACY_STEALTH_INDEXED_NOT_ACTIVE`
  and leaves state unchanged.
- Wire: a node without the field parses the same transaction JSON and ignores nothing it should reject (the JSON
  idiom `selective_disclosures` uses; this is not evidence about direct bincode).

## 8. Producer/applier parity plan

One pure function `validate_indexed_stealth_outputs(&payload, &state_view) -> Result<(), StateError>` called from
mempool admission, from block-producer transaction selection and from `apply_confidential_transfer`. Parity test:
for each rejection case (R1, R2, R3, R5) assert the producer excludes the transaction **and** the applier rejects a
block that contains it, with the same error code. A block built by hand with a violating transaction must be refused
by the applier even though the producer would never emit it.

## 9. RED tests that prove it (names are proposals; optionally landed as `#[ignore]`d tests first)

Privacy crate (S1), the P2-5 matrix row "two outputs to one recipient yield distinct one-time keys":
- `legacy_stealth_two_outputs_same_r_collide_3413` (documents today's bug; passes on main, pins the legacy behaviour).
- `indexed_stealth_two_outputs_same_r_yield_distinct_keys_3413` (RED until S1).
- `indexed_stealth_payment_id_pad_independent_of_scalar_3413`.
- `indexed_stealth_rejects_identity_a_b_r_p_3413`.
- `indexed_stealth_recipient_recovers_each_output_key_3413`.
- `legacy_stealth_functions_unchanged_golden_3413`.

Shared-protocol:
- `confidential_transfer_v3_preimage_binds_every_stealth_field_3413` (tampered twins).
- `v2_signature_cannot_authorise_payload_with_indexed_outputs_3413` (no downgrade).
- `stealth_outputs_refused_while_gate_off_3413`.
- `legacy_confidential_transfer_root_and_preimage_unchanged_gate_off_3413` (S0 pins).
- `duplicate_one_time_key_in_one_transaction_refused_3413` (S4).
- `duplicate_one_time_key_across_transactions_refused_3413`, `one_time_key_set_not_age_pruned_3413`, `reorg_removes_abandoned_branch_one_time_keys_3413` (S5).
- `failed_indexed_transfer_leaves_state_unchanged_3413`.
- `producer_excludes_what_applier_rejects_indexed_stealth_3413`.
- `checkpoint_without_stealth_set_loads_and_roots_equal_3413` (S5 migration).
- Existing gates that must stay green: `signed_type_inventory`, `signing_preimage_v2_golden`,
  `registry_doc_matches_code` (if a domain-tag row is added), the integration-target inventory, detect-secrets.

## 10. Dependencies, risks and open owner questions

- The review (P1-1 core4#3407, P1-2 core4#3408, P2-4 core4#3412, P2-6 core4#3414) says the privacy paths are
  reachable in default builds. Option A does not close those; S3 to S5 should not be scheduled before #3407 settles
  key-image canonicalization (P1-1) and the reorg/atomicity work (P2-4, P2-6), because R3 depends on the same rollback path.
- Per-chain uniqueness (R3) costs state growth proportional to private outputs. Owner decision: accept, or restrict
  the first release to R2 (S4) and defer S5.
- Payment id: authenticate-by-transaction (proposed) or drop the field.
- Feature name `stealth_indexed_outputs` is a proposal.
- The off-chain HTTP routes keep server-side key custody (P2-7, core4#3415); that is independent of this schema.
