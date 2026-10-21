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

/// Circuit proving a confidential transfer is balanced:
/// sum(inputs) == sum(outputs) + fee, with all values non-negative (< 2^64)
/// and blinding factors conserved across inputs and outputs.
///
/// Public inputs: [input_commitment, output_commitment, fee]
/// Private inputs: [input_values..., input_blindings..., output_values..., output_blindings...]
///
/// Constraints:
/// 1. Range: each value fits in n_bits
/// 2. Commitment opening: input_commitment == Poseidon(sum(input_values), sum(input_blindings))
/// 3. Commitment opening: output_commitment == Poseidon(sum(output_values), sum(output_blindings))
/// 4. Value balance: sum(input_values) == sum(output_values) + fee
/// 5. Blinding conservation: sum(input_blindings) == sum(output_blindings)
#[derive(Clone)]
pub struct ConfidentialTransferCircuit {
    /// Public: commitment to sum of inputs.
    pub input_commitment: Option<Fr>,
    /// Public: commitment to sum of outputs.
    pub output_commitment: Option<Fr>,
    /// Public: the fee amount (transparent).
    pub fee: Option<Fr>,
    /// Private: input amounts.
    pub input_values: Vec<Option<Fr>>,
    /// Private: input blinding factors.
    pub input_blindings: Vec<Option<Fr>>,
    /// Private: output amounts.
    pub output_values: Vec<Option<Fr>>,
    /// Private: output blinding factors.
    pub output_blindings: Vec<Option<Fr>>,
    /// Bits for range checks.
    pub n_bits: usize,
}

impl ConfidentialTransferCircuit {
    pub fn new(
        input_commitment: Fr,
        output_commitment: Fr,
        fee: Fr,
        input_values: Vec<Fr>,
        input_blindings: Vec<Fr>,
        output_values: Vec<Fr>,
        output_blindings: Vec<Fr>,
        n_bits: usize,
    ) -> Self {
        Self {
            input_commitment: Some(input_commitment),
            output_commitment: Some(output_commitment),
            fee: Some(fee),
            input_values: input_values.into_iter().map(Some).collect(),
            input_blindings: input_blindings.into_iter().map(Some).collect(),
            output_values: output_values.into_iter().map(Some).collect(),
            output_blindings: output_blindings.into_iter().map(Some).collect(),
            n_bits,
        }
    }

    pub fn blank(num_inputs: usize, num_outputs: usize, n_bits: usize) -> Self {
        Self {
            input_commitment: None,
            output_commitment: None,
            fee: None,
            input_values: vec![None; num_inputs],
            input_blindings: vec![None; num_inputs],
            output_values: vec![None; num_outputs],
            output_blindings: vec![None; num_outputs],
            n_bits,
        }
    }

    /// Compute commitment = Poseidon(sum(values), sum(blindings)).
    pub fn compute_commitment(values: &[Fr], blindings: &[Fr]) -> Fr {
        let value_sum: Fr = values.iter().sum();
        let blinding_sum: Fr = blindings.iter().sum();
        poseidon_hash(&[value_sum, blinding_sum])
    }
}

/// Helper: enforce that a value fits in n_bits via bit decomposition.
fn enforce_range(
    cs: ConstraintSystemRef<Fr>,
    value: &FpVar<Fr>,
    value_raw: Option<Fr>,
    n_bits: usize,
) -> Result<(), SynthesisError> {
    let value_u64 = value_raw.map(|v| {
        let bytes = v.into_bigint().to_bytes_le();
        u64::from_le_bytes(bytes[..8].try_into().unwrap_or([0u8; 8]))
    });

    let mut reconstructed = FpVar::zero();

    for i in 0..n_bits {
        let bit_val = value_u64.map(|v| (v >> i) & 1 == 1);
        let bit_var = Boolean::new_witness(cs.clone(), || {
            bit_val.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let bit_fp = FpVar::from(bit_var);
        let power = FpVar::constant(Fr::from(2u64).pow([i as u64]));
        reconstructed += &bit_fp * &power;
    }

    reconstructed.enforce_equal(value)?;
    Ok(())
}

impl ConstraintSynthesizer<Fr> for ConfidentialTransferCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // Public inputs
        let input_comm_var = FpVar::new_input(cs.clone(), || {
            self.input_commitment.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let output_comm_var = FpVar::new_input(cs.clone(), || {
            self.output_commitment.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let fee_var = FpVar::new_input(cs.clone(), || {
            self.fee.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Input values and blindings
        let mut input_sum = FpVar::zero();
        let mut input_blinding_sum = FpVar::zero();
        for i in 0..self.input_values.len() {
            let val = FpVar::new_witness(cs.clone(), || {
                self.input_values[i].ok_or(SynthesisError::AssignmentMissing)
            })?;
            let blind = FpVar::new_witness(cs.clone(), || {
                self.input_blindings[i].ok_or(SynthesisError::AssignmentMissing)
            })?;
            enforce_range(cs.clone(), &val, self.input_values[i], self.n_bits)?;
            input_sum += &val;
            input_blinding_sum += &blind;
        }

        // Output values and blindings
        let mut output_sum = FpVar::zero();
        let mut output_blinding_sum = FpVar::zero();
        for i in 0..self.output_values.len() {
            let val = FpVar::new_witness(cs.clone(), || {
                self.output_values[i].ok_or(SynthesisError::AssignmentMissing)
            })?;
            let blind = FpVar::new_witness(cs.clone(), || {
                self.output_blindings[i].ok_or(SynthesisError::AssignmentMissing)
            })?;
            enforce_range(cs.clone(), &val, self.output_values[i], self.n_bits)?;
            output_sum += &val;
            output_blinding_sum += &blind;
        }

        // Verify input commitment: Poseidon(input_sum, input_blinding_sum) == input_commitment
        let config = poseidon_config_bn254();
        let mut input_sponge = PoseidonSpongeVar::new(cs.clone(), &config);
        input_sponge.absorb(&input_sum)?;
        input_sponge.absorb(&input_blinding_sum)?;
        let input_hash = input_sponge
            .squeeze_field_elements(1)
            .map_err(|_| SynthesisError::Unsatisfiable)?;
        input_hash[0].enforce_equal(&input_comm_var)?;

        // Verify output commitment: Poseidon(output_sum, output_blinding_sum) == output_commitment
        let mut output_sponge = PoseidonSpongeVar::new(cs, &config);
        output_sponge.absorb(&output_sum)?;
        output_sponge.absorb(&output_blinding_sum)?;
        let output_hash = output_sponge
            .squeeze_field_elements(1)
            .map_err(|_| SynthesisError::Unsatisfiable)?;
        output_hash[0].enforce_equal(&output_comm_var)?;

        // Balance constraint: input_sum == output_sum + fee
        let balance = &output_sum + &fee_var;
        input_sum.enforce_equal(&balance)?;

        // Blinding factor conservation: input_blinding_sum == output_blinding_sum
        // Without this constraint, a prover could use mismatched blinding factors
        // that satisfy commitments individually but break homomorphic balance
        // verification, potentially enabling value creation.
        input_blinding_sum.enforce_equal(&output_blinding_sum)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proving;

    #[test]
    fn confidential_transfer_balanced() {
        // Use matching blinding factors (required by blinding conservation)
        let input_vals = vec![Fr::from(100u64)];
        let input_blinds = vec![Fr::from(42u64)];
        let output_vals = vec![Fr::from(90u64)];
        let output_blinds = vec![Fr::from(42u64)]; // must match input blindings sum
        let fee = Fr::from(10u64);

        let input_comm = ConfidentialTransferCircuit::compute_commitment(&input_vals, &input_blinds);
        let output_comm = ConfidentialTransferCircuit::compute_commitment(&output_vals, &output_blinds);

        let blank = ConfidentialTransferCircuit::blank(1, 1, 64);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        let circuit = ConfidentialTransferCircuit::new(
            input_comm, output_comm, fee,
            input_vals, input_blinds,
            output_vals, output_blinds,
            64,
        );
        let proof = proving::prove(circuit, &pk, "confidential-transfer").unwrap();

        let result = proving::verify(&proof, &pvk, &[input_comm, output_comm, fee]).unwrap();
        assert!(result, "balanced transfer should verify");
    }

    #[test]
    fn confidential_transfer_unbalanced_fails() {
        let input_vals = vec![Fr::from(100u64)];
        let input_blinds = vec![Fr::from(42u64)];
        let output_vals = vec![Fr::from(90u64)];
        let output_blinds = vec![Fr::from(42u64)];
        let fee = Fr::from(10u64);

        let input_comm = ConfidentialTransferCircuit::compute_commitment(&input_vals, &input_blinds);
        let output_comm = ConfidentialTransferCircuit::compute_commitment(&output_vals, &output_blinds);

        let blank = ConfidentialTransferCircuit::blank(1, 1, 64);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        let circuit = ConfidentialTransferCircuit::new(
            input_comm, output_comm, fee,
            input_vals, input_blinds,
            output_vals, output_blinds,
            64,
        );
        let proof = proving::prove(circuit, &pk, "confidential-transfer").unwrap();

        // Wrong fee
        let wrong_fee = Fr::from(5u64);
        let result = proving::verify(&proof, &pvk, &[input_comm, output_comm, wrong_fee]).unwrap();
        assert!(!result, "unbalanced transfer should fail");
    }

    #[test]
    fn confidential_transfer_multiple_io() {
        // Multiple inputs/outputs with conserved blinding factors
        let input_vals = vec![Fr::from(60u64), Fr::from(40u64)];
        let input_blinds = vec![Fr::from(10u64), Fr::from(20u64)]; // sum = 30
        let output_vals = vec![Fr::from(50u64), Fr::from(45u64)];
        let output_blinds = vec![Fr::from(15u64), Fr::from(15u64)]; // sum = 30
        let fee = Fr::from(5u64);

        let input_comm = ConfidentialTransferCircuit::compute_commitment(&input_vals, &input_blinds);
        let output_comm = ConfidentialTransferCircuit::compute_commitment(&output_vals, &output_blinds);

        let blank = ConfidentialTransferCircuit::blank(2, 2, 64);
        let (pk, pvk) = proving::generate_keys(blank).unwrap();

        let circuit = ConfidentialTransferCircuit::new(
            input_comm, output_comm, fee,
            input_vals, input_blinds,
            output_vals, output_blinds,
            64,
        );
        let proof = proving::prove(circuit, &pk, "confidential-transfer").unwrap();

        let result = proving::verify(&proof, &pvk, &[input_comm, output_comm, fee]).unwrap();
        assert!(result);
    }

    #[test]
    fn mismatched_blinding_factors_rejected() {
        // This test validates the fix for issue #786: without the blinding
        // conservation constraint, this circuit would produce a valid proof
        // despite using different blinding sums for inputs and outputs.
        //
        // With the constraint in place, ark-groth16's prover panics on
        // unsatisfied constraint systems (assertion in prover.rs), so we
        // catch the panic to confirm rejection.
        let result = std::panic::catch_unwind(|| {
            let input_vals = vec![Fr::from(100u64)];
            let input_blinds = vec![Fr::from(42u64)];
            let output_vals = vec![Fr::from(90u64)];
            let output_blinds = vec![Fr::from(99u64)]; // DIFFERENT from input blindings
            let fee = Fr::from(10u64);

            let input_comm =
                ConfidentialTransferCircuit::compute_commitment(&input_vals, &input_blinds);
            let output_comm =
                ConfidentialTransferCircuit::compute_commitment(&output_vals, &output_blinds);

            let blank = ConfidentialTransferCircuit::blank(1, 1, 64);
            let (pk, _pvk) = proving::generate_keys(blank).unwrap();

            let circuit = ConfidentialTransferCircuit::new(
                input_comm, output_comm, fee,
                input_vals, input_blinds,
                output_vals, output_blinds,
                64,
            );

            proving::prove(circuit, &pk, "confidential-transfer")
        });

        assert!(result.is_err(), "mismatched blinding factors must be rejected");
    }

    #[test]
    fn commitment_is_not_linear() {
        // Poseidon is NOT homomorphic: C(v1,b1) + C(v2,b2) != C(v1+v2, b1+b2)
        let c1 = ConfidentialTransferCircuit::compute_commitment(
            &[Fr::from(10u64)],
            &[Fr::from(5u64)],
        );
        let c2 = ConfidentialTransferCircuit::compute_commitment(
            &[Fr::from(20u64)],
            &[Fr::from(7u64)],
        );
        let c_sum = ConfidentialTransferCircuit::compute_commitment(
            &[Fr::from(30u64)],
            &[Fr::from(12u64)],
        );
        assert_ne!(
            c1 + c2,
            c_sum,
            "Poseidon commitment must not be homomorphic (linear)"
        );
    }
}
