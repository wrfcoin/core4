# Design: a governed, issuance-neutral block-speed vote (B-V)

Status: **Proposed (draft design for owner ratification).** No code. It follows the accepted
block-speed record `docs/adr/2026-10-23-3566-block-speed-record.md` (public core4 #28), decision 1
(**B-V**: a governance vote, tier change recorded in state, issuance-neutral) and decision 2 (limits
set later, after the cadence measurement). This document designs the mechanism and leaves the
limits to the owner.

Date: 2026-10-28.

Refs: `wrfcoin/core4-archive#3566`, epic `wrfcoin/core4-archive#3560`. Code references are to public
`core4` main `cc7f4d4` and public `infra` main `5f1ca0e`. "PoP" is
`shared-protocol/src/consensus/proof_of_publishing.rs`.

## 1. What sets the pace today

| Layer | What it is | Source |
|---|---|---|
| **Operations, the actual pace on 888** | The `block-heartbeat` service calls `POST /blocks/mine` on a fixed deadline. `BLOCK_HEARTBEAT_INTERVAL_SECONDS` defaults to **77** and is refused outside **30–600 s** | infra `testnet/docker-compose.block-heartbeat.yml:15`, `testnet/services/block-heartbeat/heartbeat.cjs:22-23` |
| Operator status only | `RESOURCE_CHAIN_BLOCK_INTERVAL_SECS = 77`: "Operator visibility policy (#3599), not a consensus timing parameter" | `native-chain/src/bin/launch_api/routes/network.rs:516-517` |
| Consensus constant, not followed by 888's cadence | `BLOCK_TIME_PROGRESSION`, a fixed-height table: 4 h → 2 h → … → 1 s | `shared-protocol/src/common/constants.rs:73-83` |
| Consensus, where the table is still read | the default schedule's per-block reward `base × block_time / 14,400` (PoP:700-715); `elapsed_seconds_at_height` (PoP:559), and through it the ⅔ reduction periods; `max_future_drift = max(15, target / 4)` (PoP:2373); the weather temporal check's per-block allowance (`weather/validation.rs`, `compute_target_block_time`) | — |
| 888 reward | Genesis-recorded flat **22.22222222 WRF per block** (the "600 s tier"), **not** scaled by time | `crypto/chain_binding.rs:224-234` |

No consensus rule bounds block spacing from below. The cadence is an operator setting, and the
cadence on chain is not established by any consensus check.

**What a measurement shows.** A read-only LEAD sample of 888 blocks 1–2775 (2026-10-25, built
from public `GET /chain/block/{h}` data and reproducible by anyone) found:
- every interval from height 4 onward was 77 s, matching the heartbeat default;
- one producer made every block;
- realised mint was about 14% of the 22.22 WRF/block ceiling, because only the producer and faucet
  slices mint every block, and the attester and contribution slices mint only on claims.

These numbers are one sample by one lane, and they are not a consensus fact. They are given so the
owner can set the limits in §6.

**The defect B-V must close.** Because the 888 reward is per block, the heartbeat setting is
already a monetary lever:

| Interval | Blocks/day | Ceiling WRF/day (22.22/block) |
|---|---|---|
| 30 s (heartbeat minimum) | 2,880 | 64,000 |
| 77 s (heartbeat default) | 1,122 | 24,935 |
| 600 s (genesis basis, heartbeat maximum) | 144 | 3,200 |

An operator edit of one environment variable changes the issuance ceiling 20-fold. A vote on block
speed under today's rules is therefore a vote on issuance. That is exactly what decision 3 forbids.

## 2. Design

### 2.1 Consensus: a recorded tier, and rewards by elapsed chain time

**(a) State-recorded tier history.** A new state field `block_speed_tiers: Vec<(activation_height,
interval_seconds)>` (root material). On 888 it is seeded at activation with `(H, 600)`, the genesis
basis. Every later entry comes only from an approved `BlockSpeedStep` (§2.2). The tier at a height is
the last entry at or below it. `compute_target_block_time` and `elapsed_seconds_at_height` read
this history instead of `BLOCK_TIME_PROGRESSION` from activation onward, so the timestamp drift
bound and the weather temporal allowance follow the voted tier.

**(b) Issuance-neutral reward.** The per-block emission becomes a **rate times elapsed chain
time**:

```
E(block) = rate_per_second × min(gap, tier_interval)
gap      = header.timestamp − parent.timestamp        (consensus header times, never a node clock)
rate     = flat_reward / 600         on 888 (22.22 WRF / 600 s = 0.037037 WRF/s)
         = adjusted_block_reward / block_time        on the default schedule (already per-second)
```

- **Daily issuance cannot exceed `rate × 86,400`, whatever the heartbeat does.** If the heartbeat
  runs faster than the tier, each block is paid its shorter gap. If it runs slower, each block is
  capped at one tier interval, so a stall never back-pays.
- **Timestamps bound the total.** The sum of gaps across a run is the last timestamp minus the
  first, which header monotonicity, median-time-past (PoP:2382) and the future-drift bound tie to
  real time. A producer that inflates one gap shortens the next.
- **A tier change does not change issuance.** It changes how a fixed per-second budget is cut into
  blocks. That is the "issuance-neutral" requirement of decision 3.
- On 888 at the current 77 s, this rule cuts the ceiling from 24,935 to 3,200 WRF/day, the
  genesis-recorded budget the testnet was designed for (`GENESIS_888.md`). That is a monetary change
  against current operations and needs explicit owner sign-off (§6 Q1).

**(c) Divisions stay exact.** `rate × gap` is computed in base units with multiply-before-divide
(`flat_reward × min(gap, tier) / 600`), in the same integer style as `adjusted_block_reward`. The
partition (`EmissionPartition::from_emission`) then splits `E(block)` across the roles exactly as
today.

### 2.2 Consensus: the vote

The general governance proposal surface is disabled at the consensus layer: `GovernanceProposal` is
refused at admission and in `verify_block` pending from-bound proposer authorization (#2024,
PoP `classify_block_transaction` §0a-ter). A vote therefore needs one of two carriers:

| Carrier | What it takes | Recommendation |
|---|---|---|
| **V1** A governance-signed record, the same shape as `ScheduleExtension` (signed by the extension authority, applied at a future height, refused by binaries that do not know it) | One new signed type `BlockSpeedStep { next_interval_seconds, activation_height, sequence }`, with validation rules below | **Now.** It reuses machinery already live on 888 |
| **V2** A token-holder proposal plus vote (the paper's §4.1 "community votes") | Re-enable `GovernanceProposal` with from-bound authorization (#2024), a new `ProposalType::BlockSpeedStep`, and the quorum and threshold rules | Later, once #2024 lands. Same validation rules |

Validation rules (consensus, both carriers):
1. `next_interval_seconds` is in the owner-set allowed set (Q2), and it moves at most one step from
   the current tier (Q3).
2. `activation_height ≥ approval_height + RUNUP` (Q4). That gives operators time to move the
   heartbeat, the same reason `ScheduleExtension` has `MIN_RUNUP_BLOCKS = 2000`
   (`governance/schedule_extension.rs:76`).
3. Dwell: the current tier has run at least `MIN_DWELL` of chain time (Q5). Cooldown: no other step
   is approved within `COOLDOWN` after one activates (Q6).
4. `sequence` is the next integer. A replay or an out-of-order step is refused.

### 2.3 Operations: the heartbeat follows

- The node exposes the current tier and the next approved step at a read-only route (for example,
  `GET /network/block-speed`).
- The heartbeat polls it with backoff (the node's 100 reads/min per-IP limiter applies) and switches
  its interval at the activation height.
- **None of this is consensus.** A heartbeat that does not follow cannot change issuance (§2.1b). It
  can only make blocks too fast (each paid less) or too slow (each capped). The reveal window,
  node-share epoch and extension run-up are counted in blocks, so their wall-clock length moves with
  the pace (Q7).

### 2.4 Feature gates (when code is written)

| Feature | Switches on | Root-affecting |
|---|---|---|
| `block_speed_tier_record` | the `block_speed_tiers` state and its seeding at activation; `compute_target_block_time` and `elapsed_seconds_at_height` read it | yes |
| `elapsed_time_emission` | `E(block) = rate × min(gap, tier)` | yes, and a monetary change on 888 (Q1) |
| `block_speed_step` | the signed `BlockSpeedStep` record (V1) and its validation | yes |

Each is appended to `ConsensusFeature` and `KNOWN_FEATURES`, with `schedulable()` returning false
until the owner approves. Each needs gate-off parity and producer / applier / WAL parity. A
determinism differential is required for `elapsed_time_emission`, because the emission reads header
timestamps.

**Why no code tonight.** The design is clean, but it touches emission, and this sprint's boundary
says no emission code until ratification (story-4 code was optional "if the design is clean").
`elapsed_time_emission` is also the change the owner must sign off on first (Q1).

## 3. Which part is consensus and which is operations

| Part | Consensus | Operations |
|---|---|---|
| The recorded tier history | ✓ | |
| The reward scaling `rate × min(gap, tier)` | ✓ | |
| Validating a `BlockSpeedStep` (allowed set, step size, run-up, dwell, cooldown, sequence) | ✓ | |
| Drift bound and weather temporal allowance following the tier | ✓ | |
| The heartbeat's interval | | ✓ |
| The route the heartbeat reads | | ✓ (read-only) |
| Collector reveal timing, reveal-window wall-clock length | | ✓ (block-counted rules are unchanged) |

## 4. Contradictions found while designing

1. 888's flat reward is "the 600 s tier" in genesis, but the heartbeat runs at 77 s, so the issuance
   ceiling is about 7.8 times the genesis budget. The tokenomics ADR's 1.168M WRF/yr figure assumes
   600 s.
2. `BLOCK_TIME_PROGRESSION` still drives `max_future_drift` and the weather temporal allowance on
   888, though 888 does not follow it. At the current heights the table says 2 h, so the drift bound
   is 1,800 s for 77 s blocks.
3. The paper's §4.1 "community votes" need the governance proposal surface that #2024 disables.

## 5. Risks

- `elapsed_time_emission` makes every role's per-block slice proportional to the gap. A role with a
  per-block fixed cost (a producer) earns less per block at faster tiers. That is the intended
  neutrality, but it should be checked against producer economics.
- Header timestamps become monetary inputs. Today they are already validated (monotonic, MTP,
  drift), and §2.1b caps each block at one tier interval.

## 6. Questions for the owner (limits are yours, per decision 2)

1. **Re-base 888 issuance to its genesis per-second budget?** With `elapsed_time_emission`, 888's
   ceiling falls from about 24,935 WRF/day (at 77 s) to 3,200 WRF/day. *Recommended: yes. That is
   the budget genesis recorded. Today's higher figure is an artefact of the heartbeat.*
2. **Allowed intervals:** *Recommended: a ladder inside the heartbeat's supported range, for
   example 600 → 300 → 150 → 77 → 60 → 30 s. The paper's 2 h → 1 s ladder needs a different
   producer design below 30 s.*
3. **Step size:** one rung per vote? *Recommended: yes.*
4. **Run-up:** *Recommended: 2,000 blocks, the same as `MIN_RUNUP_BLOCKS`.*
5. **Minimum dwell:** the paper says one week. *Recommended: one week of chain time.*
6. **Cooldown:** the paper says 72 h. *Recommended: 72 h of chain time.*
7. **Maximum dwell with an automatic step (the paper's "maximum of a year"):** *Recommended: no
   automatic step on the testnet. Revisit at mainnet.*
8. **Carrier:** V1 (governance-signed record) now, V2 (token-holder vote) after #2024? *Recommended:
   yes.*
9. **Block-counted windows** (3-block reveal window, 512-block epoch, 2,000-block run-up): keep them
   counted in blocks, or convert them to chain time? *Recommended: keep them in blocks for now, and
   check reveal-window slack at each rung before approving it.*
