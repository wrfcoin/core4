//! Criterion wall-clock benchmarks over the story #2177 measurement matrix.
//!
//! This is the *timing* half of the benchmark suite. It sweeps every circuit
//! family across its input-size knob so the growth curve is visible, and it
//! separates the three phases that have completely different operational
//! characters:
//!
//! - `zk_setup`  — one-time, minutes, gigabytes. Sampled minimally; the point
//!   is the order of magnitude, not a tight confidence interval.
//! - `zk_prove`  — per proof, prover-side, off the consensus path.
//! - `zk_verify` — per proof, per node, **on** the consensus path. This is the
//!   one with an enforced budget in `zk-circuits/src/budgets.rs`.
//!
//! Memory, key sizes and proof sizes are *not* measured here — criterion
//! cannot measure them meaningfully. They come from
//! `src/bin/zk-proving-report.rs`, which isolates each phase in its own
//! process so peak RSS is attributable. Together the two produce
//! `docs/zk/PROVING_COSTS.md`.
//!
//! ```text
//! cargo bench -p wrfcoin-zk-circuits --bench proving
//! cargo bench -p wrfcoin-zk-circuits --bench proving -- zk_verify
//! ```
//!
//! The existing `benches/proof_bench.rs` is left untouched: it covers batch
//! verification and hand-picked fixtures that are not part of this matrix.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::time::Duration;
use wrfcoin_zk_circuits::bench_matrix::{CircuitKind, MATRIX};
use wrfcoin_zk_circuits::proving;

/// Setup is measured on a reduced matrix: one small and one large case per
/// family. Benchmarking a multi-second trusted setup 15 ways is hours of
/// wall-clock for information the report harness already records exactly once.
const SETUP_CASES: &[CircuitKind] = &[
    CircuitKind::RangeProof { n_bits: 8 },
    CircuitKind::RangeProof { n_bits: 64 },
    CircuitKind::ConfidentialTransfer {
        num_inputs: 1,
        num_outputs: 1,
        n_bits: 64,
    },
    CircuitKind::SelectiveDisclosure { num_fields: 1 },
    CircuitKind::SelectiveDisclosure { num_fields: 64 },
    CircuitKind::WeatherModel { num_samples: 1 },
    CircuitKind::WeatherModel { num_samples: 128 },
];

/// Fixed, circuit-independent overhead paid inside `generate_constraints`.
///
/// Every circuit in this crate calls `poseidon_config_bn254()`, which derives
/// the round constants and MDS matrix from the Poseidon Grain LFSR *on every
/// call*. That cost is paid on every setup, every proof and every constraint
/// synthesis, and it does not shrink with circuit size — so at the constraint
/// counts these circuits actually run at, it can dominate the SNARK work it is
/// wrapped in. Benched separately so the fixed and variable halves of proving
/// cost can be told apart in `docs/zk/PROVING_COSTS.md`.
fn bench_fixed_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("zk_fixed_overhead");
    group.bench_function("poseidon_config_bn254", |b| {
        b.iter(|| {
            black_box(wrfcoin_zk_circuits::circuits::poseidon_config::poseidon_config_bn254())
        });
    });
    group.finish();
}

fn bench_setup(c: &mut Criterion) {
    let mut group = c.benchmark_group("zk_setup");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));
    for kind in SETUP_CASES {
        group.bench_with_input(BenchmarkId::from_parameter(kind.id()), kind, |b, kind| {
            b.iter(|| black_box(kind.generate_keys().expect("setup")));
        });
    }
    group.finish();
}

fn bench_prove(c: &mut Criterion) {
    let mut group = c.benchmark_group("zk_prove");
    group.sample_size(20);
    for kind in MATRIX {
        // Keys are generated *outside* the timed loop, which is also how the
        // system must behave in production: never generate a proving key at
        // request time (#1833, #2038).
        let (pk, _) = kind.generate_keys().expect("setup");
        group.bench_with_input(BenchmarkId::from_parameter(kind.id()), kind, |b, kind| {
            b.iter(|| black_box(kind.prove(&pk).expect("prove")));
        });
    }
    group.finish();
}

fn bench_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("zk_verify");
    for kind in MATRIX {
        let (pk, pvk) = kind.generate_keys().expect("setup");
        let proof = kind.prove(&pk).expect("prove");
        let inputs = kind.public_inputs();
        assert!(
            proving::verify(&proof, &pvk, &inputs).expect("verify"),
            "{}: fixture must verify before benchmarking",
            kind.id()
        );
        group.bench_with_input(BenchmarkId::from_parameter(kind.id()), kind, |b, _| {
            b.iter(|| black_box(proving::verify(&proof, &pvk, &inputs).expect("verify")));
        });
    }
    group.finish();
}

criterion_group!(
    proving_benches,
    bench_fixed_overhead,
    bench_setup,
    bench_prove,
    bench_verify
);
criterion_main!(proving_benches);
