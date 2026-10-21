use ark_bn254::Fr;
use ark_relations::gr1cs::{ConstraintSynthesizer, ConstraintSystem};
use wrfcoin_zk_circuits::bench_matrix::CircuitKind;
use wrfcoin_zk_circuits::circuits::{
    confidential_transfer::ConfidentialTransferCircuit,
    poseidon_config::poseidon_hash,
    range_proof::RangeProofCircuit,
    selective_disclosure::SelectiveDisclosureCircuit,
    weather_model::WeatherModelCircuit,
};

fn circuit_accepts<C: ConstraintSynthesizer<Fr>>(circuit: C) -> bool {
    let cs = ConstraintSystem::<Fr>::new_ref();
    circuit.generate_constraints(cs.clone()).is_ok() && cs.is_satisfied().unwrap_or(false)
}

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn range_circuit_matches_native_accept_and_reject() {
    let mut seed = 0x2179_0001_u64;
    for _ in 0..32 {
        let value = next(&mut seed) & 0xffff;
        let blinding = Fr::from(next(&mut seed));
        let native_commitment = poseidon_hash(&[Fr::from(value), blinding]);
        assert_eq!(
            RangeProofCircuit::compute_commitment(value, blinding),
            native_commitment,
            "public commitment must match the native function"
        );
        assert!(circuit_accepts(RangeProofCircuit::new(
            native_commitment,
            Fr::from(value),
            blinding,
            16,
        )));

        let wrong_preimage = value ^ 1;
        assert!(!circuit_accepts(RangeProofCircuit::new(
            native_commitment,
            Fr::from(wrong_preimage),
            blinding,
            16,
        )));
    }

    let out_of_range = 1_u64 << 16;
    let blinding = Fr::from(7_u64);
    let commitment = RangeProofCircuit::compute_commitment(out_of_range, blinding);
    assert!(!circuit_accepts(RangeProofCircuit::new(
        commitment,
        Fr::from(out_of_range),
        blinding,
        16,
    )));
}

#[test]
fn transfer_circuit_matches_native_accept_and_reject() {
    let mut seed = 0x2179_0002_u64;
    for _ in 0..24 {
        let inputs = [1_000 + next(&mut seed) % 10_000, 1_000 + next(&mut seed) % 10_000];
        let fee = 1 + next(&mut seed) % 100;
        let total_in: u64 = inputs.iter().sum();
        let total_out = total_in - fee;
        let outputs = [total_out / 2, total_out - total_out / 2];
        let input_blindings = [11_u64, 29_u64];
        let output_blindings = [17_u64, 23_u64];

        let input_values: Vec<Fr> = inputs.into_iter().map(Fr::from).collect();
        let output_values: Vec<Fr> = outputs.into_iter().map(Fr::from).collect();
        let input_blinds: Vec<Fr> = input_blindings.into_iter().map(Fr::from).collect();
        let output_blinds: Vec<Fr> = output_blindings.into_iter().map(Fr::from).collect();
        let input_commitment = poseidon_hash(&[Fr::from(total_in), Fr::from(40_u64)]);
        let output_commitment = poseidon_hash(&[Fr::from(total_out), Fr::from(40_u64)]);

        assert_eq!(
            ConfidentialTransferCircuit::compute_commitment(&input_values, &input_blinds),
            input_commitment
        );
        assert_eq!(
            ConfidentialTransferCircuit::compute_commitment(&output_values, &output_blinds),
            output_commitment
        );
        assert!(circuit_accepts(ConfidentialTransferCircuit::new(
            input_commitment,
            output_commitment,
            Fr::from(fee),
            input_values.clone(),
            input_blinds.clone(),
            output_values.clone(),
            output_blinds.clone(),
            16,
        )));

        // The native balance predicate rejects a wrong fee; the circuit must
        // reject in the same direction with otherwise valid openings.
        assert_ne!(total_in, total_out + fee + 1);
        assert!(!circuit_accepts(ConfidentialTransferCircuit::new(
            input_commitment,
            output_commitment,
            Fr::from(fee + 1),
            input_values.clone(),
            input_blinds.clone(),
            output_values.clone(),
            output_blinds.clone(),
            16,
        )));

        // Each negative below breaks exactly ONE of the circuit's equalities and
        // keeps every other one satisfied, so each fails only if that specific
        // constraint is enforced.
        let build = |ic: Fr, oc: Fr, ob: &[Fr]| {
            ConfidentialTransferCircuit::new(
                ic,
                oc,
                Fr::from(fee),
                input_values.clone(),
                input_blinds.clone(),
                output_values.clone(),
                ob.to_vec(),
                16,
            )
        };

        // Input-commitment equality: the public input commitment is not the
        // Poseidon opening of the (valid) input witness.
        assert!(!circuit_accepts(build(
            input_commitment + Fr::from(1_u64),
            output_commitment,
            &output_blinds
        )));

        // Output-commitment equality: likewise for the output side.
        assert!(!circuit_accepts(build(
            input_commitment,
            output_commitment + Fr::from(1_u64),
            &output_blinds
        )));

        // Blinding-sum conservation (#786): output blindings sum to 41, not 40.
        // The output commitment is recomputed over the new sum, so the output
        // opening, the input opening and the value balance all still hold; only
        // sum(input_blindings) == sum(output_blindings) is violated.
        let skewed_blinds = vec![output_blinds[0] + Fr::from(1_u64), output_blinds[1]];
        let skewed_output_commitment = poseidon_hash(&[Fr::from(total_out), Fr::from(41_u64)]);
        assert_eq!(
            ConfidentialTransferCircuit::compute_commitment(&output_values, &skewed_blinds),
            skewed_output_commitment
        );
        assert!(!circuit_accepts(build(
            input_commitment,
            skewed_output_commitment,
            &skewed_blinds
        )));
    }
}

#[test]
fn disclosure_circuit_matches_native_and_defeats_the_819_attack() {
    fn old_linear_commitment(fields: &[Fr]) -> Fr {
        fields
            .iter()
            .enumerate()
            .map(|(index, field)| *field * Fr::from(index as u64 + 1))
            .sum()
    }

    // #819's exact attack class: freely alter n-1 fields, then solve the last
    // linear term. These are different witnesses for the same old commitment.
    let original = vec![Fr::from(10_u64), Fr::from(20_u64), Fr::from(30_u64)];
    let forged = vec![Fr::from(12_u64), Fr::from(19_u64), Fr::from(30_u64)];
    assert_ne!(original, forged);
    assert_eq!(old_linear_commitment(&original), old_linear_commitment(&forged));

    let production_commitment = SelectiveDisclosureCircuit::compute_commitment(&original);
    assert_ne!(
        production_commitment,
        SelectiveDisclosureCircuit::compute_commitment(&forged),
        "Poseidon must break the concrete linear field-split forgery from #819"
    );
    assert!(!circuit_accepts(SelectiveDisclosureCircuit::new(
        production_commitment,
        forged,
    )));

    let mut seed = 0x2179_0003_u64;
    for _ in 0..24 {
        let fields: Vec<Fr> = (0..4).map(|_| Fr::from(next(&mut seed))).collect();
        let native_commitment = poseidon_hash(&fields);
        assert_eq!(
            SelectiveDisclosureCircuit::compute_commitment(&fields),
            native_commitment
        );
        assert!(circuit_accepts(SelectiveDisclosureCircuit::new(
            native_commitment,
            fields.clone(),
        )));
        let mut wrong_witness = fields;
        wrong_witness[0] += Fr::from(1_u64);
        assert!(!circuit_accepts(SelectiveDisclosureCircuit::new(
            native_commitment,
            wrong_witness,
        )));
    }
}

#[test]
fn weather_circuit_matches_native_accept_and_reject() {
    let mut seed = 0x2179_0004_u64;
    for _ in 0..24 {
        let input_hash = Fr::from(next(&mut seed));
        let model_id = Fr::from(1 + next(&mut seed) % 32);
        let state_samples: Vec<Fr> = (0..4).map(|_| Fr::from(next(&mut seed))).collect();
        let mut preimage = state_samples.clone();
        preimage.push(model_id);
        preimage.push(input_hash);
        let native_output = poseidon_hash(&preimage);

        assert!(circuit_accepts(WeatherModelCircuit::new(
            input_hash,
            native_output,
            model_id,
            Fr::from(1_u64),
            state_samples.clone(),
        )));
        assert!(!circuit_accepts(WeatherModelCircuit::new(
            input_hash,
            native_output + Fr::from(1_u64),
            model_id,
            Fr::from(1_u64),
            state_samples.clone(),
        )));
        assert!(!circuit_accepts(WeatherModelCircuit::new(
            input_hash,
            native_output,
            model_id,
            Fr::from(0_u64),
            state_samples,
        )));
    }
}

#[test]
fn canonical_constraint_counts_do_not_drop_silently() {
    let expected = [
        (CircuitKind::RangeProof { n_bits: 64 }, 306),
        (
            CircuitKind::ConfidentialTransfer {
                num_inputs: 1,
                num_outputs: 1,
                n_bits: 64,
            },
            614,
        ),
        (CircuitKind::SelectiveDisclosure { num_fields: 64 }, 7_774),
        (CircuitKind::WeatherModel { num_samples: 128 }, 15_794),
    ];

    for (circuit, count) in expected {
        assert_eq!(circuit.num_constraints().unwrap(), count, "{}", circuit.id());
    }
}

