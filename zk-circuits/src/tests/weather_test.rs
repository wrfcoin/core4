#[cfg(test)]
mod tests {
    use crate::circuits::poseidon_config::poseidon_config_bn254;
    use crate::circuits::weather_model::WeatherModelCircuit;
    use crate::proving::{generate_keys, prove, verify};
    use ark_bn254::{Bn254, Fr};
    use ark_crypto_primitives::sponge::{poseidon::PoseidonSponge, CryptographicSponge};
    use ark_groth16::Proof;
    use ark_serialize::CanonicalDeserialize;

    /// Compute the expected output_hash natively: Poseidon(samples || model_id || input_hash).
    fn expected_output_hash(samples: &[Fr], model_id: Fr, input_hash: Fr) -> Fr {
        let config = poseidon_config_bn254();
        let mut sponge = PoseidonSponge::new(&config);
        for s in samples {
            sponge.absorb(s);
        }
        sponge.absorb(&model_id);
        sponge.absorb(&input_hash);
        let out: Vec<Fr> = sponge.squeeze_field_elements(1);
        out[0]
    }

    #[test]
    fn test_weather_model_circuit_success() {
        let input_hash = Fr::from(100u64);
        let model_id = Fr::from(1u64);
        let time = Fr::from(3600u64);
        let samples = vec![Fr::from(10u64), Fr::from(20u64)];

        let output_hash = expected_output_hash(&samples, model_id, input_hash);

        let circuit = WeatherModelCircuit::new(input_hash, output_hash, model_id, time, samples);

        let blank = WeatherModelCircuit::blank(2);
        let (pk, pvk) = generate_keys(blank).expect("Keygen failed");

        let proof = prove(circuit, &pk, "weather-v1").expect("Proving failed");
        Proof::<Bn254>::deserialize_compressed(&proof.proof_bytes[..])
            .expect("proof bytes should parse");

        let public_inputs = vec![input_hash, output_hash, model_id, time];
        let is_valid = verify(&proof, &pvk, &public_inputs).expect("Verification failed");

        assert!(
            !proof.proof_bytes.is_empty(),
            "proof serialization should not be empty"
        );
        assert!(is_valid, "Valid proof was rejected");
    }

    #[test]
    fn test_weather_model_circuit_failure_tampered_output() {
        let input_hash = Fr::from(100u64);
        let model_id = Fr::from(1u64);
        let time = Fr::from(3600u64);
        let samples = vec![Fr::from(10u64), Fr::from(20u64)];
        let output_hash = expected_output_hash(&samples, model_id, input_hash);

        let circuit = WeatherModelCircuit::new(input_hash, output_hash, model_id, time, samples);

        let blank = WeatherModelCircuit::blank(2);
        let (pk, pvk) = generate_keys(blank).expect("Keygen failed");
        let proof = prove(circuit, &pk, "weather-v1").expect("Proving failed");
        Proof::<Bn254>::deserialize_compressed(&proof.proof_bytes[..])
            .expect("proof bytes should parse");

        // Tamper with output hash
        let tampered_inputs = vec![input_hash, output_hash + Fr::from(1u64), model_id, time];
        let is_valid = verify(&proof, &pvk, &tampered_inputs).expect("Verification failed");

        assert!(!is_valid, "Tampered proof was accepted");
    }
}
