use ark_bn254::Fr;
use ark_crypto_primitives::sponge::{
    constraints::CryptographicSpongeVar,
    poseidon::constraints::PoseidonSpongeVar,
};
use ark_ff::{BigInteger, Field, PrimeField};
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::prelude::*;
use ark_relations::gr1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use super::poseidon_config::{poseidon_config_bn254, poseidon_hash};

/// Circuit proving a committed value lies in [0, 2^n_bits).
///
/// Public inputs: [commitment]  (commitment = Poseidon(value, blinding))
/// Private inputs: [value, blinding]
///
/// Constraints:
/// 1. Bit decomposition: value has at most n_bits bits
/// 2. Reconstruction: sum(bit_i * 2^i) == value
/// 3. Commitment: Poseidon(value, blinding) == commitment
#[derive(Clone)]
pub struct RangeProofCircuit {
    /// Public: commitment to the value.
    pub commitment: Option<Fr>,
    /// Private: the value to prove is in range.
    pub value: Option<Fr>,
    /// Private: blinding factor.
    pub blinding: Option<Fr>,
    /// Number of bits for the range (default 64).
    pub n_bits: usize,
}

impl RangeProofCircuit {
    pub fn new(commitment: Fr, value: Fr, blinding: Fr, n_bits: usize) -> Self {
        Self {
            commitment: Some(commitment),
            value: Some(value),
            blinding: Some(blinding),
            n_bits,
        }
    }

    pub fn blank(n_bits: usize) -> Self {
        Self {
            commitment: None,
            value: None,
            blinding: None,
            n_bits,
        }
    }

    /// Compute commitment = Poseidon(value, blinding).
    pub fn compute_commitment(value: u64, blinding: Fr) -> Fr {
        poseidon_hash(&[Fr::from(value), blinding])
    }
}

impl ConstraintSynthesizer<Fr> for RangeProofCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        let n_bits = self.n_bits;

        // Public input: commitment
        let commitment_var = FpVar::new_input(cs.clone(), || {
            self.commitment.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Private witness: value
        let value_var = FpVar::new_witness(cs.clone(), || {
            self.value.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Private witness: blinding
        let blinding_var = FpVar::new_witness(cs.clone(), || {
            self.blinding.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Bit decomposition: prove value fits in n_bits
        // Extract bits from the value
        let value_u64 = self.value.map(|v| {
            let bytes = v.into_bigint().to_bytes_le();
            u64::from_le_bytes(bytes[..8].try_into().unwrap_or([0u8; 8]))
        });

        let mut reconstructed = FpVar::zero();

        for i in 0..n_bits {
            let bit_val = value_u64.map(|v| (v >> i) & 1 == 1);
            let bit_var = Boolean::new_witness(cs.clone(), || {
                bit_val.ok_or(SynthesisError::AssignmentMissing)
            })?;

            // Precomputed constant avoids iterative FpVar multiplication dispatch
            let bit_fp = FpVar::from(bit_var);
            let power = FpVar::constant(Fr::from(2u64).pow([i as u64]));
            reconstructed += &bit_fp * &power;
        }

        // Enforce: reconstructed == value
        reconstructed.enforce_equal(&value_var)?;

        // Enforce: Poseidon(value, blinding) == commitment
        let config = poseidon_config_bn254();
        let mut sponge = PoseidonSpongeVar::new(cs, &config);
        sponge.absorb(&value_var)?;
        sponge.absorb(&blinding_var)?;
        let hash_vars = sponge
            .squeeze_field_elements(1)
            .map_err(|_| SynthesisError::Unsatisfiable)?;
        hash_vars[0].enforce_equal(&commitment_var)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proving;

    #[test]
    fn range_proof_valid_value() {
        let value = 12345u64;
        let blinding = Fr::from(999u64);
        let commitment = RangeProofCircuit::compute_commitment(value, blinding);

        let blank = RangeProofCircuit::blank(64);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        let circuit = RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64);
        let proof = proving::prove(circuit, &pk, "range-proof").unwrap();

        let result = proving::verify(&proof, &pvk, &[commitment]).unwrap();
        assert!(result, "valid range proof should verify");
    }

    #[test]
    fn range_proof_zero_value() {
        let value = 0u64;
        let blinding = Fr::from(42u64);
        let commitment = RangeProofCircuit::compute_commitment(value, blinding);

        let blank = RangeProofCircuit::blank(64);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        let circuit = RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64);
        let proof = proving::prove(circuit, &pk, "range-proof").unwrap();

        let result = proving::verify(&proof, &pvk, &[commitment]).unwrap();
        assert!(result);
    }

    #[test]
    fn range_proof_max_u64_value() {
        let value = u64::MAX;
        let blinding = Fr::from(1u64);
        let commitment = RangeProofCircuit::compute_commitment(value, blinding);

        let blank = RangeProofCircuit::blank(64);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        let circuit = RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64);
        let proof = proving::prove(circuit, &pk, "range-proof").unwrap();

        let result = proving::verify(&proof, &pvk, &[commitment]).unwrap();
        assert!(result);
    }

    #[test]
    fn range_proof_wrong_commitment_fails() {
        let value = 100u64;
        let blinding = Fr::from(50u64);
        let commitment = RangeProofCircuit::compute_commitment(value, blinding);

        let blank = RangeProofCircuit::blank(64);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        let circuit = RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64);
        let proof = proving::prove(circuit, &pk, "range-proof").unwrap();

        let wrong = commitment + Fr::from(1u64);
        let result = proving::verify(&proof, &pvk, &[wrong]).unwrap();
        assert!(!result);
    }

    #[test]
    fn range_proof_32_bit_valid() {
        let value = 12345u32;
        let blinding = Fr::from(999u64);
        let commitment = RangeProofCircuit::compute_commitment(value as u64, blinding);

        let blank = RangeProofCircuit::blank(32);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        let circuit = RangeProofCircuit::new(commitment, Fr::from(value as u64), blinding, 32);
        let proof = proving::prove(circuit, &pk, "range-proof-32").unwrap();

        let result = proving::verify(&proof, &pvk, &[commitment]).unwrap();
        assert!(result, "32-bit range proof should verify");
    }

    #[test]
    fn range_proof_32_bit_max_value() {
        let value = u32::MAX;
        let blinding = Fr::from(1u64);
        let commitment = RangeProofCircuit::compute_commitment(value as u64, blinding);

        let blank = RangeProofCircuit::blank(32);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        let circuit = RangeProofCircuit::new(commitment, Fr::from(value as u64), blinding, 32);
        let proof = proving::prove(circuit, &pk, "range-proof-32").unwrap();

        let result = proving::verify(&proof, &pvk, &[commitment]).unwrap();
        assert!(result);
    }

    #[test]
    fn commitment_is_not_linear() {
        // Poseidon is NOT homomorphic: H(v1, b1) + H(v2, b2) != H(v1+v2, b1+b2)
        let c1 = RangeProofCircuit::compute_commitment(10, Fr::from(5u64));
        let c2 = RangeProofCircuit::compute_commitment(20, Fr::from(7u64));
        let c_sum = RangeProofCircuit::compute_commitment(30, Fr::from(12u64));
        assert_ne!(
            c1 + c2,
            c_sum,
            "Poseidon commitment must not be homomorphic (linear)"
        );
    }
}
