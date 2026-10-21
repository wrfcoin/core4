# Economic abuse model

**Status:** current-code baseline for #2141. This models controls found in the
live tree; it does not turn an unset configuration or external service policy
into a mainnet number.

## Reading the model

A capacity bound is not automatically an attacker price. Parameters labelled
`[testnet]` describe the current testnet/liveness posture. `[mainnet]` means a
code default or a decision that must be made for mainnet.

Source seams: `shared-protocol/src/config/settings.rs` (node policy),
`shared-protocol/src/mempool/pool.rs` (admission/ordering),
`shared-protocol/src/consensus/proof_of_publishing.rs` (fee-less and rewards),
`shared-protocol/src/common/constants.rs` (limits), and
`consensus-engines/src/selection/bft_finality.rs` (finality).

## Parameter table

| Parameter | Current value | Label | Controls | Consumer |
| --- | ---: | --- | --- | --- |
| `mempool_min_fee_rate` | 0 milli-units/byte | [testnet] | Default has no priced admission | #2144 |
| `mempool_max_size_bytes` | 50 MiB | [mainnet] config default | Local pending-byte bound | #2144 |
| `mempool_max_age_secs` | 14,400 (4 h) | [mainnet] config default | Pending-tx retention | #2144 |
| `mempool_max_txs_per_sender` | 1,024 | [mainnet] config default | Claimed-sender bound; sender is not authenticated | #2144 |
| `max_transaction_size` | 100,000 bytes | [mainnet] config default | Transaction payload size | #2143 |
| `max_block_size` | 1,000,000 bytes | [mainnet] config default | Configured block size | #2143 |
| `MAX_BLOCK_SIZE` | 32 MiB | [mainnet] source constant | Conflicts with config; reconcile before pricing | #2142/#2143 |
| `max_transactions_per_block` | 5,000 | [mainnet] config default | Block transaction count | #2143/#2144 |
| fee-less total | `50 * target_block_seconds` | [mainnet] consensus | Zero-fee reveal limit per block | #2144 |
| fee-less worker/block | `3 * target_block_seconds` | [mainnet] consensus | Per-worker reveal limit | #2144 |
| fee-less worker/epoch | `50 * target_block_seconds` | [mainnet] consensus | Per-worker epoch limit | #2144 |
| `WRFCOIN_MIN_WORK_FACTOR` | 0 testnet; positive mainnet floor | [testnet]/[mainnet] | Zero-fee receipt requirement; receipt does not prove external work | #2588/#2144 |
| user emission cap | live block reward / 100 per 86,400 s | [mainnet] consensus | Per-contributor rewards | #2145/#2148 |
| `MAX_BLOCK_EMISSION` | 3,200 WRF at genesis | [mainnet] consensus | Coinbase + contribution/forecast + gamification ceiling | #2145/#2148 |
| finality | `floor(2 * total_stake / 3) + 1` | [mainnet] consensus | Stake required to finalize | #2147 |
| state-byte price | unset | [mainnet] decision required | No storage rent/per-byte fee | #2142 |
| faucet identity price/cap | not in core4 | [testnet] external policy | Backend/operator responsibility | #2148 |
| `WRFCOIN_SYBIL_MAX_STATIONS_PER_IDENTITY` | 8 | [testnet] | Stations one identity (a non-operator bearer's wallet claim, else the contributor id) may commit for; edge refusal `SYBIL_STATION_LIMIT` | #2148 |
| `WRFCOIN_SYBIL_MAX_COMMITS_PER_STATION` | 120 per window | [testnet] | Commits one station may make per window; edge refusal `SYBIL_STATION_COMMIT_CAP` | #2148 |
| `WRFCOIN_SYBIL_WINDOW_SECS` | 3,600 | [testnet] | Window for the per-station cap and limiter entry expiry | #2148 |
| `WRFCOIN_SYBIL_MAX_TRACKED_ENTRIES` | 100,000 (ceiling 1,000,000) | [testnet] | Bound on limiter state; at capacity new keys are refused, never tracked | #2148 |
| per-operator epoch reward cap / minimum station age | unset | [testnet] decision required | Consensus-side reward eligibility; needs an owner policy table and an operator/station relation, feature-gated | #2148 |

## Vectors

### Transfer spam

- **Attacker cost:** default marginal fee is zero. With a configured nonzero
  floor, minimum fee is `ceil(rate * serialized_bytes / 1000)` base units.
- **Network cost:** signature verification, relay, up to 50 MiB local pending
  memory, and up to four hours retention.
- **Defence/parameter:** byte bound, TTL, fee-rate ordering, and 1,024 pending
  transactions per claimed sender. Sender rotation defeats the latter as an
  identity price.
- **Conclusion:** no finite economically profitable spam rate is established
  while the default floor is zero; only local capacity is bounded.

### Weather and sensor spam

- **Attacker cost:** testnet floor 0 allows no receipt. A positive floor checks
  a self-signed `work_factor`, not independently measured work (#2588), so it
  is not a priced external cost. Reward eligibility additionally requires
  provenance and oracle quorum.
- **Network cost:** reveal validation, replay/state tracking, fee-less slots,
  and oracle-attestation work.
- **Defence/parameter:** total, per-worker/block, and per-worker/epoch
  fee-less caps; station/provenance and quorum on the reward path.
- **Rate:** at the **launch posture** — genesis block time 14,400 s (4 h) —
  **720,000 total, 43,200 per worker/block, 720,000 per worker/epoch**. These
  scale down with target block seconds to 50 / 3 / 50 at terminal one-second
  blocks. Leading with the terminal figures understated the allowance that
  actually applies at launch by **14,400×**, which is the wrong direction for
  an abuse model: the reader sizes the threat against the smallest cap the
  chain will ever have, at the moment it has the largest. A fake
  station's economic rate is unquantified: receipt identity is unbound and no
  station/oracle operator cost is specified here.

### Permanent-state bloat

- **Attacker cost:** **unpriced.** No source seam charges retained state bytes
  or specifies storage rent, so cost to bloat by 1 GB cannot be calculated.
- **Network cost:** replicated disk, snapshots/WAL, state sync, and indexes.
- **Defence/parameter:** transaction/block ingress limits bound rate only.
- **Required measurement:** #2142 must define canonical retained-byte
  accounting (including indexes and state-transition overhead), then set a
  tested `[mainnet]` rent, fee, or storage budget.

### Signature-heavy transactions

- **Attacker cost:** fee ordering exists but default minimum rate is zero; no
  signature-count/gas price exists.
- **Network cost:** CPU verification can be amplified inside generic byte/count
  limits; fee revenue therefore cannot be compared with verification cost.
- **Defence/parameter:** generic limits only. The 1 MB config maximum and 32
  MiB source constant conflict, so neither is a safe pricing denominator.
- **Required measurement:** #2143 benchmarks worst accepted signature forms,
  establishes deterministic cost units, and prices them.

### Sustained congestion

- **Attacker cost:** zero under the default floor; otherwise configured rate
  times serialized bytes.
- **Network cost:** pending-pool occupancy and delayed honest traffic.
- **Defence/parameter:** 50 MiB, four-hour TTL, claimed-sender cap, and
  deterministic fee-rate ordering.
- **Conclusion:** the local mempool is bounded, not unbounded; no live
  congestion fee adjustment guarantees clearing. #2144 owns that policy.

### Epoch-boundary reward manipulation

- **Attacker cost:** reward-eligible contribution plus quorum; no separate
  epoch-entry fee.
- **Network cost:** strategically timed claims consume shared emission budget.
- **Defence/parameter:** per-contributor daily cap, `MAX_BLOCK_EMISSION`,
  deterministic allocation, and finalized-reward tracking.
- **Residual:** contributor identity is not established as a costly unique
  station/human identity. #2145 must test ordering, rounding, rotation, and
  budget exhaustion.

### Validator-set capture

- **Attacker cost:** `floor(2T/3)+1` active stake for total stake `T`; exact
  WRF cost is unavailable without an active set and stake distribution.
- **Network cost:** a supermajority can compromise safety/finality.
- **Defence/parameter:** strict stake-weighted 2/3+ finality. Count alone is
  not security when stake is concentrated.
- **Residual:** no authoritative current or target mainnet validator count is
  configured. #2147 must publish count/distribution before a numeric capture
  cost is claimed.

### Faucet farming and fake stations

- **Attacker cost:** faucet economics are external to core4. Contribution
  claims are capped per contributor/day and share block emission, but a station
  identity price is not parameterized.
- **Network cost:** emission, oracle work, state growth, and fee-less capacity.
- **Defence/parameter:** provenance/quorum, user emission cap, block emission
  cap, and fee-less limits.
- **Residual:** these do not establish station Sybil resistance. The ingress
  caps above (#2148) bound stations per identity and commits per station
  before the mempool; they price rate per key, not identity. The identity is
  a self-asserted `contributor` id unless a non-operator bearer carries a
  wallet claim, and `station_id` is only shape-validated: neither the edge,
  mempool admission nor `RegisterCommit` consults the station registry. A
  bearer can rotate `station_id` and `contributor` for fresh budgets, and
  those commits can land on-chain; commits for unregistered stations are
  `Unclassified` and earn nothing (#2460 gate). Refusing unregistered
  stations at the edge is an open owner decision (Refs #2148). Faucet claim policy
  stays external (the faucet is an emission slice here, #3039, not a
  service), and the reward-side per-operator epoch cap and minimum station
  age remain the consensus follow-up recorded on #2148.

## Honest-use envelope

| User | Must remain feasible | Parameters to protect |
| --- | --- | --- |
| Real station | Normal commit/reveal cycle and explicitly enabled bootstrap | work-factor posture; fee-less total/worker limits; fee floor |
| Wallet | Ordinary transfer and bounded burst before four-hour TTL | fee floor; mempool size/TTL; sender cap |
| Validator | Block/attestation operational traffic with verification headroom | byte/count limits; future signature schedule |

## Follow-on contract

This is the input table for [#2142](https://github.com/wrfcoin/core4/issues/2142),
[#2143](https://github.com/wrfcoin/core4/issues/2143),
[#2144](https://github.com/wrfcoin/core4/issues/2144),
[#2145](https://github.com/wrfcoin/core4/issues/2145),
[#2146](https://github.com/wrfcoin/core4/issues/2146),
[#2147](https://github.com/wrfcoin/core4/issues/2147), and
[#2148](https://github.com/wrfcoin/core4/issues/2148). A sibling changing a
parameter must update or explicitly supersede this model; an unset value is not
a mainnet value.
