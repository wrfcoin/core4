//! Bounded proofs over the economic arithmetic (#2102 story #2108).
//!
//! Kani model-checks the small, high-value arithmetic that decides quorum, fees and burn.
//! Each proof is mirrored by a `#[cfg(test)]` exhaustive/bounded test so the property is
//! **verifiable locally** (`cargo test -p wrfcoin-consensus-engines economics::proofs`) even
//! where a Kani solver is unavailable — the Kani harness proves it over the unbounded (but
//! ranged) input space, the test proves it over a concrete sample/exhaustion.
//!
//! ## Which of the story's five properties are proved here — and which are not
//!
//! #2108 names reward-budget, fee, stake/slash and quorum properties. This module
//! does **not** prove all five, and the gap is recorded rather than implied:
//!
//! | Property | Status |
//! |---|---|
//! | **quorum threshold** | **PROVED** over `1..=8` — strict supermajority, monotonicity, and the exactly-at / one-below boundary |
//! | **slash non-inflation** (base-unit/bps) | **PROVED** over the full `u64` stake domain against production `penalty_amount_bps` |
//! | **effective fee cap** | **PROVED** over the full `u64` domain against production `FeeMarket::calculate_effective_fee`. Note this is the EIP-1559 gas-price cap, *not* base-unit/bps arithmetic — the base-unit/bps property the story specified is the slash non-inflation row above |
//! | **slash bps narrowing is lossless** | **NOT a Kani harness** — two `u128` chains; exceeded 439s unbounded and 900s bounded to `MAX_SUPPLY`. See "Why two slash properties are not Kani proofs". Retained as a sampled test |
//! | **slash monotonic in stake** | **NOT a Kani harness** — two `u128` chains; exceeded 300s unbounded and 900s bounded to `MAX_SUPPLY`. Same section. Retained as a sampled test |
//! | **fee burn monotonicity** | **NOT a Kani harness** — see "Why fee-burn is not a Kani proof" below. Retained as a sampled test |
//! | **reward budget / full distribution** | **NOT PROVED HERE.** No reward or emission arithmetic exists in `consensus-engines`; `MAX_BLOCK_EMISSION` and the `EmissionCap` predicate live in `shared-protocol/src/invariants/rewards.rs`, which this crate's Kani workflow does not cover. Proving it needs a harness in that crate and a workflow path-filter change — tracked, not done here |
//!
//! An earlier revision of this file claimed the reward and slash properties were
//! "discharged by the existing runtime invariants". That was an overstatement:
//! a runtime invariant predicate is not a bounded proof, and the invariant wiring
//! it referenced (#2100) is still open as PR #2260 and is not on this branch.
//!
//! ## Why fee-burn is not a Kani proof
//!
//! `calculate_fee_burn` is `base_fee.saturating_mul(gas_used)` — a one-line wrapper
//! over the standard library. The monotonicity harness quantified three unbounded
//! `u64`s across **two** symbolic 64x64 multiplications, which CBMC bit-blasts into
//! two ~4096-partial-product multiplier circuits plus 128-bit overflow detection.
//! In live run `29875816548` that single harness consumed the remainder of a
//! 45-minute job and was cancelled, and the quorum proofs were never reached.
//!
//! The contrast in that same run is the diagnosis: `proof_effective_fee_capped`
//! quantifies the *same* unbounded `u64` domain and **completed**, because
//! `calculate_effective_fee` is comparison, subtraction, `min` and addition — all
//! linear. **The cost is nonlinear multiplication, not loose bounds.** Tightening
//! the range would only help by shrinking effective bit-width, and a proof over an
//! artificially narrow width is a proof about nothing.
//!
//! So the harness is withdrawn rather than made faster: what it proved is a
//! property of `u64::saturating_mul`, not of this codebase. The base-unit/bps
//! arithmetic below is the fee-shaped arithmetic the story actually specified,
//! and it is provable because the u128 intermediate is divided by a *constant*.
//!
//! ## Why two slash properties are not Kani proofs
//!
//! `slash_bps_narrowing_is_lossless` and `slash_monotonic_in_stake` are withdrawn
//! as harnesses. `penalty_amount_bps` alone is tractable — one `u128` product over
//! a *constant* divisor, proved in ~10s by `proof_slash_never_exceeds_stake`. But
//! both of these properties evaluate that expression **twice** (narrowing compares
//! the real result against its widest-rate expansion; monotonicity compares two
//! stakes), so each harness carries two `u128` multiply-then-divide chains that
//! CBMC must relate. Unbounded over the full `u64` they exceeded **439s** and
//! **300s**.
//!
//! ### A correction: the bound argument this file used to make was wrong
//!
//! An earlier revision justified the withdrawal by claiming that *"at
//! `WRF_UNITS_PER_COIN = 10^8` a realistic balance needs the full `u64`, so a
//! bound small enough to be tractable would exclude every real stake."*
//! **That is false**, and it rested on an arithmetic error in `validator.rs`
//! (since corrected): `u64::MAX / 10^8` is ~184.47 **billion** WRF, not the
//! ~18.4M WRF that comment claimed — off by four orders of magnitude.
//!
//! A representative bound does exist. Total WRF supply is
//! `21_000_000 * 10^8 = 2_100_000_000_000_000` base units
//! (`shared_protocol::consensus::proof_of_publishing::MAX_SUPPLY`), which fits in
//! **51 bits**. `stake <= MAX_SUPPLY` therefore excludes no protocol-valid stake
//! at all — it excludes only the 13 high `u64` bits the protocol can never reach.
//!
//! ### So the bound was applied and measured, and it does not help
//!
//! Both harnesses were re-run with `kani::assume(stake as u128 <= MAX_SUPPLY)`.
//! **Each exceeded a 900s ceiling without completing** — against ~10s for the
//! single-chain proof.
//!
//! None of these figures is a completion time; all are ceilings the runs hit, so
//! the unbounded and bounded numbers are not directly comparable. The finding is
//! simply that neither harness completes, bounded or not.
//!
//! The likely reason is structural, and it is why the bound cannot help: the cost
//! is in the `u128` divide-by-constant circuit, whose width CBMC fixes from the
//! `u128` *type*, not from the range of `stake`. Constraining `stake` to 51 bits
//! narrows the symbolic input while leaving both 128-bit dividers at full width.
//!
//! Both properties are therefore claimed as **sampled tests, not proofs** —
//! `slash_never_exceeds_stake_sampled` and `slash_monotonic_in_stake_sampled`
//! below, which exercise `u64::MAX` and the boundary stakes concretely. The
//! withdrawal stands; only its justification changed, from a false claim about
//! the WRF domain to a measured one about solver cost.

#![allow(dead_code)]

use wrfcoin_shared_protocol::common::constants::compute_oracle_quorum;

use crate::economics::eip1559::FeeMarket;
use crate::validator::SlashingSeverity;
use crate::validator::{
    BPS_DENOMINATOR, PENALTY_BPS_CRITICAL, PENALTY_BPS_MAJOR, PENALTY_BPS_MINOR,
    penalty_amount_bps,
};

/// Upper bound on validator/oracle count for the bounded quorum proof — the story asks for
/// 1..=8, matching the TLA+ `QuorumThresholdSound` model config.
const MAX_QUORUM_N: usize = 8;

/// Property: for every set size `n` in `1..=8`, the oracle quorum is a *strict* supermajority
/// — `quorum(n) * 3 > n * 2` — so no `<= 2/3` coalition can ever reach quorum.
fn quorum_is_strict_supermajority(n: usize) -> bool {
    let q = compute_oracle_quorum(n);
    // Strict 2/3: q > 2n/3  <=>  3q > 2n (integer-safe form, no division).
    q.saturating_mul(3) > n.saturating_mul(2)
}

/// Property: quorum is monotonically non-decreasing in the set size.
fn quorum_is_monotonic(n: usize) -> bool {
    n == 0 || compute_oracle_quorum(n) >= compute_oracle_quorum(n - 1)
}

/// Property: fee burn (`base_fee * gas_used`, saturating) never overflows and is monotonic in
/// `gas_used`. `saturating_mul` guarantees no panic/overflow by construction; we prove
/// monotonicity holds even at the saturation boundary.
fn fee_burn_monotonic(base_fee: u64, g1: u64, g2: u64) -> bool {
    if g1 > g2 {
        return true; // only assert for g1 <= g2
    }
    let m = FeeMarket::default();
    m.calculate_fee_burn(base_fee, g1) <= m.calculate_fee_burn(base_fee, g2)
}

/// Property: the effective gas price never exceeds the caller's `max_fee_per_gas`, and the
/// function refuses (returns `None`) when the cap is below the base fee.
fn effective_fee_is_capped(max_fee: u64, max_priority: u64, base_fee: u64) -> bool {
    let m = FeeMarket::default();
    match m.calculate_effective_fee(max_fee, max_priority, base_fee) {
        None => max_fee < base_fee,
        Some(fee) => fee.effective_gas_price <= max_fee && fee.base_fee == base_fee,
    }
}

/// Property: the quorum threshold is the *exact* boundary — a coalition of
/// `quorum(n)` reaches it and a coalition of `quorum(n) - 1` does not.
///
/// The earlier revision proved only the formula's shape. This exercises the
/// accept/reject decision itself, which is what a vote-counting call site does.
fn quorum_boundary_is_exact(n: usize, votes: usize) -> bool {
    let q = compute_oracle_quorum(n);
    let accepted = votes >= q;
    // Exactly-at accepts; one-below rejects. Nothing between them.
    if votes == q {
        accepted
    } else if votes + 1 == q {
        !accepted
    } else {
        true
    }
}

/// Property: an empty oracle set can never reach quorum with zero votes.
///
/// `compute_oracle_quorum(0)` floors at `DEFAULT_ORACLE_QUORUM`, so the empty
/// set must not be accepted by a zero-vote round.
fn empty_set_never_reaches_quorum() -> bool {
    compute_oracle_quorum(0) > 0
}

/// Property: **slashing cannot inflate.** A penalty never exceeds the stake it is
/// taken from, over the whole `u64` base-unit domain and every severity.
///
/// This is the base-unit/bps arithmetic the story specified: `stake * bps / 10_000`
/// with a `u128` intermediate. Provable where fee-burn was not, because the
/// division is by a *constant* denominator rather than a symbolic operand.
fn slash_never_exceeds_stake(stake: u64, severity: &SlashingSeverity) -> bool {
    penalty_amount_bps(stake, severity) <= stake
}

/// Property: the `as u64` narrowing preserves the EXACT per-severity quotient.
///
/// "Lossless" means production returns precisely `stake * bps(severity) / 10_000`, with
/// no truncation from the `u128 -> u64` cast. The predicate recomputes that value through
/// an independent path and asserts equality — see the body for why an upper bound was not
/// enough. This is the silent-truncation class the `u128` intermediate exists to prevent
/// (#1401).
fn slash_bps_narrowing_is_lossless(stake: u64, severity: &SlashingSeverity) -> bool {
    // Independent oracle: recompute the exact penalty for THIS severity as a full `u128`
    // (no narrowing), through a separate `match` and expression, then require production to
    // equal it exactly.
    //
    // The previous form checked only `production <= stake*MAX_PENALTY_BPS/DENOM` — a
    // one-sided bound against the WIDEST rate that ignored `severity` entirely. A truncated
    // result, or a constant `0`, satisfied it: the #2253 review planted `penalty_amount_bps
    // -> 0` and this predicate stayed green. Equality against the per-severity exact value
    // closes that: `0 == exact` is false for any non-zero `stake * bps`.
    //
    // "Lossless" means the `as u64` cast in production does not truncate the `u128`
    // quotient. So the property is: the exact quotient is representable in `u64`, AND
    // production returns precisely it.
    let bps: u128 = match severity {
        SlashingSeverity::Minor => PENALTY_BPS_MINOR as u128,
        SlashingSeverity::Major => PENALTY_BPS_MAJOR as u128,
        SlashingSeverity::Critical => PENALTY_BPS_CRITICAL as u128,
    };
    let exact = (stake as u128) * bps / (BPS_DENOMINATOR as u128);
    exact <= u64::MAX as u128 && penalty_amount_bps(stake, severity) as u128 == exact
}

/// Property: penalties are monotonic in stake — a larger stake never yields a
/// smaller penalty at the same severity.
fn slash_monotonic_in_stake(a: u64, b: u64, severity: &SlashingSeverity) -> bool {
    if a > b {
        return true;
    }
    penalty_amount_bps(a, severity) <= penalty_amount_bps(b, severity)
}

// ---------------------------------------------------------------------------
// Kani proof harnesses (CI-only; behind #[cfg(kani)] so they never touch the
// default `cargo test` path). See .github/workflows/kani.yml.
// ---------------------------------------------------------------------------
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    #[kani::proof]
    fn proof_quorum_strict_supermajority() {
        let n: usize = kani::any();
        kani::assume(n >= 1 && n <= MAX_QUORUM_N);
        assert!(quorum_is_strict_supermajority(n));
    }

    #[kani::proof]
    fn proof_quorum_monotonic() {
        let n: usize = kani::any();
        kani::assume(n >= 1 && n <= MAX_QUORUM_N);
        assert!(quorum_is_monotonic(n));
    }

    /// Boundary behaviour: exactly-at accepts, one-below rejects.
    ///
    /// `votes` is bounded to `0..=MAX_QUORUM_N + 1` so the one-above-set case is
    /// included. That bound is justified rather than convenient: the accept/reject
    /// boundary for each `n` sits at `votes ∈ {quorum(n) - 1, quorum(n)}`, and over
    /// the modelled range quorum is monotonic and peaks at `quorum(8) = 6`, so every
    /// boundary lies at `votes <= MAX_QUORUM_N`; the trailing `+ 1` additionally
    /// covers the "one vote above the full set" case (`votes = n + 1`).
    ///
    /// It is deliberately NOT claimed that `quorum(n) <= n`. That is false at the
    /// floor: `compute_oracle_quorum` maxes with `DEFAULT_ORACLE_QUORUM = 2`, so
    /// `quorum(1) = 2 > 1` (a single oracle cannot self-certify, #811). The bound
    /// holds because `quorum(n) <= MAX_QUORUM_N` for `n <= MAX_QUORUM_N`, not because
    /// quorum is bounded by `n`. The harness proves the true boundary at the real
    /// floor: for `n = 1` it checks accept at `votes = 2` and reject at `votes = 1`.
    #[kani::proof]
    fn proof_quorum_boundary_is_exact() {
        let n: usize = kani::any();
        let votes: usize = kani::any();
        kani::assume(n >= 1 && n <= MAX_QUORUM_N);
        kani::assume(votes <= MAX_QUORUM_N + 1);
        assert!(quorum_boundary_is_exact(n, votes));
    }

    #[kani::proof]
    fn proof_empty_set_never_reaches_quorum() {
        assert!(empty_set_never_reaches_quorum());
    }

    /// Slash non-inflation over the FULL u64 stake domain — no `assume` on stake.
    ///
    /// Tractable despite being unbounded because the `u128` product is divided by
    /// a constant, so CBMC never bit-blasts a symbolic-by-symbolic multiplier.
    #[kani::proof]
    fn proof_slash_never_exceeds_stake() {
        let stake: u64 = kani::any();
        let sev = any_severity();
        assert!(slash_never_exceeds_stake(stake, &sev));
    }

    // -----------------------------------------------------------------------
    // DELIBERATELY ABSENT: `slash_bps_narrowing_is_lossless` and
    // `slash_monotonic_in_stake` have no harness here. Both evaluate
    // `penalty_amount_bps` (or its u128 expansion) TWICE, so each carries two
    // u128 multiply/divide chains — the shape that also made the fee-burn
    // harness intractable. Unbounded they exceeded 439s and 300s, versus ~10s
    // for the single-chain `proof_slash_never_exceeds_stake` above.
    //
    // Bounding `stake` HAS been tried, and does not help. `stake <= MAX_SUPPLY`
    // (2_100_000_000_000_000 base units, 51 bits) excludes no protocol-valid
    // stake, so it is an honest bound — and with it, each harness still
    // exceeded a 900s ceiling. The cost is the u128 divide-by-constant circuit,
    // whose width comes from the u128 type rather than from `stake`'s range.
    //
    // An earlier revision claimed instead that "a realistic balance needs the
    // full u64, so any tractable bound excludes every real stake." That was
    // false — u64::MAX is ~184.47 BILLION WRF, not ~18.4M. Do not restore it.
    // Both keep their sampled mirror tests instead.
    // See the module docs, "Why two slash properties are not Kani proofs".
    // -----------------------------------------------------------------------

    /// The EIP-1559 effective gas-price cap. Tractable over the full unbounded
    /// `u64` domain because `calculate_effective_fee` is comparison,
    /// subtraction, `min` and addition — all linear, no symbolic multiply.
    #[kani::proof]
    fn proof_effective_fee_capped() {
        let max_fee: u64 = kani::any();
        let max_priority: u64 = kani::any();
        let base_fee: u64 = kani::any();
        assert!(effective_fee_is_capped(max_fee, max_priority, base_fee));
    }

    /// Nondeterministic severity: three variants, so the proofs above quantify
    /// over every penalty rate rather than a chosen one.
    fn any_severity() -> SlashingSeverity {
        match kani::any::<u8>() % 3 {
            0 => SlashingSeverity::Minor,
            1 => SlashingSeverity::Major,
            _ => SlashingSeverity::Critical,
        }
    }
}

// ---------------------------------------------------------------------------
// Locally-runnable equivalents: exhaustive where the domain is small, bounded
// sampling otherwise. These give real assurance without a Kani solver.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quorum_strict_supermajority_exhaustive_1_to_8() {
        for n in 1..=MAX_QUORUM_N {
            assert!(
                quorum_is_strict_supermajority(n),
                "quorum({n})={} is not a strict 2/3 supermajority",
                compute_oracle_quorum(n)
            );
        }
    }

    #[test]
    fn quorum_monotonic_exhaustive() {
        for n in 1..=64usize {
            assert!(quorum_is_monotonic(n), "quorum not monotonic at n={n}");
        }
    }

    #[test]
    fn fee_burn_never_overflows_and_is_monotonic() {
        let samples = [0u64, 1, 21_000, 1_000_000, u64::MAX / 2, u64::MAX];
        for &base in &samples {
            for &g1 in &samples {
                for &g2 in &samples {
                    // saturating_mul cannot panic; assert the monotonic property.
                    assert!(fee_burn_monotonic(base, g1.min(g2), g1.max(g2)));
                }
            }
        }
    }

    const SEVERITIES: [SlashingSeverity; 3] = [
        SlashingSeverity::Minor,
        SlashingSeverity::Major,
        SlashingSeverity::Critical,
    ];

    #[test]
    fn quorum_boundary_exact_exhaustive() {
        for n in 1..=MAX_QUORUM_N {
            for votes in 0..=MAX_QUORUM_N + 1 {
                assert!(
                    quorum_boundary_is_exact(n, votes),
                    "quorum boundary wrong at n={n} votes={votes} (quorum={})",
                    compute_oracle_quorum(n)
                );
            }
        }
        assert!(empty_set_never_reaches_quorum());
    }

    /// The bps rate for a severity, recomputed here as an oracle fully independent of
    /// `penalty_amount_bps` — so a wrong severity→rate mapping in production is caught by
    /// divergence rather than shared with the checker.
    fn oracle_bps(sev: &SlashingSeverity) -> u128 {
        match sev {
            SlashingSeverity::Minor => 100,   // 1.00%, the documented rate
            SlashingSeverity::Major => 500,   // 5.00%
            SlashingSeverity::Critical => 1_000, // 10.00%
        }
    }

    #[test]
    fn slash_never_exceeds_stake_sampled() {
        // The stake set spans: zero; the smallest non-zero; the Minor-rate integer
        // boundary (99→0, 100→1); the denominator boundary (9_999/10_000/10_001); one
        // whole coin; the protocol supply cap MAX_SUPPLY (51 bits); and the full generic
        // u64 edge. The boundaries are where a `u128→u64` truncation or off-by-one would
        // actually show, which a coarse `[0, big, u64::MAX]` set would step over.
        const MAX_SUPPLY_U64: u64 = 2_100_000_000_000_000; // 21_000_000 * 10^8
        let stakes = [
            0u64, 1, 99, 100, 101, 9_999, 10_000, 10_001, 100_000_000,
            MAX_SUPPLY_U64, u64::MAX / 2, u64::MAX,
        ];
        for sev in &SEVERITIES {
            for &stake in &stakes {
                let produced = penalty_amount_bps(stake, sev);
                assert!(
                    slash_never_exceeds_stake(stake, sev),
                    "penalty {produced} exceeded stake {stake} at {sev:?}",
                );
                // Losslessness = production equals the exact per-severity quotient. The
                // predicate asserts this internally; assert it directly here too, so the
                // property is legible at the call site and a constant/truncated production
                // result (the #2253 review's `-> 0` mutation) turns THIS test red.
                let exact = (stake as u128) * oracle_bps(sev) / 10_000;
                assert!(
                    exact <= u64::MAX as u128,
                    "exact penalty {exact} not representable in u64 at stake={stake} {sev:?}",
                );
                assert_eq!(
                    produced as u128, exact,
                    "narrowing was lossy: production {produced} != exact {exact} at \
                     stake={stake} {sev:?}",
                );
                assert!(
                    slash_bps_narrowing_is_lossless(stake, sev),
                    "bps narrowing lost precision at stake={stake} {sev:?}"
                );
            }
        }
    }

    #[test]
    fn slash_monotonic_in_stake_sampled() {
        let stakes = [0u64, 1, 10_000, 100_000_000, u64::MAX / 2, u64::MAX];
        for sev in &SEVERITIES {
            for &a in &stakes {
                for &b in &stakes {
                    assert!(slash_monotonic_in_stake(a.min(b), a.max(b), sev));
                }
            }
        }
    }

    #[test]
    fn effective_fee_capped_sampled() {
        let samples = [0u64, 1, 100, 1_000, u64::MAX / 2, u64::MAX];
        for &max_fee in &samples {
            for &max_prio in &samples {
                for &base in &samples {
                    assert!(effective_fee_is_capped(max_fee, max_prio, base));
                }
            }
        }
    }
}
