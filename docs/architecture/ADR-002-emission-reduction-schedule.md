# ADR-002: Deterministic wall-clock emission reductions

**Status**: Accepted  
**Date**: 2026-07-29  
**Decision owner**: Core4 genesis cut

## Context

The original Bitcoin-style reward schedule halved a block-denominated reward
every 210,000 blocks.  WRFcoin changes block time as the chain progresses, so
a fixed number of blocks no longer represents a fixed duration.  Reductions
would consequently arrive much faster in wall-clock time at later block-time
tiers.

The genesis-cut decision recorded in #2480 is to start below Bitcoin's reward,
reduce the reward **by** one third (retain two thirds), and preserve the
21,000,000 WRF supply cap.  Reward calculation is consensus code; ambiguity in
units, time source, or rounding would make independently replayed state roots
diverge.

## Decision

The genesis schedule is the one implemented by merged #2497:

- `INITIAL_BLOCK_REWARD = 800 WRF` in base units.
- An emission period is `126,000,000` elapsed schedule seconds.
- At each completed period, retain `2/3` of the previous reward.
- Elapsed time is derived solely from the compile-time block-time progression,
  never from producer-supplied timestamps.
- Reduction uses iterative integer `floor(reward * 2 / 3)` arithmetic.  Once it
  reaches zero, it remains zero.

At the genesis block time, one period emits 7,000,000 WRF.  The geometric
series `7,000,000 * (1 + 2/3 + ...)` is 21,000,000 WRF before integer-floor
losses, so realised issuance cannot exceed `MAX_SUPPLY`.

## Consequences

- The schedule is deterministic across replay and remains tied to wall time as
  block production accelerates.
- The 21M cap and `MAX_BLOCK_EMISSION` remain unchanged.
- Floors make actual terminal emission slightly below the analytic series; this
  is intentional and must remain covered by the supply-bound test.
- Any future change to reward, period, ratio, time source, or rounding is a
  new genesis-cut consensus decision, not a tuning change.

## Alternatives rejected

- Block-count reductions: invalid because changing block duration changes their
  wall-clock cadence.
- Floating-point exponentiation: invalid because consensus arithmetic must be
  bit-identical.
- A one-third retained reward: contradicts the owner decision to reduce *by*
  one third and would require a faster starting schedule to approach 21M.

## Evidence and follow-up

`shared-protocol/src/consensus/proof_of_publishing.rs` is the normative
implementation, including the period, integer-floor rule, terminal behavior,
and supply identity test.  This ADR records that already-merged policy; it
does not change consensus behavior.

Related: #2480, merged #2497, #2492, #2485.
