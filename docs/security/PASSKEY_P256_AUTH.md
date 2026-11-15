# Passkey / WebAuthn (P-256) account authorization (#2169)

Part of epic #2167 (smart accounts & account abstraction). Lets a WRFCoin account be controlled
by a platform passkey instead of a seed phrase: the account address is derived from a WebAuthn
credential's P-256 public key, and a transparent transfer debiting it is authorised by a WebAuthn
assertion whose challenge is bound to the transfer.

Code: `shared-protocol/src/accounts/passkey.rs` (verifier, address derivation, payload form),
`privacy/src/p256_ecdsa.rs` (the curve primitive), `ConsensusFeature::PasskeyP256Auth`
(`shared-protocol/src/crypto/chain_binding.rs`).

## Rolling upgrade — what changes, and when

Nothing changes until genesis schedules `passkey_p256_auth` at a height H
(`0:v1,…,H:passkey_p256_auth`, #3092). Every consensus site that authorises a transparent
transfer asks `feature_active(ConsensusFeature::PasskeyP256Auth, height)`:

| surface | site | below H | from H |
|---|---|---|---|
| block validation (sync + producer self-screen) | `classify_block_transaction` → `Transaction::verify_transparent_transfer_authorization_at_height(ctx.height)` | ed25519 rule only | ed25519 **or** valid passkey |
| state apply | `State::apply_transparent_transfer` → `TransparentTransferPayload::verify_authorization_at_height(nonce, current_block)` | ed25519 rule only | ed25519 **or** valid passkey |
| mempool admission (no height) | `verify_authorization_for_admission` | ed25519 rule only unless the chain **schedules** the feature | admitted when scheduled; the exact height is enforced downstream by the three height-aware sites |
| API `/transactions/submit` (`passkey_auth` member) | `submit_transaction_compat` → `passkey_auth_active(tip+1)` then `verify_passkey_authorization` | refused `503 FEATURE_NOT_ACTIVE` | accepted and enqueued |

The ed25519 rule (`TransparentTransferPayload::verify_authorization`) is unchanged at every
height and never reads the passkey member. "Schedules" for admission means the base schedule
(genesis ∪ bootstrap) **or** an applied governance schedule extension (`feature_scheduled`, the
same source `feature_active` reads) — chain 888 activates the feature at h3400 through extension
#0. A passkey transfer admitted before H (e.g. via gossip) is dropped by the producer self-screen
and evicted from the mempool; it is never mined below H. `tests/passkey_activation_2169.rs` proves the flip at
exactly H on the payload, state-apply and mempool surfaces; `tests/integration/
passkey_feature_parity_2169.rs` replays the committed determinism fixture with the feature active
from height 5 and requires `expected_roots.json` unchanged.

**Pre-activation byte-identity.** A pre-#2169 binary ignores an unknown `passkey_auth` member. To
keep the new binary's pre-activation behaviour identical, the member is deserialised leniently:
anything that is not a well-formed `PasskeyAuthorization` reads as absent
(`deserialize_lenient_passkey`). A payload the old binary parses and accepts (ed25519-authorised)
is therefore parsed and accepted identically by the new one; a passkey-only payload is refused by
both until H.

## Wire form

`TransparentTransferPayload.passkey_auth` (all hex):

| field | content |
|---|---|
| `public_key_p256` | raw `X ‖ Y` (64 bytes) — the COSE EC2 coordinates |
| `rp_id_hash` | `SHA-256(rpId)` (32 bytes) |
| `authenticator_data` | raw `authenticatorData` |
| `client_data_json` | raw `clientDataJSON` bytes, exactly as the client produced them |
| `signature` | DER `ECDSA-Sig-Value`, **low-`s` normalised** |

`auth_pubkey` / `auth_signature` (ed25519) stay absent on a passkey transfer.

## The challenge a passkey signs over

```
message   = "wrfcoin_passkey_transfer_auth_v1"
          ‖ u32le(len chain_id) ‖ chain_id ‖ u32le(len network_id) ‖ network_id
          ‖ u32le(len from) ‖ from ‖ u32le(len to) ‖ to ‖ u64le(amount) ‖ u64le(nonce)
challenge = sha3_256(message)                     # 32 bytes → navigator.credentials.get
```

Same framing as the ed25519 message, under a different tag, so neither signature is a valid
authorization under the other scheme. The bytes are pinned in
`tests/passkey_webauthn_2169.rs::the_authorization_message_bytes_are_pinned` for wallet-SDK parity.

## What the verifier checks (`verify_webauthn`)

In order, cheapest first, one typed refusal each (`PasskeyError`):

1. field sizes ≤ 4096 bytes;
2. `clientDataJSON` parses; `type == "webauthn.get"`; `challenge == base64url_nopad(challenge)`;
3. `authenticatorData` ≥ 37 bytes; `rpIdHash == credential.rp_id_hash`; user-present flag set;
   user-verified flag set (the consensus policy — a funds-moving authorization needs biometric /
   PIN, not mere presence; scoped session keys, #2170, may relax it);
4. signature counter: if either the stored or the asserted counter is non-zero, the asserted one
   must be strictly greater (WebAuthn §6.1.1) — a regression indicates a cloned authenticator;
5. ECDSA-P256-SHA256 over `authenticatorData ‖ SHA-256(clientDataJSON)`, **low-`s` only**.

## Address derivation

`sha3_256("wrfcoin.passkey_address.v1" ‖ rp_id_hash ‖ 0x04 ‖ X ‖ Y)[..20]`. Folding the tag means no
ed25519 key (address `sha3_256(key)[..20]`) can derive a passkey address; folding the RP ID hash means
one key under two relying parties is two accounts. `from` must equal this address for the
credential the payload names — the same `from`-binding the ed25519 rule enforces (#1727).

## Policies and their reasons

- **Low-`s` only.** ECDSA is malleable; a relayer flipping `s → n − s` would mint a second
  transaction body with an equally valid signature. Every authenticator's output is equally valid
  after normalisation, so the client normalises and re-encodes before submitting (as smart-wallet
  SDKs do); an un-normalised assertion is refused, never silently accepted.
- **User verification required** on the consensus path (above).
- **Counter regression** is enforced by the verifier whenever a stored counter exists. The
  transaction path keeps **no per-credential counter state yet** — that is credential-registry
  state, which arrives with recovery (#2172) / delegation (#2175) on the #3091 migration
  framework — so `PasskeyAuthorization::verify_for_transfer` verifies against a stored counter of
  `0`. Transfer replay is prevented by the account nonce and the challenge binding
  (`tests/passkey_activation_2169.rs` applies the same authorization twice: the second fails).
- **Validation cost.** One P-256 verification per passkey transfer, after cheap structural
  checks. Pricing signature-heavy transactions is #2143.

## Test evidence

- `privacy/src/p256_ecdsa.rs` unit tests: round trip in both encodings, tampering, the high-`s`
  twin accepted on the curve and refused by the low-`s` rule, malformed inputs typed.
- `shared-protocol/tests/passkey_webauthn_2169.rs`: one refusal per verifier check; the full
  Wycheproof `ecdsa_secp256r1_sha256_p1363_test.json` (262 vectors, vendored under
  `tests/fixtures/wycheproof/`); address domain separation; pinned message bytes; payload-level
  binding and tamper tests; lenient-member byte-identity; the `SignatureVerifier` hook.
- `shared-protocol/tests/passkey_activation_2169.rs`: the activation flip at H on payload, mempool
  and state apply; ed25519 unchanged on every surface at every height.
- `tests/integration/passkey_feature_parity_2169.rs`: roots unchanged with the feature active.
- `shared-protocol/tests/passkey_extension_admission_3400.rs`: extension-scheduled activation —
  admitted once the extension is applied, self-screened below H and packed at H, state apply
  refused below H; ed25519 unchanged.
- `launch_api` `transactions_submit_accepts_passkey_transfer_only_once_active_at_next_height`: the
  route refuses a passkey transfer below tip+1 activation and accepts it at activation.

## Not in this story

Credential registration state and counters (#2172/#2175 on #3091), session keys (#2170),
sponsorship (#2171), verification-cost pricing (#2143), any wallet or frontend surface (separate
repositories), scheduling the feature on a live chain (owner decision; nothing here changes a
genesis).
