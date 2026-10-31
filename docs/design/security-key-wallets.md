# Security-key wallets: "require my YubiKey to send" for 12-word wallets

Status: implemented behind the UNSCHEDULED consensus feature `security_key_spend`. Owner GO 2026-11-01.
Scheduling is the owner's signature (a schedule-extension kit), never part of this change.

## What a user gets

A 12-word (passphrase, ed25519) wallet can register a FIDO2 security key (a YubiKey or any WebAuthn P-256
authenticator). From then on every send from that account needs **both** the phrase key's signature **and** a
touch + PIN on the security key. A thief who learns the phrase alone cannot move the money.

Passkey wallets (frontend#29) are a different thing: there the security key IS the wallet (the address is
derived from the P-256 key). Here the address stays the phrase key's address; the security key is a second
factor the chain enforces.

## Where it plugs in

The per-account authorization dispatch (`account_authorization_dispatch`, active on chain 888 since h3400,
`shared-protocol/src/accounts/authorization.rs`) is the hook. A new closed-enum rule is added:

```
AuthorizationRule::SignerPlusSecurityKey { policy: SecurityKeyPolicy }
SecurityKeyPolicy { keys: 1..=2 RegisteredSecurityKey, escape_delay_blocks, pending_escape: Option<PendingEscape> }
RegisteredSecurityKey { public_key_p256 (64 B raw X||Y), rp_id_hash (32 B), credential_id (1..=255 B, public handle) }
```

It is stored in the existing `State::authorization_declarations` map (no new State field, no storage-version
bump, no new state-root section): the declarations section of `state_root_material` gains one arm for the new
variant. That map is empty on every live chain today (nothing extracts `DeclareAuthorization`), and a security-key
rule can only be written once `security_key_spend` is active, so before activation the section stays byte-absent.

The counters already on `AccountAuthorizationDeclaration` are reused: `next_nonce` is the replay nonce of every
security-key action, `generation` counts changes.

## Spending (Transfer, Stake, Unstake)

`authorize_debit_with_security_key` (sibling of `authorize_debit`) runs the existing signer-is-sender ed25519 check
and then, for an account whose rule is `SignerPlusSecurityKey` and whose escape has not matured, requires a
`security_key_auth` member in the transaction's JSON `data`:

```
security_key_auth = { public_key_p256, authenticator_data, client_data_json, signature }   (all hex)
```

- The named key must be one of the account's registered keys (so at most ONE P-256 verification per spend).
- WebAuthn verification is the passkey verifier (`passkey::verify_webauthn`) unchanged: `webauthn.get`, RP ID hash
  equals the registered key's, user-present AND user-verified (touch + PIN), low-s ECDSA-P256-SHA256.
- Challenge = `SHA3-256(len||"wrfcoin.security_key_spend.v1" || len||account || len||debit_message)`, where
  `debit_message` is the exact ed25519 preimage of the spend (transfer: chain_id, network_id, from, to, amount,
  nonce; stake: the same plus the stake/unstake flag). So the assertion is single-use (nonce), chain-bound, and
  cannot be lifted onto another amount, recipient, account or chain.
- The member is a separate JSON key next to the transfer payload, parsed leniently. No payload struct gains a field,
  so a payload's own bytes are unchanged and an old binary simply ignores the member.

Debit paths that move transparent value and do not go through the seam already refuse every non-default rule
(node bond lifecycle, node receipts, node fraud report), and shielded deposits / governance proposals are
disabled chain-wide. So a protected account can send, stake and unstake (with the key) and cannot bond a node
until those paths learn the rule (owner decision 3 below).

Pre-block validation (`verify_block`, mempool) has no State and still checks the ed25519 half only. That is safe
because the State-backed check is strictly stronger (it never accepts what the stateless check refuses): a
phrase-only spend from a protected account is dropped deterministically at block assembly
(`governance_transitions_for_block`) and refused by every applier. The node API runs the State-backed check at
submit time so the user gets `SECURITY_KEY_REQUIRED` instead of a silent drop.

## Turning it on, changing it, turning it off

New transaction type `SecurityKeyAction` (appended, bincode index 40), routed through the governance sink like
`Transfer` (so an invalid action is dropped at assembly, never a block fault). Its `data` is:

```
{ account, nonce, action, owner_public_key, owner_signature, key_assertions: [ {public_key_p256, authenticator_data, client_data_json, signature} ] }
```

`owner_signature` is ed25519 by the phrase key over
`"wrfcoin.security_key_action.v1" || chain_id || network_id || account || nonce || action-encoding`
(every field length-prefixed). Each key assertion's challenge is `SHA3-256` of the same bytes, so the key co-signs
exactly this action on this chain at this nonce.

| action | phrase signature | security-key assertion | effect |
|---|---|---|---|
| `enable { key, escape_delay_blocks }` | yes | the NEW key (proof of possession) | rule on; allowed when off or when an escape has matured |
| `add_key { key }` | yes | an EXISTING key AND the new key | second key (max 2) |
| `remove_key { public_key_p256 }` | yes | any registered key | removing the last key turns the rule off |
| `disable` | yes | any registered key | rule off immediately |
| `begin_escape` | yes | none | time lock starts: rule lapses at `height + escape_delay_blocks` |
| `cancel_escape` | yes | any registered key | pending escape cleared |

Refusals: duplicate key, more than 2 keys, `add_key`/`begin_escape` during a pending escape, `begin_escape` on a
matured escape, stale nonce, wrong chain, key not registered, assertion without user verification, high-s.

### Lost key: the time-locked escape

`begin_escape` needs only the phrase. The rule keeps being enforced until `executable_at_height`; from that height
`authorization_for` resolves the account to `SignerIsSender` without any further transaction (same "lapse falls
back to the account's own key" rule as an expired delegation). During the window the owner, who still has the key,
cancels with a key-signed `cancel_escape`. A thief who has only the phrase can start an escape but cannot cancel the
owner's cancel, and the owner sees the pending escape in every wallet. The right response to a stolen phrase is
then to send everything (phrase + key) to a fresh wallet.

`escape_delay_blocks` is chosen at `enable`: min 1,123 blocks (~24 h at 77 s), default 7,855 (~7 days), max 33,663
(~30 days). It can only be changed by turning the rule off with the key and enabling again.

With two keys a lost key is replaced immediately with the other (`remove_key` + `add_key`), no waiting.

### Why no signature counter

Like `passkey_p256_auth`, the chain does not store a per-credential WebAuthn counter: replay is already prevented
by the account nonce in the challenge, and storing a counter would make every spend write declaration state. Clone
detection is therefore not enforced on chain (same as passkey wallets).

## Fee and size

An assertion adds about 37 B authenticatorData + ~140-250 B clientDataJSON + ~71 B DER signature + 64 B key,
hex-encoded: roughly 650-850 bytes per protected send, so the per-byte fee floor charges it automatically. The
verification-work budget (#2143) charges one extra P-256 verification for a Transfer/Stake/Unstake that carries
`security_key_auth`, height-gated on `security_key_spend` so pre-activation pricing is byte-identical, and two P-256
verifications for every `SecurityKeyAction` (fail-expensive: always the maximum an action can carry).

## Clients

- CLI (`wrfcoin wallet security-key …`): `status`, `enable`, `add`, `remove`, `disable`, `escape`, `cancel-escape`,
  and `wallet send` attaches the key automatically when the account is protected. The CLI drives the key through
  libfido2's `fido2-cred` / `fido2-assert` tools (Linux, macOS, Windows), builds `clientDataJSON` itself with
  origin `wrfcoin-cli`, normalises s to low-s, and never prints the phrase. RP ID defaults to `wrfcoin.com` so a
  key enrolled in the CLI also works in the web wallet. PKCS#11 (YubiKey PIV) is not used: PIV produces bare ECDSA,
  not a WebAuthn assertion, and the chain verifies WebAuthn.
- Node API: `GET /accounts/{address}/security-key` (status, next nonce, pending escape, whether the feature is active
  at tip+1); `POST /accounts/security-key/actions` (State-checked, then enqueued); `/transactions/submit` accepts
  `security_key_auth` and refuses a protected phrase-only send with `SECURITY_KEY_REQUIRED`.
- Web wallet (follow-up PR, frontend): reuse the passkey WebAuthn code: `navigator.credentials.create` for enable,
  `navigator.credentials.get` with `allowCredentials=[credential_id]` and the challenge above for sends; show a
  red banner while an escape is pending with a one-tap cancel.
- Mobile (follow-up): same as web through the platform FIDO2 API (Android Credential Manager / iOS
  ASAuthorization security-key provider, NFC or USB-C YubiKey).

## Activation

- Unscheduled in this binary. `schedulable()` is `true` (the binary accepts a schedule extension naming it),
  `has_behaviour()` is `true`. It only has effect when `account_authorization_dispatch` is also active (true on 888).
- Before activation: a `SecurityKeyAction` makes a block invalid (`verify_block`) and is refused by the mempool unless
  the feature is scheduled; no rule can be stored; `security_key_auth` members are ignored; verify pricing is
  unchanged. A binary carrying this change replays chain 888 to identical state roots.
