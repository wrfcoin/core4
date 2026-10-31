# ADR: a burned transfer fee, switched on with sponsored first sends (owner GO 2026-11-01)

Status: accepted for build. Ships **unscheduled**. Scheduling is the owner's signature.

Companion: `docs/adr/2026-11-01-sponsored-first-sends.md` (core4#96, `paymaster`). Context: the
sinks ADR draft (core4#50, `docs/adr/2026-10-28-3562-sinks-verification.md`) and the owner's
2026-10-24 tokenomics answers (21M cap, decay with a perpetual tail, burn a defined protocol inflow).

## Decision

Every transparent transfer pays a small protocol fee, and the fee is **burned**. It is switched on
by a new feature, `ConsensusFeature::TransferFeeBurn` (`transfer_fee_burn`), in the **same release
and at the same height** as `paymaster`, so a newcomer's first send can be sponsored and nobody meets
a fee wall. The first fee is **0.00005 WRF (5_000 base units)**.

Sponsorship is meant for mainnet, where there is no faucet and coins have real value. Both pieces
are built and proven on testnet first.

## What the white paper says

WRFcoin white paper V0.2 (2018-07-01), §5 "Handling Data, Rewards, & Tokens":

> "Network development and the collection, storage, and processing of environmental data will
> always be necessary. Therefore, new Lotus WRF tokens will be mined continuously for as long as the
> network operates. This will require regularly scheduled token burns as the max supply is fixed."

§5.6 "Token Burning":

> "As tokens are mined continuously, this will require tokens to be burned at regularly scheduled
> intervals. [...] The primary source of tokens to be burned will come from the for-profit hardware
> division. Another portion of the tokens that are burned will have been paid as fees by users
> requesting private analysis."

The paper's cap was 100M; the chain's is 21M (owner 2026-10-24). The principle carries over: issuance
continues, so something must be burned. A burned transfer fee is the simplest "defined protocol
inflow" (owner answer, 10-24) that burns in proportion to use.

## How it relates to the sinks ADR (core4#50)

core4#50 §6.1 proposes `protocol_inflow_burn` (F5): burn 50% of each transaction fee, producer keeps
50%, plus forfeiture and clawback burns. Two facts change that row:

1. **No fee is charged on chain today.** `tx.fee` is never debited or credited in state apply; it only
   orders the mempool and feeds an admission floor that defaults to 0. So there is no fee to split.
2. **The owner chose 100% burn** for the transfer fee (2026-11-01). Producers are paid by the emission
   slice, not by fees.

So this change is the transfer-fee slice of F5 under its own name. F5 keeps the forfeiture and clawback
burns; core4#50 should drop its "50% of transaction fees" row or point here. It does not duplicate a
registered feature: core4#50 is docs only and registers none.

## Rules (consensus, from activation)

| Rule | Where |
|---|---|
| A transparent `Transfer` must declare `tx.fee >=` the fee in force at its block, or the transfer is refused (`SP_TRANSFER_FEE_BELOW_MINIMUM`) and the producer keeps it out of the block | `State::apply_transparent_transfer` via `consensus::transfer_fee::check_declared_fee` |
| The fee debited is the **protocol fee**, never the declared `tx.fee` | same |
| Without a sponsorship, the sender's spendable balance and balance must each cover `amount + fee`; the fee is debited after the transfer, both or neither apply | `check_sender_covers_amount_and_fee`, `burn_transfer_fee` |
| With a sponsorship `paymaster` accepts at that height, the sponsor's signed `fee` must be `>=` the transfer fee (`SP_TRANSFER_FEE_SPONSOR_BELOW_MINIMUM`); the sponsor pays its signed fee (burned) and the sender pays no fee | `apply_transparent_transfer` + core4#96's `commit_sponsorship` |
| The fee is credited to nobody and added to `supply_authorized_burning`; INV-SUPPLY-1 reads it as an authorized burn | `burn_transfer_fee` |

**Why the protocol fee and not `tx.fee`.** The sender's authorization signs `from|to|amount|nonce`
only. `tx.fee` is outside it, so any relayer or peer could raise `tx.fee` on a copy of a transfer.
Charging `tx.fee` would let a stranger burn a victim's balance. Charging a fee the chain fixes per
height means nobody but the chain decides what a sender loses. `tx.fee` must still declare at least
the fee, so a mined transfer says what it cost, and it still orders the mempool.

**The parameter.** `consensus::transfer_fee::TRANSFER_FEE_STEPS` binds a fee to a feature. The fee at
a height is the last step whose feature is active. The value is therefore set by the same owner-signed
schedule entry that switches the fee on (genesis for a new chain, a #3277 extension for 888). A later
change is a new row under a new feature name (`transfer_fee_burn_r2`), so every past block replays
under the fee it was mined with. A step must lie inside the sponsor fee bounds
(`MIN_SPONSORED_FEE..=MAX_SPONSORED_FEE`, 0.00001 to 0.01 WRF), so a sponsor can always cover it
(unit test).

**Supply.** The 21M cap is measured against `issuance_committed_supply()`, the sum of balances and
reserves held now (sinks ADR §1, §5). A burn lowers that sum, so it frees headroom under the cap, which
is what lets issuance continue as a perpetual tail. The `supply_authorized_burning` journal is not in
the state root (`state.rs`, INV-SUPPLY-1 notes); the balances it explains are.

## Which transactions pay

| Transaction | Pays? | Why |
|---|---|---|
| Transparent `Transfer` (wallets, CLI, faucet drips, any system-authority transfer) | **Yes** | Moves value between accounts. A faucet drip costs the faucet 0.00005 WRF |
| Sponsored transparent `Transfer` | Sponsor pays | core4#96 |
| Coinbase, `Reward`, `RewardClaim`, contribution/forecast reward decisions | No | Emission; charging would reduce block rewards |
| Contribution / forecast commit and reveal, oracle attestation and registration | No | Data and attester flows; earning must stay free |
| Node bond lifecycle, node service batch (node-pay receipts), node fraud | No | Node-pay settlement and bonds have their own model |
| Governance proposal / vote / execute, schedule extension, station registration and owner actions, slashing reports | No | Governance-signed or deposit-based; no balance transfer |
| Stake / Unstake | No | Locks value, does not move it to another account |
| Confidential transfer, shielded deposit/withdrawal | Not yet | Disabled at admission; they get a fee rule in the release that enables them |

Only the transparent-transfer arm reads the fee, so the other paths are untouched by construction.
The tests check that rewards and internal balance moves burn nothing.

## Admission and wallets

- **Mempool** (`add_transaction_with_signing_keys_at_height`): once the fee is in force at tip+1, a
  transparent transfer that declares less is refused with the same `SP_TRANSFER_FEE_BELOW_MINIMUM`
  text. P2P gossip passes the height, so peers screen the same way.
- **`POST /transactions/submit`**: new optional `fee` (base units). Once the fee is in force at tip+1,
  `fee` below it, or a sponsorship whose fee is below it, is refused with `400 FEE_BELOW_MINIMUM`
  ("this transfer costs a network fee of 0.00005 WRF (5000 base units), which is burned ...") before
  any replay or rate-limit state changes. When `fee` is absent the node declares the chain's fee. The
  node declares the fee as soon as it is scheduled, so a transfer queued just before activation and
  mined just after is not refused. The response adds `fee`, `fee_burned` and `fee_paid_by`.
- **`GET /network/info`** adds `transfer_fee`: `{feature, active, scheduled, next_height, base_units,
  wrf, burned, sponsorable}`. Wallets read this before a send.
- **CLI** (`wrfcoin wallet send`, interactive and plain): reads `transfer_fee`, shows
  `Network fee: 0.00005000 WRF (burned)`, subtracts it from the remaining balance, warns when the
  balance does not cover amount + fee, and sends `fee`. An older node that reports nothing shows
  "none"; a node it cannot ask shows "unknown", never "none".
- **Web and phone wallets**: separate PRs. They must show the fee and use amount + fee for "max"
  before the owner schedules this.

## Unscheduled behaviour

`transfer_fee_at(h)` is `None` at every height on a chain that does not schedule the feature (888,
777 and mainnet today). Then `tx.fee` is never read in apply, admission is unchanged, the node
declares `fee = 0` (byte-identical envelopes), and every state root is unchanged. The tests apply the
same transfer with declared fees 0, 1, 4_999, 5_000, 50_000 and `u64::MAX` and get one root.

## Tests

- `shared-protocol/tests/transfer_fee_burn.rs` (own process): unscheduled inertness; activation
  boundary (H-1 free, H charged); admission at tip+1; under-fee refusal leaves state untouched; amount
  + fee exact fit and one-unit short; over-declaring burns only the protocol fee; supply journal,
  `issuance_committed_supply` and INV-SUPPLY-1; sponsor pays, sponsor below minimum refused,
  undeclared sponsored transfer refused, sponsored newcomer with exactly the amount; rewards and
  internal moves burn nothing; property test over amounts and declared fees.
- `native-chain/tests/transfer_fee_burn_parity.rs` (two child processes, gate off and on at H = 3 with
  `paymaster`): the real producer, the P2P applier and WAL replay agree on root, balances and burn
  journal for blocks that burn fees, carry a sponsored first send and drop an under-fee transfer.
- `consensus::transfer_fee` unit tests: wire ordinal 26, not schedulable, step bounds.
- launch_api: `/transactions/submit` declares and enforces the fee at tip+1, `/network/info` reports it.
- CLI: preview shows the burned fee; `parse_transfer_fee` accepts only an active positive fee.

## Before scheduling (owner)

Schedule `transfer_fee_burn` and `paymaster` **in the same release, at the same height**. Never one
without the other.

1. core4#96 merged, and this change merged on top of it, CI green at the merged head, one merge at a
   time onto current main.
2. Flip both features to `schedulable = true` in one reviewed release (paymaster's own list: meter the
   sponsor signature in `verify_cost`, provision the sponsor key and service).
3. **Determinism differential** across the three producers (ws, Mango, CT101) on a binary with both
   features scheduled on a scratch chain, including blocks that burn fees and carry sponsorships.
4. **Sync and WAL replay**: a fresh node P2P-syncs and a restarted node replays its WAL across blocks
   with mined fee burns and sponsored transfers, reaching the producers' root (the parity test above
   is the unit-level version; this is the multi-node one).
5. **Verification budget**: the fee adds no signature. The sponsor signature is core4#96's item.
6. Wallets: web and phone show the fee and compute "max send" as balance minus fee; the backend relays
   pass `fee` and `sponsorship` through; the faucet budget accounts for 0.00005 WRF per drip.
7. All three producers upgrade together. Then the owner signs the schedule extension (both names, one
   height). No agent schedules it.

## Open questions for the owner

- Mainnet fee: keep 0.00005 WRF, or set a mainnet value now? Recommend: keep it; change later with a
  `transfer_fee_burn_r2` step.
- Strict wallets: refuse a submit that omits `fee` once active, instead of declaring the chain fee for
  it? Recommend: no (lenient) for testnet; the response says what was charged, and the CLI already
  sends `fee`.
- Should faucet drips be exempt? Recommend: no. It is 0.00005 WRF per drip, and an exemption would be a
  privileged sender rule in consensus.
