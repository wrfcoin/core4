# ADR: tokenomics reconciliation with the 2018 white paper

Status: **Accepted (owner, 2026-10-24).** This is an options document for the owner. It records **no
decision**, and it authorizes no change to constants, genesis, emission, shares, burns or
deployment. Every economics change it describes is consensus-breaking and needs separate review.

Date: 2026-10-23.

Refs: `wrfcoin/core4-archive#3561`, a private backlog issue. It asks for an owner-decision ADR that
reconciles the chain's tokenomics with the white paper, or records a deliberate departure, item by
item: max supply, emission curve, allocations, burns (including the stale-wallet burn), and the
destination of the 15% emission slice. Related private issues:

- `#3562`: a sinks program paying at least 15% to verified environmental mitigation.
- `#3577`: sinks via existing third-party verification programs.
- `#3563`: one scarcity rule.
- `#3565`: one submission per device per block.
- `#3566`: block speed.

Owner direction (2026-10-15): "follow the white paper going forward", allowing for how the project
has grown since 2018. The paper is a direction, not a specification.

All code references are to public `core4` main `bde93b2`. "PoP" is
`shared-protocol/src/consensus/proof_of_publishing.rs`.

## 1. Current state, as implemented

### 1.1 Supply

| Item | Value | Source |
|---|---|---|
| Base units | 10^8 per WRF | `shared-protocol/src/common/constants.rs:39` `WRF_UNITS_PER_COIN` |
| Max supply | **21,000,000 WRF** | PoP:2314 `MAX_SUPPLY`; enforced as a pay-nothing deferral in `shared-protocol/src/blockchain/state.rs:4234,4353,4521` and refused in `native-chain/src/bin/launch_api/routes/rewards.rs:295` |
| Genesis supply | 0; the baseline is adopted from held balances | `state.rs:1261,1581,6419-6433` |
| Team / foundation / sale allocation | **None** | No allocation or vesting constants. `docs/testnet/GENESIS_888.md`: "There is no list of arbitrary premine recipients." |
| Testnet 888 one-time faucet credit | 100,000 WRF to the recorded faucet address | `GENESIS_888.md` (`testnet_faucet_allocation_base_units = 10000000000000`); bounded ≤ `MAX_SUPPLY` at `shared-protocol/src/crypto/chain_binding.rs:196-201` |

### 1.2 Emission

**Default schedule.** This is what a non-testnet genesis would get, per ADR-002 (Accepted).

- `INITIAL_BLOCK_REWARD` = **800 WRF** per 14,400-second block (PoP:533, PoP:452).
- The reward is scaled by block time: `base × block_time / 14,400` (PoP:700-715). Emission per
  second is therefore independent of the block-time tier.
- Every `EMISSION_REDUCTION_PERIOD_SECONDS` = 126,000,000 s (about 3.99 years) the reward keeps
  ⅔, floored (PoP:473, PoP:511-512).
- That gives **7,000,000 WRF in the first period (about 1,752,000 WRF/year)**, a geometric series
  that sums to 21,000,000. This is pinned by `the_emission_parameters_satisfy_the_21m_constraint_2492`
  (PoP:19191).
- Floor rounding leaves the realised total slightly below the cap (`total_scheduled_emission`,
  PoP:636).
- `MAX_BLOCK_EMISSION` = 3,200 WRF (PoP:2347).

**Live testnet, chain 888.**

- A genesis-recorded **flat** reward of **22.22222222 WRF per 600-second block** replaces the
  decaying curve (`chain_binding.rs:224-234`, bounded at 3,333,333,333 base units at :191; the
  owner selected it on 2026-10-19 per `GENESIS_888.md`).
- That is a budget of about **1,168,000 WRF/year**, not a promise that every slice is minted.
- If fully minted, it would reach the 21M cap in about 18 years.
- Both figures assume one block per 600 s. The reward is paid **per block**, so realised issuance
  scales with the actual production cadence, which this ADR did not measure. The companion
  block-speed record (`2026-10-23-3566-block-speed-record.md`) asks for that measurement before
  any ruling.

**Block time.** `BLOCK_TIME_PROGRESSION` (`constants.rs:73-83`) is a fixed-height schedule:
4 h → 2 h → 1 h → 10 min → 2 min → 30 s → 15 s → 5 s → 1 s. It is coupled to emission
(`constants.rs:59-72`: "Editing a row changes tokenomics, not just block cadence"). No governance
parameter changes it. The companion record `2026-10-23-3566-block-speed-record.md` covers it.

### 1.3 Allocation of each block's emission (the role partition)

| Role (`EmissionRole`, PoP:791) | Mainnet share (PoP:830) | Testnet share (PoP:831; used by 888) |
|---|---|---|
| Contribution (stations) | 70% | 69% |
| **Attester** (oracle attesters) | **15%** | **15%** |
| Producer | 10% | 10% |
| Node service | 5% | 5% |
| Faucet | 0% | 1% |

- The split uses integer arithmetic with a largest-remainder tie-break (PoP:914). The rationale is
  in ADR-003 role-earnings (`docs/architecture/ADR-003-role-earnings-architecture.md:136-146`).
- **There is no `Sink` role.**
- 25% of the contribution slice is paid as coinbase (PoP:1159).
- Scarcity multipliers (data deserts) apply inside the contribution slice. See the companion
  scarcity ADR.

### 1.4 Burns, fees and forfeiture

| Mechanism | State |
|---|---|
| Burn accounting | `supply_authorized_burning` journal (`state.rs:1267`; outside the state root per :1255). |
| Slashing burn | Yes (`state.rs:5402`). |
| Paymaster sponsored-spend burn | Yes (`state.rs:5882`, `accounts/paymaster.rs:192`; behind the `paymaster` feature). |
| Transaction fees | A `fee` field exists. No consensus fee burn or fee split was found. ADR-003 says producers "keep priority fees on top" and that its stage "adds no consensus fee debit or burn". |
| Node-share fraud forfeiture | The bond moves to a non-spendable protocol account and still counts toward supply. No burn and no reporter bounty. |
| Scheduled burn | **None.** |
| Stale-wallet burn | **None.** |

### 1.5 How economics values can change today

| Kind | Examples | How it changes |
|---|---|---|
| Hard constants | `MAX_SUPPLY`, reward, ⅔ ratio, reduction period, block-time table, share tables, coinbase share, per-user caps, data-desert grid | Binary release **plus** an activation height. No `ConsensusFeature` changes the emission curve or the supply cap today (`crypto/protocol_support.rs:55-78` lists 22 known features). |
| Governance parameters | `ScarcityPolicy`, quality and task-assignment policies | A governance parameter update. |
| Genesis config | Testnet flat reward, faucet address and allocation, activation schedule, node-share timing | Fixed at a chain's genesis; a new genesis to change. |

Activation uses the genesis activation schedule plus governance-signed `ScheduleExtension`
(`chain_binding.rs:807,962`). A binary that does not know a scheduled feature halts cleanly.

## 2. White-paper intent (V0.2, July 2018)

| Section | Paper |
|---|---|
| 7.1 | **100,000,000 WRF max supply**: 15,000,000 reserved for foundation, team, advisors and early backers (3,000,000 released per year for 5 years); **45,000,000 mined** through data collection, storage, processing, sinks and software development; 40,000,000 available in a token offering. |
| 5.1 | **450,000 WRF per year** (37,500 per month) "at full throttle". Each of the first twelve data-stream sub-chains adds 1/12 of the throttle, so mining the 45M would take about 100 years. Rewards are weighted by data scarcity, human impact and network demand. |
| 1, 5.5 | **"At least 15% of the tokens mined from each block are reserved for sinks"**: litter clean-up, direct air capture, habitat restoration and similar work. Sinks are also funded by private-analysis fees and a crowdfunding portal. |
| 5.6 | Supply limited to 100M "so WRF is not a deflationary asset". Monthly scheduled burns start at 1% of rewards distributed and rise to 100% at max supply. Burn sources are hardware-division revenue, private-analysis fees and buy-backs. Wallets stagnant for over a decade would be burned. |
| 7.2 | Wide distribution through airdrops, region-bound faucets and scarcity-weighted mining. |
| 9.1 | The paper itself notes that the token's legal classification (utility, security, cryptocurrency) could be judged differently by different parties. |

## 3. Gaps

| # | Item | Paper | Chain | Gap type |
|---|---|---|---|---|
| G1 | Max supply | 100M | 21M | Contradiction |
| G2 | Emission curve | Flat 450k/yr, sub-chain throttled | 1.752M/yr decaying ⅔ per ~4 yr (default); flat 1.168M/yr budget (888) | Contradiction |
| G3 | Allocations | 15M team (vested), 40M offering | None (pure emission; testnet faucet only) | Contradiction (the chain is a "fair launch") |
| G4 | 15% slice | Sinks | Attesters | Contradiction; sinks absent |
| G5 | Scheduled burns | Monthly, 1% → 100% | None | Missing |
| G6 | Stale-wallet burn | > 10 years | None | Missing |
| G7 | Fee-funded sinks / burns | Private-analysis fees to sinks and burns | No fee burn or split | Missing |

The paper's numbers are internally coupled. A 100M cap with only 45M mined only works because 55M
are pre-allocated (G1 + G3), and the monthly burns exist because mining continues "forever" toward
a cap (G5). Ruling on one item constrains the others. §5 groups them into coherent packages.

## 4. Options per item

Each option lists its consequences. **None is recommended.**

### O1 Max supply (G1)

| Option | Consequences |
|---|---|
| **O1-a** Keep 21M | No code change. Aligns with ADR-002 and its tests. Departs from the paper; record it as a deliberate departure. |
| **O1-b** 100M (paper) | Requires changing the emission curve too (O2), or 100M is never reached. Without allocations (O3), 55M has no source. Rewrites ADR-002 and its pinned tests. |
| **O1-c** Another figure | Any cap must be re-derived with the curve; the same coupling applies. |

### O2 Emission curve (G2)

| Option | Consequences |
|---|---|
| **O2-a** Keep the decaying ⅔ / ~4-year curve | Front-loaded: about a third of supply in the first ~4 years. Rewards early stations. Inflation trends to zero. |
| **O2-b** Flat 450k/yr (paper) | Long-tail, back-loaded issuance (about 100 years for 45M). Much smaller per-block rewards than today: about 51 WRF per hour vs 200 WRF per hour in period 1. Station rewards fall about 3.9× unless shares change. |
| **O2-c** Flat rate with a cap (888-style) | Already proven on the testnet (flat 22.22/600 s). Simple to explain. Reaches the cap at a known date, after which only fees and sinks fund roles. |
| **O2-d** Decaying curve with a perpetual tail | Funds roles forever. Requires burns (O4) to keep a cap. Closest to the paper's "not deflationary" intent. |

### O3 Allocations (G3)

| Option | Consequences |
|---|---|
| **O3-a** None: pure emission, as today | Simplest legally and in public trust ("everything is earned"). The project has no treasury for development or sinks beyond what emission roles fund. |
| **O3-b** Paper allocations (15M team, vested; 40M offering) | **Needs legal counsel before any design work.** A token offering and investor allocation raise securities-law questions the paper itself flags (9.1). The chain has no vesting machinery. This ADR does not propose either. |
| **O3-c** Emission-funded pools instead of a premine | For example a governed "development" or "sinks treasury" role paid per block like the faucet slice. No premine. It is paid only as work happens and stays visible on-chain. It needs a new `EmissionRole` and governance over spending. |

### O4 Scheduled burns (G5) and fee burns (G7)

| Option | Consequences |
|---|---|
| **O4-a** No scheduled burn | The cap plus curve already bounds supply under O2-a / O2-c. Burns still happen through slashing and the paymaster. |
| **O4-b** Paper burns (monthly, 1% → 100%) | Only meaningful with perpetual issuance (O2-d). Needs a burn source the protocol controls. The paper's sources (hardware revenue, buy-backs) are off-chain and treasury-funded. |
| **O4-c** Burn a defined protocol inflow | For example a share of transaction fees or of future private-analysis fees. Deterministic and on-chain. Creates a fee policy where none exists today (producers currently keep fees). |

### O5 Stale-wallet burn (G6)

| Option | Consequences |
|---|---|
| **O5-a** No | Matches common self-custody expectations. Lost coins stay lost and count toward supply. |
| **O5-b** Yes, after N years of inactivity | **Irreversible and controversial.** Needs a consensus definition of "activity" (sends only? receives? signed heartbeat?), long notice, and an exemption story for escrow, bonds and non-spendable accounts. It interacts with the forfeiture account, which must never be burned or "re-activated". |
| **O5-c** Reclaim to sinks instead of burning | Same activity questions. Redirects lost coins to the paper's mission rather than destroying them. Same controversy. |

### O6 Destination of the 15% slice (G4)

The paper reserves at least 15% of each block for sinks. The chain pays 15% to attesters.

| Option | Shares (mainnet: contribution / attester / producer / node / sink) | Consequences |
|---|---|---|
| **O6-a** Keep attesters at 15%, no sink slice | 70 / 15 / 10 / 5 / 0 | Departs from the paper. Sinks could only be funded from non-emission sources (fees, crowdfunding). |
| **O6-b** Add sinks carved from contributions (as the testnet faucet was) | 55 / 15 / 10 / 5 / 15 | No other role changes. Stations earn about 21% less. That is the most visible change to the people the network depends on. |
| **O6-c** Add sinks pro-rata from all roles | ≈ 59.5 / 12.75 / 8.5 / 4.25 / 15 | Shares the cost. Every role's documented share changes, and attester and producer economics need re-checking against their bonds and hardware costs. |
| **O6-d** Swap: the 15% becomes sinks, and attesters are funded elsewhere | 70 / 0 / 10 / 5 / 15 | Literal paper alignment. Removes the attester incentive the oracle trust model relies on, so attesters would need fee or contribution funding. Highest security risk. |
| **O6-e** A smaller sink slice now, growing by governance | for example 70→65 / 15 / 10 / 5 / 5 | Lets the sinks program (`#3562`, `#3577`) prove verification before large amounts flow. Does not meet "at least 15%" until it grows. |

Whatever the slice, **an unclaimed sink slice must mint zero**, as the faucet slice already does
(ADR-003 role-earnings:549-585). No sink money should exist until verified outcomes claim it, and
verification is designed in `#3562` / `#3577`. Payouts tied to third-party programs must not be
marketed as carbon offsets without counsel (`#3577`).

## 5. Coherent packages (for orientation only)

| Package | O1 | O2 | O3 | O4 | O5 | O6 | Summary |
|---|---|---|---|---|---|---|---|
| **P1 "Keep, add sinks"** | 21M | decaying | none | none | no | b, c or e | Smallest change. Records departures on cap, curve and allocation; adopts the paper's sinks. |
| **P2 "Paper, minus the offering"** | 100M | flat or tail | emission-funded pools (O3-c) | fee burn (O4-c) | optional | b or c | Moves toward the paper's numbers without a sale. Needs new emission math and burns. |
| **P3 "Paper as written"** | 100M | flat 450k | 15M + 40M | monthly burns | yes | d | Requires legal counsel first. Largest rewrite. |

## 6. Migration considerations

- **Mainnet has no genesis yet.** Chain 888 is the public **testnet**, and its tokens carry no
  monetary value (`GENESIS_888.md`). Cap, curve, allocations and share tables can be chosen
  **at mainnet genesis** with nothing to migrate. That is the cheapest moment, and **after value
  accrues on mainnet these items become effectively irreversible**. The owner should rule before
  the mainnet genesis inputs are frozen.
- **Changing the live testnet (888) without a new genesis.** Every item above is a hard constant or
  genesis value (§1.5). Changing it on 888 needs a binary release plus a new `ConsensusFeature`
  activated at a future height through the governance-signed schedule extension. The feature
  would select the new share table or curve from height H onward, never retroactively.
  - The share table and curve are **state-root material**. The change needs gate-OFF parity tests,
    a state-root differential and replay tests, following the pattern of
    `docs/adr/2026-10-19-3290-forecast-identity-binding.md`.
  - The simpler alternative for the testnet is to rehearse the ratified economics on a **new
    testnet genesis**, as 888 itself did with its flat reward.
- **New roles.** A `Sink` (or treasury) role changes `EMISSION_ROLE_COUNT` and the role order used
  for remainder tie-breaks (PoP:778, :813). That is wire- and root-affecting, and tests pin it.
- **Pinned tests.** `the_emission_parameters_satisfy_the_21m_constraint_2492`,
  `the_reward_sequence_is_exactly_the_floored_two_thirds_chain_2492` and the economic property
  tests would change deliberately under any O1/O2 change, and must not be "fixed" to pass.
- **Burns and the stale-wallet rule** need new consensus state (activity tracking) and are the
  hardest to undo.
- **Stale documentation.** The doc-comment reward table at PoP:664-668 still shows 50-WRF-era
  numbers whatever the ruling. It should be corrected separately; that is a comment, not
  consensus.

### Public statements that would become wrong

Follow-up issues are to be filed after ratification; this ADR does not fix them.

| Ruling changes… | Documents that state the current value |
|---|---|
| Cap (O1) or curve (O2) | `docs/architecture/ADR-002-emission-reduction-schedule.md`, `docs/architecture/ADR-003-role-earnings-architecture.md`, `docs/architecture/ADR-004-reward-activation-roadmap.md`, `docs/security/PROTOCOL_SPEC.md`, `docs/security/KANI_PROOFS.md`, `docs/security/THREAT_MODEL.md`, `docs/CODE_STANDARDS.md`, `docs/code-standards/solidity-standards.md` |
| Shares (O6) | `ADR-003-role-earnings-architecture.md` (table at 136-146), `ADR-004-reward-activation-roadmap.md`, any website or app copy quoting role percentages |
| Testnet 888 economics | `docs/testnet/GENESIS_888.md`, `GENESIS_888_EVIDENCE.md` |

(The public frontend source was searched for the supply cap and does not state it.)

## 7. Questions for owner ratification

Please answer each item. "Keep as implemented, recorded as a deliberate departure from the
paper" is a valid answer to every one.

1. **Max supply:** 21M (keep), 100M (paper), or another figure?
2. **Emission curve:** decaying ⅔ per ~4 years (keep), flat (paper's 450k/yr, or 888-style), or
   decaying with a perpetual tail?
3. **Allocations:** none (keep), emission-funded pools (O3-c), or the paper's allocations? The
   paper's allocations are subject to legal counsel first.
4. **Scheduled or fee burns:** none, the paper's monthly schedule, or a burn of a defined protocol
   inflow?
5. **Stale-wallet rule:** none, burn, or reclaim-to-sinks? If either of the last two, what counts as
   activity, and after how long?
6. **15% slice:** which of O6-a…e, and if sinks get a slice, how large at launch?
7. **Where to apply:** at mainnet genesis only, or also on the live testnet (feature-gated at a
   height, or by a new testnet genesis)?
8. **Who signs off,** and should the ruling wait for the sinks verification design (`#3562`,
   `#3577`) so the slice size is set with a verified claim path in hand?

## Decision (accepted 2026-10-24)

Owner ruling, 2026-10-24: "Merge as accepted, recommended answers." Each open question gets the
answer this document itself recommends, quoted with its line. Where the document makes no
recommendation, the question was left open for the owner and is answered below as `Decision (owner, 2026-10-24)`: none was invented.

This ADR is accepted as the reconciliation record. It recommends no option (line 140: "**None is
recommended.**"), so every economics question was left for the owner and is answered below. Nothing here changes what pays
on chain: any ruling on these items is policy only, and implementation requires a separate consensus
PR + owner deploy. Line 249 ("Keep as implemented, recorded as a deliberate departure from the
paper") is offered as a valid answer to every question, not as a recommendation.

1. *Max supply: 21M (keep), 100M (paper) or another figure?* **Decision (owner, 2026-10-24): Keep 21M.**
2. *Emission curve: decaying ⅔ per ~4 years, flat, or decaying with a perpetual tail?* **Decision (owner, 2026-10-24): Decay with a perpetual tail. Follow-up decision needed: a 21M cap and a perpetual tail must be reconciled (tail funded by burned/recycled supply, or asymptotic to the cap). Cadence measurement (see the block-speed record) precedes any numbers.** Relevant text (not a recommendation): line 63: "asks for that measurement before". The
   doc defers to the block-speed record's cadence measurement before any ruling.
3. *Allocations: none, emission-funded pools (O3-c), or the paper's allocations (legal counsel
   first)?* **Decision (owner, 2026-10-24): O3-c: emission-funded pools (no premine).**
4. *Scheduled or fee burns: none, monthly schedule, or burn of a defined protocol inflow?*
   **Decision (owner, 2026-10-24): Burn a defined protocol inflow.**
5. *Stale-wallet rule: none, burn or reclaim-to-sinks; what counts as activity, after how long?*
   **Decision (owner, 2026-10-24): Burn, after long inactivity. Follow-up decision needed: the definition of activity and the inactivity period are still to be set.**
6. *15% slice: which of O6-a…e, and how large a sink slice at launch?* **Decision (owner, 2026-10-24): O6-e: a small sink slice now (e.g. 65/15/10/5/5), grown by governance. An unclaimed sink slice still mints zero.**
   Whatever the slice, the doc already requires that an unclaimed sink slice mints zero (see "Stated
   in every option" below).
7. *Where to apply: mainnet genesis only, or also the live testnet (feature-gated or new genesis)?*
   **Decision (owner, 2026-10-24): Also the live testnet, feature-gated.** Relevant text (not a recommendation): line 212: "That is the cheapest
   moment"; line 222: "The simpler alternative for the testnet is to rehearse the ratified
   economics". These lines compare costs; they are not labelled a recommendation.
8. *Who signs off, and should the ruling wait for the sinks verification design (#3562, #3577)?*
   **Decision (owner, 2026-10-24): The owner ratifies, after the sinks verification design (#3562, #3577) is in hand; ratification waits for that design. Implementation is feature-gated on the live testnet.**

Stated in every option, so accepted as written (no owner choice involved):

- line 195: "**an unclaimed sink slice must mint zero**" (policy only; implementation requires a
  separate consensus PR + owner deploy).
- line 196: "No sink money should exist until verified outcomes claim it" (policy only;
  implementation requires a separate consensus PR + owner deploy).
- line 198: "marketed as carbon offsets without counsel" (communications rule).
