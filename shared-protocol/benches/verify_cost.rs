//! Calibration benchmark for the #2143 verification-cost table.
//!
//! `VerifyPricing::CALIBRATED` prices each verification primitive relative to
//! one ed25519 verify. Those ratios must come from measurement, not intuition —
//! underpricing the term an attacker controls linearly is the entire vector the
//! story exists to close. This benchmark is what produces them, committed so
//! recalibration on new hardware is repeatable rather than archaeological.
//!
//! Run:
//!
//! ```text
//! cargo bench -p wrfcoin-shared-protocol --bench verify_cost
//! ```
//!
//! Then read the ratios off the report and, if they have moved materially,
//! update `VerifyPricing::CALIBRATED` in
//! `shared-protocol/src/consensus/verify_cost.rs` in a reviewable diff that
//! cites the run. The absolute nanoseconds are hardware-specific and are NOT
//! what the table encodes; the ratio of each primitive to `ed25519_verify` is.
//!
//! `mlsag_verify` is the load-bearing one: it is measured across ring sizes
//! precisely to show the cost is linear in members, which is why
//! `per_ring_sig_member` prices members rather than signatures.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ed25519_dalek::{Signer, SigningKey, Verifier};
use wrfcoin_privacy::ring_signatures::{MLSAGSignature, RingMember};

const MESSAGE: &[u8] = b"wrfcoin verification cost calibration (#2143)";

/// The anchor: every other constant is expressed as a multiple of this.
fn bench_ed25519_verify(c: &mut Criterion) {
    let key = SigningKey::from_bytes(&[7u8; 32]);
    let verifying = key.verifying_key();
    let signature = key.sign(MESSAGE);

    c.bench_function("ed25519_verify", |b| {
        b.iter(|| {
            let ok = verifying.verify(MESSAGE, &signature).is_ok();
            assert!(ok, "the benchmark must measure a SUCCEEDING verify");
            ok
        })
    });
}

/// MLSAG verification across ring sizes.
///
/// The per-iteration cost should be linear in ring size; `Throughput::Elements`
/// makes criterion report per-member cost directly, which is the number
/// `per_ring_sig_member` encodes.
fn bench_mlsag_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("mlsag_verify");
    for ring_size in [2usize, 8, 64, 256, 1024] {
        // `sign` inserts the signer, so pass ring_size - 1 decoys.
        let decoys: Vec<RingMember> = (0..ring_size - 1)
            .map(|i| {
                let secret = [(i % 251) as u8 + 1; 32];
                let key = curve25519_public(&secret);
                RingMember { public_key: key }
            })
            .collect();
        let private_key = [9u8; 32];
        let signature = match MLSAGSignature::sign(MESSAGE, &private_key, &decoys) {
            Ok(signature) => signature,
            // A ring this build refuses to sign is not a calibration point.
            Err(_) => continue,
        };
        assert!(
            signature.verify(MESSAGE).unwrap_or(false),
            "ring {ring_size}: the benchmark must measure a SUCCEEDING verify"
        );

        group.throughput(Throughput::Elements(ring_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(ring_size),
            &signature,
            |b, signature| b.iter(|| signature.verify(MESSAGE).unwrap_or(false)),
        );
    }
    group.finish();
}

/// Compressed Ristretto public key for a scalar, via the privacy crate's own
/// curve so the decoys are points its verifier will accept.
fn curve25519_public(secret: &[u8; 32]) -> [u8; 32] {
    use curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;
    use curve25519_dalek::scalar::Scalar;
    (&Scalar::from_bytes_mod_order(*secret) * RISTRETTO_BASEPOINT_TABLE)
        .compress()
        .to_bytes()
}

/// Bulletproof-style range proof verification, as `RewardClaim` and
/// `ConfidentialTransfer` force it.
fn bench_range_proof_verify(c: &mut Criterion) {
    use wrfcoin_privacy::pedersen::PedersenCommitment;
    use wrfcoin_privacy::range_proof::RangeProof;

    // The commitment generates its own blinding; the proof must use the same
    // one or it would not verify, and a benchmark of a FAILING verify measures
    // the wrong thing.
    let (commitment, blinding) = PedersenCommitment::commit(42_000);
    let Ok(proof) = RangeProof::prove_default(42_000, &blinding) else {
        return;
    };
    assert!(
        proof.verify(&commitment).unwrap_or(false),
        "the benchmark must measure a SUCCEEDING verify"
    );
    c.bench_function("range_proof_verify", |b| {
        b.iter(|| proof.verify(&commitment).unwrap_or(false))
    });
}

criterion_group!(
    benches,
    bench_ed25519_verify,
    bench_mlsag_verify,
    bench_range_proof_verify
);
criterion_main!(benches);
