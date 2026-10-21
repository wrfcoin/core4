use ark_bn254::Fr;
use ark_crypto_primitives::sponge::{poseidon::PoseidonSponge, CryptographicSponge};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use wrfcoin_zk_circuits::{
    circuits::{
        confidential_transfer::ConfidentialTransferCircuit,
        poseidon_config::poseidon_config_bn254, range_proof::RangeProofCircuit,
        selective_disclosure::SelectiveDisclosureCircuit, weather_model::WeatherModelCircuit,
    },
    proving,
};

fn bench_range_proof(c: &mut Criterion) {
    let blinding = Fr::from(42_u64);
    let value = 12_345_u64;
    let commitment = RangeProofCircuit::compute_commitment(value, blinding);
    let (pk, pvk) =
        proving::generate_keys(RangeProofCircuit::blank(64)).expect("range keys should generate");

    c.bench_function("range_proof_generate", |b| {
        b.iter(|| {
            let circuit = RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64);
            black_box(proving::prove(circuit, &pk, "range-proof").expect("range prove"))
        });
    });

    let verify_proof = proving::prove(
        RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64),
        &pk,
        "range-proof",
    )
    .expect("range proof for verify");
    c.bench_function("range_proof_verify", |b| {
        b.iter(|| {
            black_box(proving::verify(&verify_proof, &pvk, &[commitment]).expect("range verify"))
        });
    });
}

fn bench_range_proof_32bit(c: &mut Criterion) {
    let blinding = Fr::from(42_u64);
    let value = 12_345_u32;
    let commitment = RangeProofCircuit::compute_commitment(value as u64, blinding);
    let (pk, pvk) = proving::generate_keys(RangeProofCircuit::blank(32))
        .expect("32-bit range keys should generate");

    c.bench_function("range_proof_generate_32bit", |b| {
        b.iter(|| {
            let circuit =
                RangeProofCircuit::new(commitment, Fr::from(value as u64), blinding, 32);
            black_box(proving::prove(circuit, &pk, "range-proof-32").expect("range prove 32"))
        });
    });

    let verify_proof = proving::prove(
        RangeProofCircuit::new(commitment, Fr::from(value as u64), blinding, 32),
        &pk,
        "range-proof-32",
    )
    .expect("32-bit range proof for verify");
    c.bench_function("range_proof_verify_32bit", |b| {
        b.iter(|| {
            black_box(
                proving::verify(&verify_proof, &pvk, &[commitment]).expect("range verify 32"),
            )
        });
    });
}

fn bench_confidential_transfer(c: &mut Criterion) {
    let input_vals = vec![Fr::from(100_u64)];
    let input_blinds = vec![Fr::from(42_u64)];
    let output_vals = vec![Fr::from(90_u64)];
    let output_blinds = vec![Fr::from(10_u64)];
    let fee = Fr::from(10_u64);
    let input_commitment =
        ConfidentialTransferCircuit::compute_commitment(&input_vals, &input_blinds);
    let output_commitment =
        ConfidentialTransferCircuit::compute_commitment(&output_vals, &output_blinds);
    let (pk, pvk) = proving::generate_keys(ConfidentialTransferCircuit::blank(1, 1, 64))
        .expect("confidential keys should generate");

    c.bench_function("confidential_transfer_generate", |b| {
        b.iter(|| {
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
            black_box(
                proving::prove(circuit, &pk, "confidential-transfer")
                    .expect("confidential transfer prove"),
            )
        });
    });

    let verify_proof = proving::prove(
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
        &pk,
        "confidential-transfer",
    )
    .expect("confidential proof for verify");
    c.bench_function("confidential_transfer_verify", |b| {
        b.iter(|| {
            black_box(
                proving::verify(
                    &verify_proof,
                    &pvk,
                    &[input_commitment, output_commitment, fee],
                )
                .expect("confidential verify"),
            )
        });
    });
}

fn bench_selective_disclosure(c: &mut Criterion) {
    let fields = vec![Fr::from(42_u64), Fr::from(100_u64), Fr::from(7_u64)];
    let commitment = SelectiveDisclosureCircuit::compute_commitment(&fields);
    let (pk, pvk) = proving::generate_keys(SelectiveDisclosureCircuit::blank(3))
        .expect("selective disclosure keys should generate");

    c.bench_function("selective_disclosure_generate", |b| {
        b.iter(|| {
            let circuit = SelectiveDisclosureCircuit::new(commitment, fields.clone());
            black_box(
                proving::prove(circuit, &pk, "selective-disclosure")
                    .expect("selective disclosure prove"),
            )
        });
    });

    let verify_proof = proving::prove(
        SelectiveDisclosureCircuit::new(commitment, fields.clone()),
        &pk,
        "selective-disclosure",
    )
    .expect("selective proof for verify");
    c.bench_function("selective_disclosure_verify", |b| {
        b.iter(|| {
            black_box(
                proving::verify(&verify_proof, &pvk, &[commitment]).expect("selective verify"),
            )
        });
    });
}

fn bench_weather_model(c: &mut Criterion) {
    let input_hash = Fr::from(10_u64);
    let model_id = Fr::from(7_u64);
    let execution_time_secs = Fr::from(15_u64);
    let state_samples = vec![Fr::from(3_u64), Fr::from(4_u64), Fr::from(5_u64)];
    let output_hash = {
        let config = poseidon_config_bn254();
        let mut sponge = PoseidonSponge::<Fr>::new(&config);
        for s in &state_samples {
            sponge.absorb(s);
        }
        sponge.absorb(&model_id);
        sponge.absorb(&input_hash);
        let out: Vec<Fr> = sponge.squeeze_field_elements(1);
        out[0]
    };
    let (pk, pvk) = proving::generate_keys(WeatherModelCircuit::blank(state_samples.len()))
        .expect("weather model keys should generate");

    c.bench_function("weather_model_generate", |b| {
        b.iter(|| {
            let circuit = WeatherModelCircuit::new(
                input_hash,
                output_hash,
                model_id,
                execution_time_secs,
                state_samples.clone(),
            );
            black_box(proving::prove(circuit, &pk, "weather-model").expect("weather model prove"))
        });
    });

    let verify_proof = proving::prove(
        WeatherModelCircuit::new(
            input_hash,
            output_hash,
            model_id,
            execution_time_secs,
            state_samples.clone(),
        ),
        &pk,
        "weather-model",
    )
    .expect("weather proof for verify");
    c.bench_function("weather_model_verify", |b| {
        b.iter(|| {
            black_box(
                proving::verify(
                    &verify_proof,
                    &pvk,
                    &[input_hash, output_hash, model_id, execution_time_secs],
                )
                .expect("weather verify"),
            )
        });
    });
}

fn bench_batch_verification(c: &mut Criterion) {
    let blinding = Fr::from(42_u64);
    let value = 123_u64;
    let commitment = RangeProofCircuit::compute_commitment(value, blinding);
    let (pk, pvk) =
        proving::generate_keys(RangeProofCircuit::blank(64)).expect("range keys should generate");
    let proofs = (0..10)
        .map(|_| {
            proving::prove(
                RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64),
                &pk,
                "range-proof",
            )
            .expect("range proof for batch verify")
        })
        .collect::<Vec<_>>();

    c.bench_function("sequential_verify_10_proofs", |b| {
        b.iter(|| {
            let all_valid = proofs
                .iter()
                .all(|proof| proving::verify(proof, &pvk, &[commitment]).unwrap_or(false));
            black_box(all_valid);
        });
    });

    c.bench_function("batch_verify_10_proofs", |b| {
        let proof_refs: Vec<&_> = proofs.iter().collect();
        let input_arrays: Vec<Vec<Fr>> = (0..10).map(|_| vec![commitment]).collect();
        let inputs: Vec<&[Fr]> = input_arrays.iter().map(|v| v.as_slice()).collect();
        b.iter(|| {
            black_box(proving::batch_verify(&proof_refs, &pvk, &inputs).unwrap());
        });
    });
}

fn proof_benches(c: &mut Criterion) {
    bench_range_proof(c);
    bench_range_proof_32bit(c);
    bench_confidential_transfer(c);
    bench_selective_disclosure(c);
    bench_weather_model(c);
    bench_batch_verification(c);
}

criterion_group!(benches, proof_benches);
criterion_main!(benches);
