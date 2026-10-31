# ADR: sponsored first sends (paymaster, #2171, owner GO 2026-11-01)

Status: accepted for build. Ships **unscheduled**. Scheduling is the owner's signature.

## Decision

A sponsor may pay the fee for someone else's transparent transfer. The sponsor signs a
`SponsorAuthorization` for that one transfer and the transfer carries it in
`TransparentTransferPayload::sponsorship`. Once `ConsensusFeature::Paymaster` is active, state apply
checks the authorization, debits the fee from the sponsor's own spendable balance, burns it, and
records the sponsorship in the sponsor's `State::paymasters` entry so protocol rate limits bound it.

This reuses the existing `paymaster` feature and `State::paymasters` map (storage format v8). It adds
no `ConsensusFeature`, no `TransactionType`, no `State` field and no storage format change, so it
stays out of the way of the other account workstreams (YubiKey-required wallets, session keys,
recipient allowlist) that add their own variants.

## What already existed (verified on main 0c6e0fb)

| Piece | State before this change |
|---|---|
| `accounts/paymaster.rs` | Deposit-based state machine (`Stake`/`RequestUnstake`/`WithdrawStake`/`SponsorOperation`), unit tests |
| `ConsensusFeature::Paymaster` | Registered, `has_behaviour = true`, `schedulable = false`, unscheduled on every chain |
| `StateTransition::ApplyPaymasterAction` | Gated arm in `state.rs`; **nothing constructs it** (p2p_events vacuity guard says so) |
| `State::paymasters` + root material | Present, empty on every chain, byte-absent from the root while empty |
| Transaction / route / CLI surface | **None.** No way to reach any of it |
| #3169 (no owner authorization on actions) | Open scheduling blocker |
| #3170 (`actual_cost = 0` accepted; `per_sender` never pruned) | Open scheduling blocker |
| Mobile `kSponsoredSendEnabled` | `false`; wired to the old EVM `/api/gas/sponsor` relay (social-login accounts, emails). That relay is superseded by native AA and must not be revived |

## Design

**Wire.** `SponsorAuthorization { sponsor, fee, expires_at_height, balance_floor, first_send_only,
sponsor_public_key, signature }`, JSON/hex. Deserialized leniently: a malformed member reads as
absent, exactly like `passkey_auth`, so an older binary and this one parse every payload the same.

**Preimage** (`wrfcoin_sponsored_send_auth_v1`, pinned in the golden fixture as
`sponsored_send_authorization_v1`): tag, length-prefixed chain id and network id, length-prefixed
`sponsor`, `from`, `to`, then `amount`, `nonce`, `fee`, `expires_at_height`, `balance_floor` (u64 LE)
and `first_send_only` (one byte). It binds the exact transfer and every rule the sponsor set.

**State apply** (`apply_transparent_transfer`), only when `paymaster` is active at the block:

1. The user's own authorization, unchanged (ed25519 or passkey).
2. Stateless rules (`SponsorAuthorization::check`): key derives `sponsor`; signature verifies; the
   sponsor is not `from`; `height <= expires_at_height <= height + 32`; `1_000 <= fee <= 1_000_000`
   base units; `first_send_only` implies transfer nonce 0.
3. Sponsor spendable balance `>= fee + balance_floor`.
4. Protocol rate limits on the sponsor record (`charge_balance_sponsorship`, computed on a copy).
5. The transfer applies (nonce, user balance). Any failure so far leaves state untouched.
6. Commit: debit the fee from the sponsor, add it to `supply_authorized_burning`, store the record.

Below activation the field is never read: balances, nonce and `state_root` are byte-identical to the
same transfer without it, and `paymasters` stays empty and byte-absent. Mempool admission screens the
sponsor signature only on a chain that schedules `paymaster`.

**Blockers closed for the reachable path.**
- #3169: a sponsorship is charged only with the sponsor key's chain-bound signature over the transfer
  it pays for, inside a 32-block window. The deposit lifecycle actions stay unreachable (no
  transaction builds them) and still need owner authorization before anything does.
- #3170: zero cost is refused on both paths (`PaymasterError::ZeroCost`, `MIN_SPONSORED_FEE`);
  `per_sender` is cleared when the sponsor's epoch rolls, so it never exceeds
  `max_sponsorships_per_epoch` entries.

**Why the sponsor's balance, not a staked deposit.** A deposit needs its own lifecycle transaction
and unstake protocol (a new `TransactionType`). Every rule a sponsor sets is already in its own
signature, and its worst-case loss is bounded on chain: fee cap x per-epoch count, stopped early by
its balance floor. The fee is burned, so a leaked sponsor key lets an attacker waste the sponsor's
WRF but never collect it.

## The finding that matters most: transfers pay no fee on chain today

On chain 888 no transaction fee is debited or credited anywhere in state apply. `tx.fee` only orders
the mempool and feeds an admission floor that is 0 by default. The web and phone wallets already say
so ("not charged on chain"). Two consequences:

1. **A fee sponsor gives users nothing until a fee exists.** A sponsored transfer costs the sponsor
   `fee`; an unsponsored one costs the user nothing.
2. **A user with 0 WRF still cannot make a first send**, because `amount >= 1` must come from their
   own balance. The wall a newcomer hits on testnet is "no coins", and that is the faucet's job
   (the passkey faucet panel, plus the Get-test-coins button already approved).

So this change makes sponsorship correct and ready. It becomes useful when the owner adds a fee
debit (the sinks ADR proposes a 50% fee burn) or when a node raises its mempool fee floor and that
fee is actually charged.

## Anti-abuse (Sybil)

Fresh addresses are free, so "one sponsored send per new address" is farmable by design. The limits
below make farming cost the sponsor little and pay the farmer nothing.

| Limit | Where | Value |
|---|---|---|
| Fee is burned, never paid to anyone | Consensus | always |
| Fee bounds per sponsored transfer | Consensus | 1_000 to 1_000_000 base units (0.00001 to 0.01 WRF) |
| Authorization lifetime | Consensus | at most 32 blocks (about 41 min) |
| First send only (nonce 0), if the sponsor asks | Consensus, sponsor-signed | sponsor's choice |
| Sponsor balance floor | Consensus, sponsor-signed | sponsor's choice |
| Per sponsor per epoch (7_200 blocks) | Consensus | 10_000 |
| Per sender per sponsor per epoch | Consensus | 200 (1 in practice with first-send-only) |
| Daily cap, per-wallet and per-IP limits | Sponsor service | recommend 200/day total, 1 per wallet ever |
| Proof of a person | Sponsor service | the faucet's existing challenge, plus a wallet signature; no email or other PII |
| Only sponsor a transfer whose sender already holds the amount | Sponsor service | always (otherwise the chain refuses it anyway) |

Worst case with the recommended service caps: 200 sends/day x 0.01 WRF = 2 WRF/day burned.

## Before scheduling (owner)

1. Decide that a fee is debited on chain (otherwise sponsorship has no user benefit).
2. Meter the sponsor signature in `verify_cost` (one ed25519 per sponsored transfer). It is not
   charged today, to keep the pre-activation block budget byte-identical. Bounded by the per-epoch cap.
3. Flip `ConsensusFeature::Paymaster` to `schedulable = true` in its own release.
4. Determinism differential across the three producers; all three upgrade together.
5. Provision the sponsor key (HSM or the faucet's key custody), fund it, ship the sponsor service.
