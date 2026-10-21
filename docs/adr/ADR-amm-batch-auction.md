# ADR: defer an AMM batch auction pending a canonical application design (#2202)

## Decision

Do not build a batch auction now. It is preferable to protocol-level ordering
work in principle, but this repository has no single canonical AMM interface
to apply it to: `IWrfAMM` and `ICarbonCreditAMM` use incompatible signatures.

## Design if justified

A bounded-height application batch would collect orders, compute one uniform
clearing price, refund unfilled orders, and make clearing invariant under an
order permutation. It must specify partial-fill rounding, expiry, cancellation,
assets, fees and settlement failure atomically. Fee policy must retain the
existing `FEE_DENOMINATOR_BPS = 10_000` representation and explicitly map the
30 bps default plus 1/5/30/100 bps tiers; otherwise a "fair" auction can change
economic outcomes through rounding or tier selection.

## Cost and timing

Choosing a canonical AMM API, order representation and settlement transaction
is a product/API migration. If settlement is included in blocks it also needs
deterministic state-transition and replay coverage. Build only after measured
AMM volume shows extraction and an owner selects one interface. This does not
require a new fork-choice rule, but it is not a free drop-in module.

## What this does not solve

It does not hide orders before batch close, prevent cross-batch attacks, cure
general mempool censorship, or make incompatible AMMs interoperable.
