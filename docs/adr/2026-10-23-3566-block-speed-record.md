# ADR: block speed — governed vote or a SUPERSEDED fixed schedule

Status: **Proposed — not ratified.** A decision record for the owner. It records no decision and
changes no schedule, emission or governance surface.

Date: 2026-10-23.

Refs: `wrfcoin/core4-archive#3566`, a private backlog issue. It asks for either a governed
block-speed vote as in the white paper, or an explicit SUPERSEDED record that keeps the fixed
schedule. Companion: `2026-10-23-3561-tokenomics-reconciliation.md`, because block time is
coupled to emission.

Code references are to public `core4` main `bde93b2`. "PoP" is
`shared-protocol/src/consensus/proof_of_publishing.rs`.

## Context

### White paper (V0.2, §4.1)

> "At the beginning, each block will be open for two hours. Once the network is operating
> comfortably and the community votes for a block speed increase, the time period will decrease …
> Upon the 43rd successful vote … the block time will be cut down to one second. The network will
> operate under the chosen block duration for a minimum of one week and a maximum of a year before
> an automatic speed increase. The community must wait 72 hours after implementing a speed
> increase before voting to perform a similar action."

The paper therefore already mixes the two models: votes to speed up, **plus** an automatic step
after at most a year.

### As implemented

| Item | Value | Source |
|---|---|---|
| Schedule | Fixed by **height**: 4 h (from 0) → 2 h (1,000) → 1 h (5,000) → 10 min (25,000) → 2 min (100,000) → 30 s (500,000) → 15 s (1,000,000) → 5 s (5,000,000) → 1 s (10,000,000, terminal). That is 9 tiers, against the paper's 2 h start and 43 votes. | `shared-protocol/src/common/constants.rs:73-83` `BLOCK_TIME_PROGRESSION`; `compute_target_block_time` PoP:2355 |
| Governance control | **None found.** The table is a compile-time constant. | — |
| Emission coupling (default schedule) | Per-block reward = `base × block_time / 14,400` (PoP:700-715), so issuance **per second** does not depend on the tier. However, the ⅔ reductions happen every 126,000,000 **seconds**, and the code recovers seconds from heights **through this table**. | `constants.rs:59-72`: "Editing a row changes tokenomics, not just block cadence." Pinned by `the_emission_parameters_satisfy_the_21m_constraint_2492` and `reduction_boundary_is_consistent_in_each_progression_phase` (`shared-protocol/tests/economic_property_tests.rs:216`). |
| Emission coupling (live testnet 888) | A **flat** 22.22222222 WRF **per block** from the genesis-recorded 600-second tier (`chain_binding.rs:224-234`). It is not scaled by block time. | `docs/testnet/GENESIS_888.md` |
| Observed cadence | **Not established here.** The node's operator status surface assumes a 77-second interval, documented as "not a consensus timing parameter" (`native-chain/src/bin/launch_api/routes/network.rs:516-517`). This record did not trace what sets the live production cadence. | — |

**Unverified risk to measure before any ruling.** On 888 the reward is fixed *per block*, so
realised issuance per day scales with how many blocks are actually produced. A faster cadence,
whether from a later table tier or from production timing, raises issuance proportionally. The
≈ 1.168M WRF/year figure in the companion ADR assumes one block per 600 s. The realised cadence and
realised issuance should be measured from chain data before the owner rules on block speed or on
emission.

## Options

| Option | What | Consequences |
|---|---|---|
| **B-S** SUPERSEDED: keep the fixed-height schedule | Record that the paper's vote model is replaced by a predetermined schedule with the same 1-second endpoint. | No code. Predictable for operators and economics. Departs from the paper's community control. The table stays a monetary constant that only a binary release plus activation can change. |
| **B-V** Governed vote (paper model) | A governance proposal type `BlockSpeedStep` with fields `next_tier` and `activation_height`. Rules from the paper: dwell at least one week, cooldown 72 hours, an optional automatic step after one year. Votes come from the existing governance machinery. | Larger build. The height → seconds mapping must come from a **recorded history of tier changes in state**, not the constant table, or emission timing becomes unpredictable. On 888-style flat per-block rewards, a vote to speed up blocks is also a vote to raise issuance, unless reward becomes per-second (see coupling below). Needs a new proposal type, an activation feature, and determinism and replay tests. |
| **B-H** Hybrid: fixed schedule, with governance that can only **hold** a step | Governance may delay the next scheduled tier, for example while the network is not "operating comfortably", but cannot accelerate. | A small surface, and close to the paper's safety intent. The seconds history must still be state-recorded, so it carries the same emission-coupling work as B-V, only less of it. |
| **B-T** Time-based tiers (wall clock instead of height) | Tiers switch at wall-clock dates. | Ties cadence to calendar rather than load. Deterministic only if dates are evaluated against consensus time (median-time-past). Low community control. |

### Emission decoupling (needed by B-V, B-H and B-T)

Any option in which the tier at a height is not known from a constant table must:

1. **Record each tier change** (height, seconds) in state, so `elapsed_seconds_at_height` derives
   from that record. Today it uses `BLOCK_TIME_PROGRESSION` (PoP:553).
2. **Make per-block rewards proportional to block time** wherever they are not already. The
   default schedule already is; the 888 flat reward is not.

Without both, block speed becomes a back door to changing issuance.

## Questions for the owner

1. **SUPERSEDED (B-S), vote (B-V), hold-only (B-H) or time-based (B-T)?**
2. If not B-S: keep the paper's limits (dwell from one week to one year, 72-hour cooldown,
   automatic step after one year), or set others?
3. Should block-speed changes be forbidden from changing issuance, which requires the decoupling
   above, or is a coupled change acceptable when governed?
4. Before ruling: measure the testnet's realised block cadence and realised issuance. Who runs that
   measurement, and should the ruling wait for it?
