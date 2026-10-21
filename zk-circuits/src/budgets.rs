//! Enforced cost budgets for the consensus path (story #2177).
//!
//! Proof **verification** and proof **size** are paid by every node that
//! validates a block, forever. They are therefore enforced here as tests.
//!
//! Proof **generation** cost is paid once, by one party, off the critical
//! path. It is measured and recorded in `docs/zk/PROVING_COSTS.md` but is
//! deliberately *not* asserted: a generation regression is a product problem,
//! not a consensus problem, and a flaky wall-clock assertion on a 16-second
//! operation would be worse than no assertion at all.
//!
//! ## Why these numbers
//!
//! See `docs/zk/PROVING_COSTS.md` for the measured baselines. The budgets are
//! set with deliberate headroom over the slowest measured hardware profile so
//! that they fail on an *architectural* regression (an extra pairing, an
//! uncompressed proof encoding, a public-input blowup) rather than on a noisy
//! CI runner.

/// Serialized size of a Groth16 proof over BN254 in compressed form:
/// `A` (G1, 32 B) + `B` (G2, 64 B) + `C` (G1, 32 B).
///
/// This is a property of the proof system, not of the circuit — it does not
/// grow with constraint count, witness size, or public-input count.
pub const GROTH16_BN254_COMPRESSED_PROOF_BYTES: usize = 128;

/// Hard ceiling on `ZkProof::proof_bytes`.
///
/// Exceeding this means the encoding changed (e.g. to uncompressed points),
/// which is a consensus-visible format change and must be a deliberate,
/// reviewed decision — not a silent regression.
pub const MAX_PROOF_BYTES: usize = 192;

/// Hard ceiling on single-proof verification, in milliseconds, measured as the
/// best of several runs on one core.
///
/// Verification is ~1.5 ms on the reference workstation and ~2.6 ms on the
/// single-core-constrained profile (see `docs/zk/PROVING_COSTS.md`). The
/// budget is set an order of magnitude above that: it exists to catch a
/// structural change, and must survive a contended CI runner.
pub const MAX_VERIFY_MILLIS: u64 = 25;

/// Number of verification runs used to establish the budget statistic.
///
/// The assertion uses the **minimum** of these runs, not the mean. Minimum is
/// the right statistic for a "can this complete within budget" claim: it is
/// the machine's actual capability, and it is immune to scheduler preemption,
/// which only ever makes samples slower.
pub const VERIFY_BUDGET_SAMPLES: usize = 20;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench_matrix::CircuitKind;
    use std::time::Instant;

    /// A small, fast subset of the measurement matrix.
    ///
    /// Budget properties are circuit-independent for proof size and vary only
    /// with public-input count for verification, so covering one case per
    /// family at its cheapest input size is sufficient — and keeps the
    /// targeted `--lib` run fast. The full sweep lives in
    /// `src/bin/zk-proving-report.rs`.
    fn budget_cases() -> Vec<CircuitKind> {
        vec![
            CircuitKind::RangeProof { n_bits: 8 },
            CircuitKind::ConfidentialTransfer {
                num_inputs: 1,
                num_outputs: 1,
                n_bits: 8,
            },
            CircuitKind::SelectiveDisclosure { num_fields: 1 },
            CircuitKind::WeatherModel { num_samples: 1 },
        ]
    }

    #[test]
    fn proof_size_within_budget() {
        for kind in budget_cases() {
            let (pk, _) = kind.generate_keys().expect("setup");
            let proof = kind.prove(&pk).expect("prove");
            assert!(
                proof.proof_bytes.len() <= MAX_PROOF_BYTES,
                "{}: proof is {} bytes, budget is {MAX_PROOF_BYTES}",
                kind.id(),
                proof.proof_bytes.len()
            );
        }
    }

    /// Stronger than the budget: the size is *exact* and identical across
    /// circuits. If this ever fails, the on-chain proof encoding changed.
    #[test]
    fn proof_size_is_constant_across_circuits() {
        for kind in budget_cases() {
            let (pk, _) = kind.generate_keys().expect("setup");
            let proof = kind.prove(&pk).expect("prove");
            assert_eq!(
                proof.proof_bytes.len(),
                GROTH16_BN254_COMPRESSED_PROOF_BYTES,
                "{}: proof size is not the compressed Groth16/BN254 size",
                kind.id()
            );
        }
    }

    #[test]
    fn verification_within_budget() {
        for kind in budget_cases() {
            let (pk, pvk) = kind.generate_keys().expect("setup");
            let proof = kind.prove(&pk).expect("prove");
            let inputs = kind.public_inputs();

            // Warm caches and branch predictors before timing.
            assert!(
                crate::proving::verify(&proof, &pvk, &inputs).expect("verify"),
                "{}: fixture proof must verify",
                kind.id()
            );

            let mut best = u128::MAX;
            for _ in 0..VERIFY_BUDGET_SAMPLES {
                let start = Instant::now();
                let ok = crate::proving::verify(&proof, &pvk, &inputs).expect("verify");
                let elapsed = start.elapsed().as_micros();
                assert!(ok, "{}: verification must succeed", kind.id());
                best = best.min(elapsed);
            }

            let budget_micros = u128::from(MAX_VERIFY_MILLIS) * 1_000;
            assert!(
                best <= budget_micros,
                "{}: fastest of {VERIFY_BUDGET_SAMPLES} verifications took {best} us, \
                 budget is {budget_micros} us",
                kind.id()
            );
        }
    }

    /// Verification is **not** constant-time in public-input count.
    ///
    /// `Groth16::prepare_inputs` runs an MSM over `vk.gamma_abc_g1`, whose
    /// length is `num_public_inputs + 1`. This test pins that structural fact
    /// so the cost model in `docs/zk/PROVING_COSTS.md` cannot drift silently.
    /// It is a structural assertion rather than a timing ratio on purpose —
    /// a timing-ratio test at this scale is noise.
    #[test]
    fn verification_cost_scales_with_public_inputs() {
        for kind in budget_cases() {
            let (pk, _) = kind.generate_keys().expect("setup");
            assert_eq!(
                pk.vk.gamma_abc_g1.len(),
                kind.num_public_inputs() + 1,
                "{}: gamma_abc_g1 length must be num_public_inputs + 1",
                kind.id()
            );
        }
    }
}
