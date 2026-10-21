# Kani Bounded Proofs — Economic Arithmetic

Part of the determinism & Rust-assurance epic (#2102, story #2108). Kani bounded model-checks
the small, high-value arithmetic that decides quorum, fees and burn. The harnesses live in
`consensus-engines/src/economics/proofs.rs` behind `#[cfg(kani)]` and run in
`.github/workflows/kani.yml`, never on the default `cargo test` path.

Every proof is mirrored by a runnable `#[cfg(test)]` equivalent (exhaustive where the domain is
small, bounded sampling otherwise), so the property is checkable without a Kani solver:
`cargo test -p wrfcoin-consensus-engines economics::proofs::tests`.

## Plan reconciliation

The story named `reward_split`, `apply_slash` and `fee_charge`. **None of those functions exist**
in `consensus-engines`. The proofs are stated against the real economic surfaces:

| Story property | Real target | Status |
|---|---|---|
| Quorum threshold > 2/3 for n=1..=8 | `shared_protocol::common::constants::compute_oracle_quorum` | **Proven** (`proof_quorum_strict_supermajority`) |
| (added) quorum monotonic | same | **Proven** (`proof_quorum_monotonic`) |
| (added) quorum boundary exact — at accepts, one-below rejects | same | **Proven** (`proof_quorum_boundary_is_exact`) |
| (added) empty set never reaches quorum | same | **Proven** (`proof_empty_set_never_reaches_quorum`) |
| `apply_slash` never raises supply — penalty <= stake | `validator::penalty_amount_bps` (u64 base units, bps/10_000) | **Proven** (`proof_slash_never_exceeds_stake`), full u64 domain |
| (added) bps u128 -> u64 narrowing is lossless | same | **NOT a Kani proof** — withdrawn; exceeded 439s unbounded and 900s bounded to `MAX_SUPPLY`. Sampled test retained |
| (added) penalty monotonic in stake | same | **NOT a Kani proof** — withdrawn; exceeded 300s unbounded and 900s bounded to `MAX_SUPPLY`. Sampled test retained |
| (added) effective fee never exceeds the cap | `FeeMarket::calculate_effective_fee` (EIP-1559 gas price, *not* base-unit/bps) | **Proven** (`proof_effective_fee_capped`) |
| `fee_charge` monotonic, no overflow | `FeeMarket::calculate_fee_burn` (`saturating_mul`) | **NOT a Kani proof** — withdrawn, see below. Sampled test retained |
| `reward_split` never mints > budget | *no such fn in this crate* | **NOT PROVEN** — see below |

### What this does not prove, stated plainly

An earlier revision of this document said the reward and slash properties were "already enforced at
runtime by the named invariants (which the differential-invariant harness #2100 runs after each
applied block)." **Two things were wrong with that.**

First, a runtime invariant predicate is not a bounded proof, and #2108 asks for proofs. Slash
**non-inflation** is now genuinely proven above, against the production `penalty_amount_bps`, over
the whole `u64` stake domain — but only that one property. Two further slash properties are
withdrawn as intractable and are claimed as sampled tests; see below.

Second, **the #2100 wiring claim was false**: #2100's implementation is PR #2260, which is still
open and is not on this branch. The document asserted a mechanism that does not exist yet — the
same defect class this epic exists to find.

**Reward budget remains unproven, and here is why rather than a promise.** There is no reward or
emission arithmetic in `consensus-engines` at all. `MAX_BLOCK_EMISSION` and the `EmissionCap`
predicate live in `shared-protocol/src/invariants/rewards.rs`, and this workflow is path-filtered to
`consensus-engines/**`. Proving it needs a harness in `shared-protocol` plus a workflow change; that
is follow-up work, not something this branch delivers.

### Why fee-burn is no longer a Kani harness

`calculate_fee_burn` is `base_fee.saturating_mul(gas_used)` — a one-line wrapper over the standard
library. Its monotonicity harness quantified three unbounded `u64`s across **two** symbolic 64x64
multiplications, which CBMC bit-blasts into two ~4096-partial-product multipliers plus 128-bit
overflow detection. In live run `29875816548` that one harness consumed the rest of a 45-minute job
and was cancelled; the quorum proofs were never reached.

The diagnosis is the contrast in that same run: `proof_effective_fee_capped` quantifies the *same*
unbounded `u64` domain and **completed**, because `calculate_effective_fee` is comparison,
subtraction, `min` and addition — all linear. **The cost is nonlinear multiplication, not loose
bounds.** Narrowing the range would only help by shrinking effective bit-width, and a proof over an
artificially narrow width proves nothing about production.

So the harness is withdrawn rather than accelerated: what it proved is a property of
`u64::saturating_mul`, not of this codebase. The bps arithmetic proven above is the fee-shaped
arithmetic #2108 actually specified — and it *is* tractable over the full domain, because the u128
product is divided by a **constant** denominator rather than a symbolic operand.

### Why two slash properties are no longer Kani harnesses

Slash **non-inflation** is proven above over the full `u64` domain, in 10s. Two *further* slash
properties are not, and the difference is the number of symbolic chains rather than the bound.

`penalty_amount_bps` alone is one `u128` product over a **constant** divisor — tractable.
`slash_bps_narrowing_is_lossless` compares the real result against its widest-rate `u128`
expansion, and `slash_monotonic_in_stake` compares two stakes; each therefore evaluates that
expression **twice** and CBMC must relate two symbolic 128-bit multiply/divide chains. Measured
**>439s** and **>300s** respectively, against **10s** for the single-chain proof — the same shape
that made fee-burn intractable.

#### Correction: the bound argument this document used to make was wrong

An earlier revision justified the withdrawal by claiming that *"at `WRF_UNITS_PER_COIN = 10^8` a
realistic balance needs the full `u64`, so a bound small enough to be tractable would exclude every
real stake."* **That is false.** It rested on an arithmetic error in `consensus-engines/src/validator.rs`
(since corrected): `u64::MAX / 10^8` is ~184.47 **billion** WRF, not the ~18.4M WRF that comment
claimed — wrong by four orders of magnitude.

A representative bound does exist. Total WRF supply is `21_000_000 * 10^8 = 2_100_000_000_000_000`
base units (`shared_protocol::consensus::proof_of_publishing::MAX_SUPPLY`), which fits in **51 bits**.
`stake <= MAX_SUPPLY` excludes no protocol-valid stake — only the 13 high `u64` bits the protocol can
never reach. Credit to the #2253 review for catching this.

#### The bound was applied and measured, and it does not help

Both harnesses were re-run under `kani::assume(stake as u128 <= MAX_SUPPLY)`:

| Harness | Unbounded (full `u64`) | Bounded to `MAX_SUPPLY` (51 bits) |
|---|---|---|
| `slash_bps_narrowing_is_lossless` | exceeded 439s | **exceeded 900s** |
| `slash_monotonic_in_stake` | exceeded 300s | **exceeded 900s** |
| *(`slash_never_exceeds_stake`, one chain, for contrast)* | **~10s** | — |

None of these is a completion time — all are ceilings the runs hit, so the unbounded and bounded
columns are not directly comparable. The finding is that neither harness completes, bounded or not.

The likely reason is structural, and it is why no bound on `stake` can help: the cost lies in the
`u128` divide-by-constant circuit, whose width CBMC fixes from the `u128` **type** rather than from
the range of `stake`. Constraining `stake` to 51 bits narrows the symbolic input while leaving both
128-bit dividers at full width.

Both are therefore claimed as **sampled tests, not proofs** —
`slash_never_exceeds_stake_sampled` and `slash_monotonic_in_stake_sampled`, which exercise
`u64::MAX`, the protocol cap `MAX_SUPPLY`, the rate/denominator boundaries, and zero. The withdrawal
stands; only its justification changed, from a false claim about the WRF domain to a measured one
about solver cost.

#### The sampled narrowing predicate must actually check losslessness

Sampled coverage is only worth anything if it can fail. The narrowing predicate originally checked a
one-sided bound — `production <= stake * MAX_PENALTY_BPS / 10_000` — against the *widest* rate,
ignoring the severity. The #2253 review planted the strongest negative control, replacing production
`penalty_amount_bps` with a constant `0`, and **all seven sampled tests still passed**, including the
one named here as narrowing coverage. A sampled test that a maximally-wrong implementation walks
through is not coverage.

It now recomputes the exact per-severity quotient through an independent `match` and expression and
asserts **equality**: `penalty_amount_bps(stake, sev) as u128 == stake * bps(sev) / 10_000`, plus
`u64`-representability. The zero mutation now turns `slash_never_exceeds_stake_sampled` red
(`production 0 != exact 1 at stake=100 Minor`), verified locally. The precondition that makes the
narrowing safe — `penalty_bps <= MAX_PENALTY_BPS < BPS_DENOMINATOR` — is now a compile-time
assertion in `validator.rs` and a `debug_assert!` at the cast site, rather than only prose.

## Stubs and bounds

For a bounded proof to mean anything, what it assumes must be explicit. The complete list:

| Harness | `kani::assume` / bound | Why |
|---|---|---|
| `proof_quorum_strict_supermajority` | `1 <= n <= 8` | Matches the TLA+ `QuorumThresholdSound` model config; the property is a per-`n` fact, so a bounded range fully covers the intended validator counts. |
| `proof_quorum_monotonic` | `1 <= n <= 8` | Monotonicity is checked stepwise (`q(n) >= q(n-1)`); the bound keeps the proof cheap. The `#[cfg(test)]` mirror exhausts `1..=64` for extra confidence. |
| `proof_quorum_boundary_is_exact` | `1 <= n <= 8`, `votes <= 9` | The accept/reject boundary sits at `votes ∈ {quorum(n)-1, quorum(n)}`; quorum is monotonic and peaks at `quorum(8)=6` over the range, so every boundary is `<= MAX_QUORUM_N` and `votes <= 9` also covers the one-above-set case. Note `quorum(n) <= n` does **not** hold — quorum floors at `DEFAULT_ORACLE_QUORUM=2`, so `quorum(1)=2 > 1` (#811); the harness proves accept at `votes=2` / reject at `votes=1` for `n=1`. |
| `proof_empty_set_never_reaches_quorum` | none — no inputs | A closed fact about `compute_oracle_quorum(0)`. |
| `proof_slash_never_exceeds_stake` | none — full `u64` `stake`, all three severities | The base-unit/bps property #2108 specified. Tractable unbounded because the `u128` product is divided by a **constant** denominator, not a symbolic operand. |
| `proof_effective_fee_capped` | none — full `u64` range | Establishes the cap and the base-fee-refusal branch over the whole input space. |

`proof_fee_burn_monotonic`, `proof_slash_bps_narrowing_is_lossless` and
`proof_slash_monotonic_in_stake` are **not in this table because they are not harnesses** — all
three are withdrawn as intractable and covered by sampled tests instead. See the two sections above.

No harness stubs out a function body; all call the real functions. There are no `kani::assume`s
beyond the ranges above.

## Enforcement (honest, per #2233)

| Aspect | Value |
|---|---|
| Workflow | `.github/workflows/kani.yml` |
| Trigger | Schedule (weekly) + `workflow_dispatch` + PRs touching `consensus-engines/**` |
| On default `cargo test`? | **No** — harnesses are `#[cfg(kani)]`; only the mirror tests run there |
| Per-harness budget | 120s, enforced by a **process-group kill**; exit 124 is reported as a tractability failure, never as a counterexample |

**The budget mechanism was itself defective and is now fixed.** It previously used
`timeout 120 cargo kani …`, which does not bound the harness: `timeout` signals only `cargo-kani`,
while the `cbmc` grandchild doing the work ignores it and keeps running. Observed directly while
measuring the withdrawn harnesses — `timeout 120` returned 124 at a wall time of **128s**, and that
harness's `cbmc` was still consuming CPU **~56s after the timeout had "fired"**. Orphaned solvers
then compete with every later harness for the same 45-minute job, which is precisely the runaway the
per-harness split exists to prevent. The budget had never been enforced; it had simply never needed
to fire, because all six harnesses finish in ~10s — a gate that has never been seen firing has never
been seen working. Each harness now runs in its own process group (`setsid`) and the **group** is
killed on expiry.

The replacement was then proved capable of firing, rather than assumed. A known-intractable harness
was planted temporarily (the withdrawn narrowing property) and run under a 45s budget:

```text
run_harness returned 124 at wall 46s   (budget 45s — 1s overshoot, vs 8s for `timeout`)
orphaned cbmc after 3s:        0       (vs surviving ~56s under `timeout`)
orphaned cargo-kani after 3s:  0
```

The probe was removed and the tree verified clean afterwards.

| Locally verified | **Yes — `cargo kani` 0.67.0 was run locally on all six harnesses, all passing.** Per-harness 7–11s; whole set 49s and 52s across two runs. The `#[cfg(test)]` mirrors also pass (7 tests) |

An earlier revision of this table said `cargo kani` had not been run locally and that the budgets
were estimates. Both are now measurements — the numbers above and in `kani.yml` come from running
this exact harness list, not from inference about the workflow's shape.

The runnable mirrors are the local evidence; the Kani job is the unbounded proof. Any property
Kani cannot discharge must be recorded here rather than weakened.
