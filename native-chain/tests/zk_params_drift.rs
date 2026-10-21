//! Guard against the embedded `zk-params/` bundles drifting from the circuits.
//!
//! A Groth16 proving key is bound to the exact A/B/C matrices of the circuit it
//! was generated from. When a circuit changes and the committed key is not
//! regenerated, the failure is silent in the worst way: the public-input count
//! usually does not change, so the bundle still deserializes, `verify` still
//! runs to completion, and it simply returns `false`. No error, no panic, just a
//! wrong answer.
//!
//! That is what happened in #2283/#2274/#2240 — the bundles were generated
//! 2026-03-08 (#575) and the circuits moved to Poseidon commitments on
//! 2026-03-16 (#837, #819), leaving every committed key incompatible with the
//! circuit it claimed to serve, undetected for four months.
//!
//! `PROVENANCE.md`'s SHA-256 table cannot catch this: it pins the *integrity* of
//! the files, not their *freshness*, so it certified stale keys as good.
//!
//! # What this asserts, and why not a shape comparison
//!
//! The first version of this guard compared four vector lengths — public inputs,
//! variables, FFT domain, witnesses — and called that an R1CS fingerprint.
//! Review of #2288 was right that it is not one: a constraint coefficient, a
//! constant, or a change of wire selection alters the exact matrices while
//! leaving all four counts identical. A same-shape stale key would have sailed
//! through, and the mutation test that "proved" the guard worked only ever
//! exercised a shape-changing mutation (308 -> 68 variables), which is the easy
//! case.
//!
//! So the assertion is now the property itself rather than a proxy for it:
//! **produce a proof for a known-valid witness using the embedded proving key,
//! and verify it with the embedded prepared verifying key.** That is what the
//! node does at runtime, and it fails for *any* incompatibility — shape-changing
//! or not.
//!
//! Shape is still computed, but only to make a failure legible: it says whether
//! the drift was structural or a same-shape matrix change.
//!
//! When this fails, regenerate the bundles and update `PROVENANCE.md`:
//!
//! ```bash
//! cargo run --release -p wrfcoin-zk-circuits --example regen_zk_params
//! ```
// An empty test binary reports "0 passed; 0 failed; 0 ignored" — identical
// to a passing target and invisible behind sibling suites (the vacuity shape
// scripts/cargo_test_nonvacuous.py rejects as EMPTY_TEST_BINARY). Without the
// feature this target now declares its absence as a countable `ignored`
// instead of compiling to silence — #2748's principle, extended to the cfg
// dimension.
#[cfg(not(feature = "zk"))]
#[test]
#[ignore = "this target's tests require --features zk; without it the binary would be empty and indistinguishable from a passing one. Run: cargo test -p wrfcoin-native-chain --features zk --test zk_params_drift"]
fn tests_require_the_zk_feature() {}

#[cfg(feature = "zk")]
mod gated {
    use ark_bn254::{Bn254, Fr};
    use ark_groth16::{PreparedVerifyingKey, ProvingKey};
    use ark_serialize::CanonicalDeserialize;
    use std::io::Cursor;
    use std::time::Instant;

    use wrfcoin_zk_circuits::circuits::confidential_transfer::ConfidentialTransferCircuit;
    use wrfcoin_zk_circuits::circuits::poseidon_config::poseidon_hash;
    use wrfcoin_zk_circuits::circuits::range_proof::RangeProofCircuit;
    use wrfcoin_zk_circuits::circuits::selective_disclosure::SelectiveDisclosureCircuit;
    use wrfcoin_zk_circuits::circuits::weather_model::WeatherModelCircuit;
    use wrfcoin_zk_circuits::proving;

    const RANGE_PROOF: &[u8] = include_bytes!("../zk-params/range-proof-64.bin");
    const CONFIDENTIAL_TRANSFER: &[u8] = include_bytes!("../zk-params/confidential-transfer-1-1-64.bin");
    const SELECTIVE_DISCLOSURE: &[u8] = include_bytes!("../zk-params/selective-disclosure-3.bin");
    const WEATHER_MODEL: &[u8] = include_bytes!("../zk-params/weather-model-2.bin");

    type KeyPair = (ProvingKey<Bn254>, PreparedVerifyingKey<Bn254>);

    fn load(bundle: &[u8], name: &str) -> KeyPair {
        let mut cursor = Cursor::new(bundle);
        let pk = ProvingKey::<Bn254>::deserialize_compressed(&mut cursor)
            .unwrap_or_else(|e| panic!("{name}: embedded proving key failed to deserialize: {e}"));
        let pvk = PreparedVerifyingKey::<Bn254>::deserialize_compressed(&mut cursor)
            .unwrap_or_else(|e| panic!("{name}: embedded verifying key failed to deserialize: {e}"));
        assert_eq!(
            cursor.position(),
            bundle.len() as u64,
            "{name}: trailing bytes after the key pair"
        );
        // The prepared VK's serialized encoding is not stable across arkworks majors;
        // mirror zk_service: require the raw VKs agree, then re-derive (fail closed).
        assert!(pvk.vk == pk.vk, "{name}: embedded pvk.vk != pk.vk");
        let pvk = ark_groth16::prepare_verifying_key(&pk.vk);
        (pk, pvk)
    }

    /// Coarse structural summary — **diagnostic only**, never the assertion.
    /// Reported on failure so a reader can tell a structural drift (counts differ)
    /// from a same-shape matrix change (counts identical, proof still rejected).
    fn shape(pk: &ProvingKey<Bn254>) -> String {
        format!(
            "public_inputs={} variables={} domain={} witnesses={}",
            pk.vk.gamma_abc_g1.len(),
            pk.a_query.len(),
            pk.h_query.len(),
            pk.l_query.len(),
        )
    }

    /// Prove a known-valid witness with the embedded proving key, then verify it
    /// with the embedded prepared verifying key.
    ///
    /// This is the property the node depends on. It holds only if the embedded key
    /// was generated from the circuit as it stands right now.
    macro_rules! assert_embedded_key_proves_and_verifies {
        ($bundle:expr, $name:expr, $circuit:expr, $public_inputs:expr) => {{
            let (pk, pvk) = load($bundle, $name);
            let started = Instant::now();

            // A panic inside `prove` is ark-groth16's `assert!(cs.is_satisfied())`,
            // which compares witness to circuit and is key-independent. That would
            // mean the fixture below is wrong, not that the key is stale.
            let proof = proving::prove($circuit, &pk, $name).unwrap_or_else(|e| {
                panic!(
                    "\n\n{}: could not produce a proof with the embedded proving key: {e}\n\
                     embedded key: {}\n",
                    $name,
                    shape(&pk)
                )
            });

            let verified = proving::verify(&proof, &pvk, $public_inputs)
                .unwrap_or_else(|e| panic!("{}: verification errored: {e}", $name));

            eprintln!(
                "[zk-params] {:<34} prove+verify {:>7.1}ms  ({})",
                $name,
                started.elapsed().as_secs_f64() * 1000.0,
                shape(&pk)
            );

            assert!(
                verified,
                "\n\n{} in native-chain/zk-params/ no longer matches its circuit.\n\
                 The committed proving key produced a proof that its own verifying key rejects,\n\
                 so the key was generated from different A/B/C matrices than the circuit now\n\
                 defines. Proofs made with it fail silently in production — verification returns\n\
                 false rather than erroring.\n\n\
                 embedded key shape: {}\n\
                 If that shape looks correct, this was a same-shape change: a coefficient, a\n\
                 constant, or a different wire selection. Shape equality does not imply key\n\
                 compatibility, which is why this guard proves rather than measures.\n\n\
                 Regenerate with:\n  \
                 cargo run --release -p wrfcoin-zk-circuits --example regen_zk_params\n\
                 then update the SHA-256 table in native-chain/zk-params/PROVENANCE.md.\n",
                $name,
                shape(&pk),
            );
        }};
    }

    #[test]
    fn range_proof_params_match_circuit() {
        let blinding = Fr::from(42u64);
        let commitment = RangeProofCircuit::compute_commitment(1000, blinding);
        assert_embedded_key_proves_and_verifies!(
            RANGE_PROOF,
            "range-proof-64.bin",
            RangeProofCircuit::new(commitment, Fr::from(1000u64), blinding, 64),
            &[commitment]
        );
    }

    #[test]
    fn confidential_transfer_params_match_circuit() {
        // Blinding sums conserved, values balanced against the fee (#786).
        let input_values = vec![Fr::from(100u64)];
        let input_blindings = vec![Fr::from(42u64)];
        let output_values = vec![Fr::from(90u64)];
        let output_blindings = vec![Fr::from(42u64)];
        let fee = Fr::from(10u64);

        let input_comm =
            ConfidentialTransferCircuit::compute_commitment(&input_values, &input_blindings);
        let output_comm =
            ConfidentialTransferCircuit::compute_commitment(&output_values, &output_blindings);

        assert_embedded_key_proves_and_verifies!(
            CONFIDENTIAL_TRANSFER,
            "confidential-transfer-1-1-64.bin",
            ConfidentialTransferCircuit::new(
                input_comm,
                output_comm,
                fee,
                input_values,
                input_blindings,
                output_values,
                output_blindings,
                64,
            ),
            &[input_comm, output_comm, fee]
        );
    }

    #[test]
    fn selective_disclosure_params_match_circuit() {
        let fields = vec![Fr::from(42u64), Fr::from(100u64), Fr::from(7u64)];
        let commitment = SelectiveDisclosureCircuit::compute_commitment(&fields);
        assert_embedded_key_proves_and_verifies!(
            SELECTIVE_DISCLOSURE,
            "selective-disclosure-3.bin",
            SelectiveDisclosureCircuit::new(commitment, fields),
            &[commitment]
        );
    }

    #[test]
    fn weather_model_params_match_circuit() {
        let samples = [Fr::from(10u64), Fr::from(20u64)];
        let model_id = Fr::from(1u64);
        let input_hash = Fr::from(100u64);
        let exec_time = Fr::from(3600u64);
        // The circuit enforces Poseidon(samples || model_id || input_hash).
        let output_hash = poseidon_hash(&[samples[0], samples[1], model_id, input_hash]);

        assert_embedded_key_proves_and_verifies!(
            WEATHER_MODEL,
            "weather-model-2.bin",
            WeatherModelCircuit::new(
                input_hash,
                output_hash,
                model_id,
                exec_time,
                samples.to_vec(),
            ),
            &[input_hash, output_hash, model_id, exec_time]
        );
    }
}
