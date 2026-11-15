# Chain-enforced send allowlist

Owner request (2026-11-01): "send transactions can only go to whitelisted addresses", and it must hold even
against a thief who has the passphrase. Code: `shared-protocol/src/accounts/send_allowlist.rs`. Gate:
`ConsensusFeature::SendAllowlist` (`send_allowlist`), shipped **unscheduled**.

## What the owner gets

| Action | Effect | When |
|---|---|---|
| Enable (delay D, initial list) | Every send out of the account to an unlisted address is refused by consensus | Immediately (only while off) |
| Remove address | Address can no longer receive | Immediately |
| Raise delay | Longer wait for future additions / turn-off | Immediately (only upward) |
| Request add | Address becomes usable | After D blocks; cancellable until then |
| Cancel add | Pending address dropped; starts a 561-block (~12 h) cooldown on new requests | Immediately |
| Request disable | Policy lifts | After D blocks; cancellable until then |
| Cancel disable | Policy stays on; starts the cooldown | Immediately |

D is per account, 1,122 ≤ D ≤ 33,662 blocks (~24 h to ~30 days at 77 s/block); the CLI default is 2,244
(~48 h). A self-send is always allowed. Receiving is never restricted. Limits: 64 addresses (including
pending), 8 pending additions.

Every action is signed by the account's ed25519 key over a length-prefixed, domain-separated preimage
(`wrfcoin.send_allowlist_action.v1`, chain id, account, action, action nonce). The per-account nonce never
resets (the entry is kept after a disable), so no signature can be replayed on another chain, account,
action or moment.

What it does not do: if the owner never looks, a thief's pending addition becomes usable after D. The
policy buys the owner D blocks of warning. `GET /accounts/{address}/send-allowlist` lists pending
additions so a wallet can make them impossible to miss.

## Design choice: a separate per-account policy, not an `AuthorizationRule`

`AuthorizationRule` (`authorization.rs`) answers *who may sign* a debit; an account declares one rule. The
allowlist answers *where a debit may go* and must hold whatever signed it (ed25519, passkey, session key,
a future delegate). As a rule it would be one alternative among several, any path not routed through
dispatch would bypass it, and dispatch itself is unscheduled. So it is its own state,
`State::send_allowlists: BTreeMap<Address, AccountSendAllowlist>`, consulted at the apply seams **after**
authorization succeeded. It follows the `recovery.rs` shape: block heights, time-locked and
owner-cancellable changes, domain-separated owner signatures, a cooldown against griefing, and state that
is byte-absent before activation.

The action travels as a new appended transaction type, `TransactionType::SendAllowlist` (index 42; `ConsensusFeature` ordinal 28), applied
through the same shared sink as `Transfer` (`ProcessGovernanceTransaction` → `apply_protocol_transaction`),
so producer, P2P applier and WAL replay run one code path.

## Which transaction types count as sending value (deny by default)

`send_allowlist::value_movement` classifies every `TransactionType` exhaustively (a new type does not compile
until classified). Guards live in the apply arms, so every emitter of a transition is covered:

| Type / transition | Class | Rule while a policy is in force |
|---|---|---|
| `Transfer` → `BalanceTransfer` (ed25519, passkey, session-key (#97), security-key (#103), sponsored or authority-signed) | to destination | Refused unless `to` is the account itself, allowed, or a matured addition |
| `ShieldedDeposit` → `ApplyShieldedDeposit` | unlistable | Refused (destination hidden by design; also disabled chain-wide, #1762) |
| `GovernanceProposal` → `CreateGovernanceProposal` | unlistable | Refused (deposit is held, never refunded; also disabled chain-wide, #2024) |
| `NodeBondLifecycle` Deposit / Update | per action | Refused unless `payout_account` is permitted (it receives the bond's rewards) |
| `NodeBondLifecycle` Exit / Withdraw | per action | Allowed (principal returns to the owner) |
| Paymaster `Stake` (`ApplyPaymasterAction`) | unlistable | Refused (deposit pays other accounts' costs) |
| Sponsored-send fee (#96, `check_sponsorship`), when the policy account is the sponsor | unlistable | Refused (the fee is burned) |
| Session key `Use` with value > 0 | to destination | Refused unless `target` is permitted |
| `Stake` / `Unstake` | within account | Allowed |
| `ConfidentialTransfer`, `ShieldedWithdrawal`, `RewardClaim` | not a debit | Allowed: they spend shielded notes, not the transparent balance |
| Rewards, coinbase, slashing, receipts, votes, registrations, data/NFT/bridge/treasury types | not a debit | Allowed: credits, protocol-initiated, or no balance effect in this binary |

Fees: with `transfer_fee_burn` (#101) a permitted send still pays the protocol fee, which is burned (credited to
nobody) and set by the chain, not by the sender. A refused send debits nothing, not even the fee, and a refused
session-key send spends none of the key's allowance (`tests/send_allowlist_interactions.rs`). A fee cap for protected
accounts is a follow-up for the fee release (owner decision).

Account recovery (`move_recovered_account`, unscheduled): the policy moves with the funds to the new owner
address; if that address had its own entry, the recovered policy wins and the larger nonce is kept.

## Activation and byte identity

Below activation: mempool admission refuses the type (`SP_SEND_ALLOWLIST_INACTIVE`), the block classifier
invalidates a block carrying one (an older binary cannot even decode it), and the apply arm returns
`InvalidTransition`. So `send_allowlists` is empty, every guard is a no-op (each checks the map first), and:

- `state_root_material` has no allowlist section (not even a tag) while the map is empty;
- checkpoints are written at the previous format (15, or the deployed 10/11/12) while it is empty; format 16
  (`MigrationV15ToV16` appends an empty map, `legacy/state_v15.rs` freezes the old layout) is written only
  once an entry exists;
- snapshots omit the field while empty, and import treats a missing field as empty.

## What the owner's schedule kit needs

1. Merge at a green head, one consensus merge at a time; build one release from that head.
2. All three producers (ws, Mango, CT101) run that binary before the activation height. A node on an older
   binary that sees `send_allowlist` scheduled halts at H with the #3093 upgrade-required notice instead of
   forking.
3. One #3277 schedule extension naming `send_allowlist` at H ≥ tip + 2,000 (`MIN_RUNUP_BLOCKS`),
   `release_commit` = the merged head, signed by the governance keys. `schedulable()` is `true`, so the
   binary's admission and the signing CLI accept it. Scheduling is the owner's signature only.
4. After H: `wrfcoin send-allowlist enable --key … --chain-id 888 --nonce 0 --allow <addr> --submitted-at
   <now> --out a.json`, then `wrfcoin send-allowlist submit --artifact a.json`, then `show`.

## What the determinism differential needs

- The committed fixture (`tests/integration/determinism_differential.rs`, `expected_roots.json`) must
  replay byte-identically on this binary with the feature unscheduled and with it scheduled but unused:
  the map stays empty, so no root moves.
- A scheduled run that exercises it: enable at H, a permitted and a refused transfer, a pending addition
  that matures, remove, disable that matures. Producer, applier and WAL roots must agree at every height
  (`native-chain/tests/send_allowlist_parity.rs` does this through the real producer for H = 3).
- Restart parity across H and across the first format-16 checkpoint (load a format-15 checkpoint, apply
  the first action, write, reload).
