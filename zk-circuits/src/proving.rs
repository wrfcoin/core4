use ark_bn254::{Bn254, Fr};
use ark_ec::pairing::Pairing;
use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::{Field, PrimeField};
use ark_groth16::{Groth16, PreparedVerifyingKey, Proof, ProvingKey};
use ark_relations::gr1cs::ConstraintSynthesizer;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use ark_std::{UniformRand, Zero};
use core::ops::Neg;
use rand::rngs::OsRng;

use crate::error::ZkError;
use crate::types::ZkProof;

/// Generate proving and verification keys for a given circuit.
///
/// # Security: Single-party trusted setup
///
/// This function uses `OsRng` (the OS CSPRNG) as the sole entropy source.
/// The toxic waste (trapdoor tau) exists transiently in memory and is dropped
/// when this function returns, but it is NOT multi-party: a compromised host
/// during generation can recover tau and forge proofs.
///
/// For **testnet** this is acceptable. For **mainnet**, replace the embedded
/// key bundles with MPC-ceremony-derived keys (see issues #717, #716 and
/// `native-chain/zk-params/PROVENANCE.md`).
pub fn generate_keys<C: ConstraintSynthesizer<Fr>>(
    circuit: C,
) -> Result<(ProvingKey<Bn254>, PreparedVerifyingKey<Bn254>), ZkError> {
    let mut rng = OsRng;
    let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit, &mut rng)
        .map_err(|e| ZkError::KeyGenerationFailed(e.to_string()))?;
    let pvk = Groth16::<Bn254>::process_vk(&vk)
        .map_err(|e| ZkError::KeyGenerationFailed(e.to_string()))?;
    Ok((pk, pvk))
}

/// Generate a Groth16 proof for a given circuit and proving key.
pub fn prove<C: ConstraintSynthesizer<Fr>>(
    circuit: C,
    pk: &ProvingKey<Bn254>,
    circuit_id: &str,
) -> Result<ZkProof, ZkError> {
    let mut rng = OsRng;
    let proof = Groth16::<Bn254>::prove(pk, circuit, &mut rng)
        .map_err(|e| ZkError::ProofGenerationFailed(e.to_string()))?;
    let mut proof_bytes = Vec::new();
    proof
        .serialize_compressed(&mut proof_bytes)
        .map_err(|e| ZkError::SerializationError(e.to_string()))?;
    Ok(ZkProof {
        proof_bytes,
        circuit_id: circuit_id.to_string(),
    })
}

/// Verify a Groth16 proof against public inputs.
pub fn verify(
    zk_proof: &ZkProof,
    pvk: &PreparedVerifyingKey<Bn254>,
    public_inputs: &[Fr],
) -> Result<bool, ZkError> {
    let proof = Proof::<Bn254>::deserialize_compressed(&zk_proof.proof_bytes[..])
        .map_err(|e| ZkError::SerializationError(e.to_string()))?;
    Groth16::<Bn254>::verify_with_processed_vk(pvk, public_inputs, &proof)
        .map_err(|e| ZkError::VerificationFailed(e.to_string()))
}

/// Batch-verify multiple Groth16 proofs against the same verification key.
///
/// Uses randomized linear combination for computational soundness: if any
/// single proof is invalid, the batch check fails with overwhelming probability
/// (1 - 1/|Fr|). All proofs must be for the same circuit (same pvk).
///
/// Performance: one `final_exponentiation` (the dominant BN254 cost) instead
/// of `n`, yielding ~3-5x speedup for batches of 10+ proofs.
pub fn batch_verify(
    proofs: &[&ZkProof],
    pvk: &PreparedVerifyingKey<Bn254>,
    public_inputs: &[&[Fr]],
) -> Result<bool, ZkError> {
    if proofs.len() != public_inputs.len() {
        return Err(ZkError::InvalidInput(
            "proofs and public_inputs length mismatch".into(),
        ));
    }
    if proofs.is_empty() {
        return Ok(true);
    }
    if proofs.len() == 1 {
        return verify(proofs[0], pvk, public_inputs[0]);
    }

    let mut rng = OsRng;
    let n = proofs.len();

    // Random scalars for linear combination (first = 1 for efficiency)
    let rs: Vec<Fr> = core::iter::once(Fr::from(1u64))
        .chain((1..n).map(|_| Fr::rand(&mut rng)))
        .collect();

    // Deserialize all proofs
    let ark_proofs: Vec<Proof<Bn254>> = proofs
        .iter()
        .map(|p| {
            Proof::<Bn254>::deserialize_compressed(&p.proof_bytes[..])
                .map_err(|e| ZkError::SerializationError(e.to_string()))
        })
        .collect::<Result<_, _>>()?;

    // Prepare public inputs for each proof
    let prepared_inputs: Vec<<Bn254 as Pairing>::G1> = public_inputs
        .iter()
        .map(|pi| {
            Groth16::<Bn254>::prepare_inputs(pvk, pi)
                .map_err(|e| ZkError::VerificationFailed(e.to_string()))
        })
        .collect::<Result<_, _>>()?;

    // Build pairing terms for multi_miller_loop.
    //
    // The Groth16 verification equation for a single proof is:
    //   e(A, B) · e(D, -γ) · e(C, -δ) = e(α, β)
    // where D = prepare_inputs(pvk, public_inputs).
    //
    // For batch with random scalars r_i, we check:
    //   ∏_i e(r_i·A_i, B_i) · e(Σ r_i·D_i, -γ) · e(Σ r_i·C_i, -δ) · e(-r_sum·α, β) = 1
    //
    // This uses n+3 pairing terms with 1 final_exponentiation instead of
    // 3n terms with n final_exponentiations.

    let mut g1s: Vec<<Bn254 as Pairing>::G1Prepared> = Vec::with_capacity(n + 3);
    let mut g2s: Vec<<Bn254 as Pairing>::G2Prepared> = Vec::with_capacity(n + 3);

    let mut sum_d = <Bn254 as Pairing>::G1::zero();
    let mut sum_c = <Bn254 as Pairing>::G1::zero();

    for i in 0..n {
        let r_bigint = rs[i].into_bigint();

        // r_i · A_i paired with B_i
        let r_a = ark_proofs[i].a.mul_bigint(r_bigint);
        g1s.push(r_a.into_affine().into());
        g2s.push(ark_proofs[i].b.into());

        // Accumulate prepared inputs and C for aggregated terms
        sum_d += prepared_inputs[i].into_affine().mul_bigint(r_bigint);
        sum_c += ark_proofs[i].c.mul_bigint(r_bigint);
    }

    // Aggregated γ term: (Σ r_i·D_i, -γ)
    g1s.push(sum_d.into_affine().into());
    g2s.push(pvk.gamma_g2_neg_pc.clone());

    // Aggregated δ term: (Σ r_i·C_i, -δ)
    g1s.push(sum_c.into_affine().into());
    g2s.push(pvk.delta_g2_neg_pc.clone());

    // α·β cancellation: (-r_sum·α, β)
    let r_sum: Fr = rs.iter().copied().sum();
    let neg_r_sum_alpha = pvk.vk.alpha_g1.mul_bigint(r_sum.into_bigint()).neg();
    g1s.push(neg_r_sum_alpha.into_affine().into());
    g2s.push(pvk.vk.beta_g2.into());

    // Single multi_miller_loop + final_exponentiation
    let ml = Bn254::multi_miller_loop(g1s, g2s);
    let result = Bn254::final_exponentiation(ml)
        .ok_or(ZkError::VerificationFailed(
            "batch final exponentiation failed".into(),
        ))?;

    // Valid batch → identity in GT (multiplicative identity)
    Ok(result.0 == <Bn254 as Pairing>::TargetField::ONE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuits::range_proof::RangeProofCircuit;

    #[test]
    fn batch_verify_all_valid() {
        let blinding = Fr::from(42u64);
        let value = 12345u64;
        let commitment = RangeProofCircuit::compute_commitment(value, blinding);

        let blank = RangeProofCircuit::blank(64);
        let (pk, pvk) = generate_keys(blank).unwrap();

        let proofs: Vec<_> = (0..5)
            .map(|_| {
                prove(
                    RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64),
                    &pk,
                    "range-proof",
                )
                .unwrap()
            })
            .collect();

        let proof_refs: Vec<&_> = proofs.iter().collect();
        let input_arrays: Vec<Vec<Fr>> = (0..5).map(|_| vec![commitment]).collect();
        let inputs: Vec<&[Fr]> = input_arrays.iter().map(|v| v.as_slice()).collect();
        assert!(batch_verify(&proof_refs, &pvk, &inputs).unwrap());
    }

    #[test]
    fn batch_verify_one_invalid_fails() {
        let blinding = Fr::from(42u64);
        let value = 12345u64;
        let commitment = RangeProofCircuit::compute_commitment(value, blinding);

        let blank = RangeProofCircuit::blank(64);
        let (pk, pvk) = generate_keys(blank).unwrap();

        let proofs: Vec<_> = (0..3)
            .map(|_| {
                prove(
                    RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64),
                    &pk,
                    "range-proof",
                )
                .unwrap()
            })
            .collect();

        let wrong = commitment + Fr::from(1u64);
        let proof_refs: Vec<&_> = proofs.iter().collect();
        let input_arrays: Vec<Vec<Fr>> = vec![vec![commitment], vec![wrong], vec![commitment]];
        let inputs: Vec<&[Fr]> = input_arrays.iter().map(|v| v.as_slice()).collect();
        assert!(!batch_verify(&proof_refs, &pvk, &inputs).unwrap());
    }

    #[test]
    fn batch_verify_empty_is_true() {
        let blank = RangeProofCircuit::blank(64);
        let (_, pvk) = generate_keys(blank).unwrap();
        assert!(batch_verify(&[], &pvk, &[]).unwrap());
    }

    #[test]
    fn batch_verify_single_matches_individual() {
        let blinding = Fr::from(42u64);
        let value = 100u64;
        let commitment = RangeProofCircuit::compute_commitment(value, blinding);

        let blank = RangeProofCircuit::blank(64);
        let (pk, pvk) = generate_keys(blank).unwrap();

        let proof = prove(
            RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64),
            &pk,
            "range-proof",
        )
        .unwrap();

        let single_result = verify(&proof, &pvk, &[commitment]).unwrap();
        let commitment_arr = [commitment];
        let batch_result = batch_verify(&[&proof], &pvk, &[&commitment_arr[..]]).unwrap();
        assert_eq!(single_result, batch_result);
    }
}
