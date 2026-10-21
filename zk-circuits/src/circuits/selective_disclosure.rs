use ark_bn254::Fr;
use ark_crypto_primitives::sponge::{
    constraints::CryptographicSpongeVar,
    poseidon::constraints::PoseidonSpongeVar,
};
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::prelude::*;
use ark_relations::gr1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use super::poseidon_config::{poseidon_config_bn254, poseidon_hash};

/// Circuit proving knowledge of preimage fields whose Poseidon hash
/// matches a public commitment.
///
/// Public inputs: [commitment]
/// Private inputs: [field_0, field_1, ..., field_{n-1}]
///
/// Constraint: Poseidon(field_0 || field_1 || ... || field_{n-1}) == commitment
#[derive(Clone)]
pub struct SelectiveDisclosureCircuit {
    /// Public: the Poseidon hash commitment of the disclosed fields.
    pub commitment: Option<Fr>,
    /// Private: the actual field values.
    pub fields: Vec<Option<Fr>>,
}

impl SelectiveDisclosureCircuit {
    pub fn new(commitment: Fr, fields: Vec<Fr>) -> Self {
        Self {
            commitment: Some(commitment),
            fields: fields.into_iter().map(Some).collect(),
        }
    }

    /// Create a blank circuit for key generation (no witness).
    pub fn blank(num_fields: usize) -> Self {
        Self {
            commitment: None,
            fields: vec![None; num_fields],
        }
    }

    /// Compute the Poseidon commitment for given field values.
    pub fn compute_commitment(fields: &[Fr]) -> Fr {
        poseidon_hash(fields)
    }
}

impl ConstraintSynthesizer<Fr> for SelectiveDisclosureCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // Allocate commitment as public input
        let commitment_var = FpVar::new_input(cs.clone(), || {
            self.commitment.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Allocate field values as private witnesses
        let mut field_vars = Vec::with_capacity(self.fields.len());
        for field_val in &self.fields {
            field_vars.push(FpVar::new_witness(cs.clone(), || {
                field_val.ok_or(SynthesisError::AssignmentMissing)
            })?);
        }

        // Compute Poseidon hash in-circuit
        let config = poseidon_config_bn254();
        let mut sponge = PoseidonSpongeVar::new(cs, &config);
        sponge.absorb(&field_vars)?;
        let hash_vars = sponge
            .squeeze_field_elements(1)
            .map_err(|_| SynthesisError::Unsatisfiable)?;

        // Enforce: Poseidon(fields) == commitment
        hash_vars[0].enforce_equal(&commitment_var)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proving;

    #[test]
    fn selective_disclosure_proof_roundtrip() {
        let fields = vec![Fr::from(42u64), Fr::from(100u64), Fr::from(7u64)];
        let commitment = SelectiveDisclosureCircuit::compute_commitment(&fields);

        // Key generation with blank circuit
        let blank = SelectiveDisclosureCircuit::blank(3);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        // Prove with actual witness
        let circuit = SelectiveDisclosureCircuit::new(commitment, fields);
        let proof = proving::prove(circuit, &pk, "selective-disclosure").unwrap();

        // Verify
        let result = proving::verify(&proof, &pvk, &[commitment]).unwrap();
        assert!(result, "valid proof should verify");
    }

    #[test]
    fn selective_disclosure_rejects_wrong_commitment() {
        let fields = vec![Fr::from(42u64), Fr::from(100u64)];
        let commitment = SelectiveDisclosureCircuit::compute_commitment(&fields);

        let blank = SelectiveDisclosureCircuit::blank(2);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        let circuit = SelectiveDisclosureCircuit::new(commitment, fields);
        let proof = proving::prove(circuit, &pk, "selective-disclosure").unwrap();

        // Verify with wrong commitment should fail
        let wrong_commitment = commitment + Fr::from(1u64);
        let result = proving::verify(&proof, &pvk, &[wrong_commitment]).unwrap();
        assert!(!result, "wrong commitment should not verify");
    }

    #[test]
    fn commitment_is_binding() {
        // Changing any single field must change the commitment — i.e., it is
        // not trivially invertible like the old linear algebraic "hash".
        let base = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];
        let base_commitment = SelectiveDisclosureCircuit::compute_commitment(&base);

        for i in 0..base.len() {
            let mut modified = base.clone();
            modified[i] = modified[i] + Fr::from(1u64);
            let modified_commitment = SelectiveDisclosureCircuit::compute_commitment(&modified);
            assert_ne!(
                base_commitment, modified_commitment,
                "changing field {i} must change the commitment"
            );
        }
    }
}
