use ark_bn254::Fr;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use wrfcoin_zk_circuits::circuits::confidential_transfer::ConfidentialTransferCircuit;
use wrfcoin_zk_circuits::circuits::range_proof::RangeProofCircuit;
use wrfcoin_zk_circuits::circuits::selective_disclosure::SelectiveDisclosureCircuit;
use wrfcoin_zk_circuits::proving;

fn bench_range_proof(c: &mut Criterion) {
    c.bench_function("range_proof_generate_keys", |b| {
        b.iter_with_setup(
            || RangeProofCircuit::blank(64),
            |blank| {
                black_box(proving::generate_keys(black_box(blank)).expect("keygen should succeed"))
            },
        );
    });

    let blinding = Fr::from(42_u64);
    let value = 12345_u64;
    let commitment = RangeProofCircuit::compute_commitment(value, blinding);
    let range_pk = proving::generate_keys(RangeProofCircuit::blank(64))
        .expect("range proof keys should generate")
        .0;

    c.bench_function("range_proof_prove", |b| {
        b.iter_with_setup(
            || {
                (
                    RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64),
                    range_pk.clone(),
                )
            },
            |(circuit, pk)| {
                black_box(
                    proving::prove(black_box(circuit), black_box(&pk), "range-proof")
                        .expect("range proof generation should succeed"),
                )
            },
        );
    });

    let (range_verify_pk, range_verify_pvk) = proving::generate_keys(RangeProofCircuit::blank(64))
        .expect("range verify keys should generate");

    c.bench_function("range_proof_verify", |b| {
        b.iter_with_setup(
            || {
                let circuit = RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64);
                let proof = proving::prove(circuit, &range_verify_pk, "range-proof")
                    .expect("range proof should generate for verify bench");
                (proof, range_verify_pvk.clone(), vec![commitment])
            },
            |(proof, pvk, public_inputs)| {
                black_box(
                    proving::verify(
                        black_box(&proof),
                        black_box(&pvk),
                        black_box(&public_inputs),
                    )
                    .expect("range proof verification should succeed"),
                )
            },
        );
    });
}

fn bench_confidential_transfer(c: &mut Criterion) {
    c.bench_function("confidential_transfer_generate_keys", |b| {
        b.iter_with_setup(
            || ConfidentialTransferCircuit::blank(1, 1, 64),
            |blank| {
                black_box(proving::generate_keys(black_box(blank)).expect("keygen should succeed"))
            },
        );
    });

    let input_vals = vec![Fr::from(100_u64)];
    let input_blinds = vec![Fr::from(42_u64)];
    let output_vals = vec![Fr::from(90_u64)];
    let output_blinds = vec![Fr::from(10_u64)];
    let fee = Fr::from(10_u64);
    let input_commitment =
        ConfidentialTransferCircuit::compute_commitment(&input_vals, &input_blinds);
    let output_commitment =
        ConfidentialTransferCircuit::compute_commitment(&output_vals, &output_blinds);
    let confidential_pk = proving::generate_keys(ConfidentialTransferCircuit::blank(1, 1, 64))
        .expect("confidential transfer keys should generate")
        .0;

    c.bench_function("confidential_transfer_prove", |b| {
        b.iter_with_setup(
            || {
                (
                    ConfidentialTransferCircuit::new(
                        input_commitment,
                        output_commitment,
                        fee,
                        input_vals.clone(),
                        input_blinds.clone(),
                        output_vals.clone(),
                        output_blinds.clone(),
                        64,
                    ),
                    confidential_pk.clone(),
                )
            },
            |(circuit, pk)| {
                black_box(
                    proving::prove(black_box(circuit), black_box(&pk), "confidential-transfer")
                        .expect("confidential transfer proof generation should succeed"),
                )
            },
        );
    });

    let (confidential_verify_pk, confidential_verify_pvk) =
        proving::generate_keys(ConfidentialTransferCircuit::blank(1, 1, 64))
            .expect("confidential transfer verify keys should generate");

    c.bench_function("confidential_transfer_verify", |b| {
        b.iter_with_setup(
            || {
                let circuit = ConfidentialTransferCircuit::new(
                    input_commitment,
                    output_commitment,
                    fee,
                    input_vals.clone(),
                    input_blinds.clone(),
                    output_vals.clone(),
                    output_blinds.clone(),
                    64,
                );
                let proof =
                    proving::prove(circuit, &confidential_verify_pk, "confidential-transfer")
                        .expect("confidential transfer proof should generate for verify bench");
                (
                    proof,
                    confidential_verify_pvk.clone(),
                    vec![input_commitment, output_commitment, fee],
                )
            },
            |(proof, pvk, public_inputs)| {
                black_box(
                    proving::verify(
                        black_box(&proof),
                        black_box(&pvk),
                        black_box(&public_inputs),
                    )
                    .expect("confidential transfer verification should succeed"),
                )
            },
        );
    });
}

fn bench_selective_disclosure(c: &mut Criterion) {
    c.bench_function("selective_disclosure_generate_keys", |b| {
        b.iter_with_setup(
            || SelectiveDisclosureCircuit::blank(3),
            |blank| {
                black_box(proving::generate_keys(black_box(blank)).expect("keygen should succeed"))
            },
        );
    });

    let fields = vec![Fr::from(42_u64), Fr::from(100_u64), Fr::from(7_u64)];
    let commitment = SelectiveDisclosureCircuit::compute_commitment(&fields);
    let selective_pk = proving::generate_keys(SelectiveDisclosureCircuit::blank(3))
        .expect("selective disclosure keys should generate")
        .0;

    c.bench_function("selective_disclosure_prove", |b| {
        b.iter_with_setup(
            || {
                (
                    SelectiveDisclosureCircuit::new(commitment, fields.clone()),
                    selective_pk.clone(),
                )
            },
            |(circuit, pk)| {
                black_box(
                    proving::prove(black_box(circuit), black_box(&pk), "selective-disclosure")
                        .expect("selective disclosure proof generation should succeed"),
                )
            },
        );
    });

    let (selective_verify_pk, selective_verify_pvk) =
        proving::generate_keys(SelectiveDisclosureCircuit::blank(3))
            .expect("selective disclosure verify keys should generate");

    c.bench_function("selective_disclosure_verify", |b| {
        b.iter_with_setup(
            || {
                let circuit = SelectiveDisclosureCircuit::new(commitment, fields.clone());
                let proof = proving::prove(circuit, &selective_verify_pk, "selective-disclosure")
                    .expect("selective disclosure proof should generate for verify bench");
                (proof, selective_verify_pvk.clone(), vec![commitment])
            },
            |(proof, pvk, public_inputs)| {
                black_box(
                    proving::verify(
                        black_box(&proof),
                        black_box(&pvk),
                        black_box(&public_inputs),
                    )
                    .expect("selective disclosure verification should succeed"),
                )
            },
        );
    });
}

fn zk_benches(c: &mut Criterion) {
    bench_range_proof(c);
    bench_confidential_transfer(c);
    bench_selective_disclosure(c);
}

criterion_group!(benches, zk_benches);
criterion_main!(benches);
