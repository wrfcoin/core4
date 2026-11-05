# Bridge trust tiers (draft, no payout change)

Refs wrfcoin/core4-archive#3570 and wrfcoin/core4-archive#3568.
Ownership design: https://github.com/wrfcoin/core4/pull/51.

This is a non-earning preview. No bridge reward path is added. A later
consensus-visible payout multiplier requires a new `ConsensusFeature` proposed to be labelled non-schedulable, owner ratification, gate-off state-root parity and
producer/applier/WAL parity. Fixture ownership is never live source verification.

## Proposed preview computation

Use integer parts per million (PPM), with 1,000,000 equal to the native baseline.
The caller supplies authenticated observation-window aggregates: independent
neighbour agreement, uptime and completed probation days. Do not let a claimant
choose these inputs. Native peers must be selected independently of the bridge
operator; copied feeds cannot count as independent neighbours.

- Start at a maximum of 250,000 PPM; increase the ceiling by 25,000 PPM per
  completed day, to a maximum of 750,000 PPM after twenty days.
- Multiply that ceiling by neighbour agreement and uptime (each 0–1,000,000),
  using integer arithmetic and flooring only at the documented divisions.
  Disagreement, gaps or missing evidence reduce the result, down to zero.
- Cap the result by the native tier for the SAME cell and observation window.
  Zero native tier means zero bridge tier; it does not authorize a new scarcity
  bonus. Selection always prefers native-signed observations before weighting.
- Consume the chain-provided scarcity multiplier unchanged. Compute
  `floor(baseUnits * cappedTierPPM * scarcityPercent / (1_000_000 * 100))`.
  Cap the final amount by the corresponding native amount computed in the same
  order. Use arbitrary-precision integers to avoid overflow. The preview returns
  zero when its feature flag is off and never authorizes payment when on.

These coefficients are draft proposals for review, not owner-approved economics.
No bond amount, reward promise, or payout activation is implied.

## One scarcity rule

The authoritative rule remains
`shared-protocol/src/consensus/data_desert.rs::desert_multiplier` and the paid
`GET /network/scarcity` response (wrfcoin/core4#46), including its
`desert_bonus_percent` field and approximately 2-degree paid cell. Its
denominator is **100**,
although the Rust field is named `desert_bonus_bps`: 100 is neutral and 150 is
1.5 times. Do not reinterpret it as denominator 10,000 or reproduce station
counts / geographic bucketing in the backend. A missing or stale chain snapshot
means no preview, not a locally invented scarcity fallback.

The proposed backend preview would accept the already-resolved chain value and does not
calculate geography or counts. The data supplier must pin cell/window/chain
snapshot identity for both native and bridge tiers. No such supplier is wired
into payout here.

## Evidence required

Tests cover disabled behavior, invalid/missing input, rising reliability/uptime,
degradation, native cap, maximum age ceiling, integer composition order and
large base amounts. A UI must label this as a hypothetical testnet preview;
testnet tokens have no promised value. Activation requires live proof per source,
a ratified aggregation window and peer independence rule, and the gated
consensus implementation described above.
