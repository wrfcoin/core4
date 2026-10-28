# ADR: a higher scarcity tier for cells with no other station (draft)

Status: **Proposed (draft for owner ratification).** The code in the same PR is gated behind
`ConsensusFeature::DesertEmptyCellTier` (`desert_empty_cell_tier`). It is **not schedulable**:
`schedulable()` returns `false`. Turning it on is a separate, owner-approved step.

Date: 2026-10-28.

Refs: `wrfcoin/core4-archive#3563` (one scarcity rule), epic `wrfcoin/core4-archive#3560`. Builds on
the accepted `docs/adr/2026-10-23-3563-scarcity-display-rule.md` (public core4 #28), question 5:
"Yes: design a higher tier for no-station deserts … Follow-up decision needed: the tier's size and
trigger." This document proposes the size and the trigger.

Code references are to public `core4` main `cc7f4d4`.

## 1. The rule paid today

`shared-protocol/src/consensus/data_desert.rs:49` `desert_multiplier`. The rule works on ≈2° cells
(`DESERT_GRID_RESOLUTION_KM = 222`) and counts active registered stations per cell in
`state.station_counts`. A cell with fewer than 3 stations pays 150% (×1.5); any other cell pays
100%. Both reward paths call it with the block height (`proof_of_publishing.rs:5062`, contribution;
`:5437`, forecast). The display route `GET /network/scarcity` (`routes/scarcity.rs`, core4#46) calls
the same function on the same map.

## 2. What "no station" means at payout time

The scarcity ADR (line 106) notes the problem: "Today it cannot arise for a paying station, because
a paying station is itself counted in its cell." At payout, the first station into an empty cell
sees a count of **1**: itself.

So the trigger is **"no other active registered station in the cell"**, which means count ≤ 1
(`EMPTY_CELL_TIER_MAX_STATIONS = 1`):

| Cell count at payout | Meaning | Today | With the tier |
|---|---|---|---|
| 0 | the station is not counted (not active, or not yet registered); `/network/scarcity` shows this to a prospective first station | 150% | **175%** |
| 1 | the paid station is alone in its cell | 150% | **175%** |
| 2 | one other station | 150% | 150% |
| ≥ 3 | dense | 100% | 100% |

Count 0 never pays in practice: an inactive station is not counted, and `station_active_reward_gate`
records it at zero. It is listed so that the display and the payout agree for a station that has
not registered yet: it will be counted 1 once it is registered, and either way it is paid the tier.

When a second station registers in the cell, both stations drop to 150% from the next block. The
tier rewards being the **only** source of data in a cell, which is the paper's §5.1 scarcity intent.
It is not a permanent "first-mover" bonus.

## 3. The multiplier: 175% (×1.75)

- **Below the combined ceiling.** `compose_reward_with_multipliers` caps desert × hyper-local at
  `MAX_COMBINED_MULTIPLIER_BPS` = 2.0×. A 200% tier would pin every lone station to the cap, so
  hyper-local accuracy would stop moving its reward. 175% leaves room.
- **Above the ordinary desert bonus** by the same step as desert over baseline halved (+25 points
  over +50). Going from 2 stations to 1 is less coverage loss than going from 3 to 2 is coverage gain
  for the network, so the step should be smaller.
- **Never below the ordinary bonus.** The tier pays `max(175, desert_bonus)`, so a future governance
  increase of the desert bonus can never make a lone station earn less than a pair.

### Sybil and farming analysis

The tier makes "be alone in a cell" worth 17% more than "be one of two". The attacks it invites:

| Attack | Effect under the tier | Existing defence | Gap |
|---|---|---|---|
| Register a fake station at a fabricated location in an empty cell | +75% instead of +50% for a fake reading | Station registration is a governance-authorised transaction (`StationRegistration`, allowlisted, #2537); spatial-consistency scoring and quality slashing on readings | Location is self-declared. The tier raises the payoff of location spoofing by 1.75/1.5 ≈ 17%. Location proof is backlog `#3568` / `#3569` |
| Keep a cell "alone" by refusing a neighbour | Not possible: any registered station in the cell lowers the count for both | — | — |
| Move a station into empty cells by re-registering coordinates | Collects the tier wherever it lands | Re-registration is owner-signed and nonce-bound (`station_registration_nonce`, `station_owner_actions`) | No minimum dwell. **Owner question Q3** proposes one |
| One owner registers many stations, each alone in its own cell | Each is paid the tier | One station per cell per owner is automatic here (count ≤ 1) | An owner can still hold many lone cells. That is real coverage if the stations are real, and spoofing if not. **Owner question Q4** proposes a per-owner cap |

The ownership gate bounds this: payouts go to the registered owner from `STATION_OWNERSHIP_BINDING`
(h3,000 on 888), so every lone-cell station is tied to one funded owner account that quality slashing
can reach. The tier does not create a new attack. It raises the reward for location spoofing by
about 17%, and that is the defence the backlog already tracks.

## 4. How `GET /network/scarcity` reports it

The route keeps calling `desert_multiplier` at `observed_at_height + 1`, so per-cell and per-point
`multiplier_percent` already show 175 once the tier is active. The rule object gains three fields:

| Field | Value |
|---|---|
| `empty_cell_tier_max_stations` | `1` |
| `empty_cell_tier_percent` | `175` |
| `empty_cell_tier_active` | whether the tier applies at the next block (desert policy active **and** the feature active) |

A cell missing from `cells` has count 0. While the tier is active it pays a first station
`empty_cell_tier_percent`; otherwise it pays `desert_bonus_percent`. The point query for an empty
cell returns `station_count: 0, multiplier_percent: 175`. The test
`empty_cell_tier_is_reported_as_paid_once_active` pins that the view equals the payout at every
sampled height.

## 5. Implementation (this PR, gated)

- `ConsensusFeature::DesertEmptyCellTier`, appended at the end of the enum and of `KNOWN_FEATURES`;
  `schedulable()` returns `false`; `has_behaviour()` returns `true`.
- `desert_multiplier`: after the desert policy's own activation check, a count ≤ 1 with the feature
  active returns `max(175, desert_bonus)`. Unscheduled, the branch is never taken, and the function
  is unchanged at every height.
- `routes/scarcity.rs`: the three fields above.
- Gate-off parity: with the feature unscheduled, the function returns the same value for every
  count and height (unit test), and the existing reward parity tests pass unchanged.
- Gate-on parity: producer, P2P applier, extracted transitions and WAL replay reach the producer's
  `state_root` for a block paying the tier (`empty_cell_tier_pays_a_lone_station_with_parity`). The
  paid amount is ×1.75/×1.5 the tier-off amount, within 2 base units of rounding.

## 6. Questions for the owner (each with a recommended answer)

1. **Trigger:** count ≤ 1 at payout ("no other active registered station in the cell")?
   *Recommended: yes.*
2. **Size:** 175% (×1.75)? *Recommended: yes. 200% would saturate the 2.0× combined ceiling.*
3. **Dwell:** should a station have to keep its registered cell for a minimum period before it earns
   the tier, so that re-registering coordinates does not chase empty cells? *Recommended: yes, one
   node-share epoch (512 blocks), in a follow-up gated slice. It needs a per-station
   "location since" height in state, so it is not in this PR.*
4. **Per-owner cap:** should one owner earn the tier in at most N cells? *Recommended: not until
   location proof (#3568/#3569) exists, then N = 3. Without location proof, a cap only pushes
   farmers to more owner accounts.*
5. **Forecast path:** the forecast reward path also calls `desert_multiplier`, so a forecaster
   station alone in its cell earns the tier too. Keep that? *Recommended: yes, one scarcity rule
   (the point of #3563).*
6. **Activation:** schedule it through a governance-signed extension on 888 after ratification?
   *Recommended: yes. Rehearse it on a test chain first with the parity test above.*
