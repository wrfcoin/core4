# Advisory governance

Signed, text-only proposals with no execution path, behind the unscheduled
consensus feature `advisory_governance` (ordinal 33). Code:
`shared-protocol/src/governance/advisory.rs`.

## What a wallet can do

- **Propose**: a title (1–160 bytes, one line) and a summary (1–1,024 bytes,
  newlines allowed), signed by the wallet's ed25519 key or, for a passkey
  wallet, its passkey (see below). The chain picks the id
  (`adv-` + 16 hex), the deadline (1,120 blocks, about 24 h at 77 s) and the
  deposit (1 WRF, debited from the signer's own spendable balance only).
- **Vote**: for or against, once per proposal, signed. A wallet may vote if its
  spendable balance was at least 1 WRF immediately before the proposal applied
  (one wallet, one vote). Node-bond escrows and the faucet sink are excluded.
- **Outcome**, recorded at the first block at or after the deadline: fewer than
  3 votes is `failed_quorum` (the deposit stays held); otherwise `passed` when
  `for × 100 ≥ 51 × (for + against)`, else `rejected`. Passed and rejected return
  the deposit to the proposer. A result counts wallets, not people.

Nothing else happens. A passed proposal changes no parameter, no law, no reward
rule and no other account.

## Why it cannot execute anything

- The payload has no parameter, value, law, deadline or deposit member, and
  unknown members are refused (`deny_unknown_fields`).
- Records live in `State::advisory_proposals`, not `governance.proposals`, so
  `GovernanceVote`, `FinalizeGovernanceProposal` and `parse_governance_update`
  never see them. Finalize (`StateTransition::FinalizeAdvisoryProposal`) sets a
  status and returns the held deposit; that is its whole body.
- The legacy unsigned `GovernanceProposal` stays refused at mempool admission
  and in the block classifier at every height, whatever this feature's state.

## Signing

Every field is `u64 LE length || bytes`:

```text
tag        "wrfcoin.advisory_governance_action.v1"
chain_id   UTF-8
network_id UTF-8
account    20 address bytes
action     propose: 0x01 || len title || title || len summary || summary
           vote:    0x02 || len proposal_id || proposal_id || 0x01 for | 0x00 against
nonce      8 bytes LE
```

The key must hash to `account`, `account` must equal `tx.sender_address`, and
the signed `nonce` must equal `tx.nonce` and the account's nonce when it
applies (the nonce then advances). So a signature cannot move to another chain,
network, wallet, text, vote or nonce. Golden vectors:
`shared-protocol/tests/fixtures/golden/signing_preimage_v2.json`, kind
`advisory_governance_action_v2`.

### Passkey wallets

A passkey (P-256 WebAuthn) wallet signs the same message. Its assertion's
challenge is `sha3_256(message above)` (`advisory_passkey_challenge`, the same
hash the passkey transfer challenge uses), so it binds the same chain, network,
account, action and nonce. The payload carries a `passkey` member (the same
`PasskeyAuthorization` shape a passkey transfer carries: P-256 key, RP ID hash,
authenticatorData, clientDataJSON, DER signature, all hex) and no
`public_key`/`signature`. Exactly one of the two forms is accepted; unknown
members inside `passkey` are refused too.

Verification is the shared `accounts::passkey::verify_webauthn` under the
consensus policy (type `webauthn.get`, the challenge, RP ID hash, user present
and verified, low-s P-256), as for `passkey_bond_payout` and
`security_key_spend`, and the credential must derive `account`. The RP ID hash
is part of a passkey address, so an assertion made for another site names
another account. Replays stop at the account nonce, as for ed25519. Same flag:
nothing here is reachable before `advisory_governance` activates. A passkey
action is priced one extra P-256 verification. An ed25519 payload is
byte-identical to before.

Neither signature covers the outer transaction, so its id can change in
transit (no funds are at risk). Wallets track a proposal by its id and a vote
by `(account, nonce)`, not by tx id.

## Limits

| Limit | Value | Checked in |
|---|---|---|
| Title / summary bytes | 160 / 1,024; C0 (summary allows `\n`), DEL, C1 and bidi controls refused | admission, classifier, apply |
| Payload | 8 KiB | parse |
| Active proposals per wallet | 1 | apply |
| Active proposals on chain | 20 | apply |
| Proposals per block | 2 (each walks every account for its snapshot) | classifier |
| Accounts with a send allowlist or security key in force | cannot pay a deposit | apply |

A stateful refusal (nonce, slots, snapshot, deadline, balance) drops the
transaction at block assembly; a block carrying it fails a peer's apply.

## Activation and layout

Below activation the type is refused by mempool admission, the block
classifier and apply, so the map is empty. While empty it is absent from
`state_root_material`, from snapshots, and from checkpoints (the writer keeps
format 16 or older; format 17 appends the map and is written only when it is
non-empty). Ships unscheduled; scheduling is the owner's signature after
review and the determinism differential.

## Node API

- `POST /governance/advisory/submit`: a signed transaction built offline; dry
  runs the exact consensus check at tip + 1; 503 until active.
- `GET /governance/advisory/proposals[?status=]`, `GET .../{id}`,
  `GET .../{id}/eligibility?address=` (also returns the wallet's next nonce).

CLI: `wrfcoin governance advisory propose|vote|submit|list|show|eligibility`.
