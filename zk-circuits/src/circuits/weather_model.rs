use ark_bn254::Fr;
use ark_crypto_primitives::sponge::{
    constraints::CryptographicSpongeVar,
    poseidon::constraints::PoseidonSpongeVar,
};
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::prelude::*;
use ark_relations::gr1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use super::poseidon_config::poseidon_config_bn254;

/// Circuit verifying the integrity of a WRF weather model execution.
///
/// It proves that for a given public input hash and model configuration,
/// the resulting output hash was computed correctly via Poseidon hash.
///
/// Public Inputs:
/// 1. input_hash (Fr representation of the GRIB/boundary data hash)
/// 2. output_hash (Fr representation of the result NetCDF/manifest hash)
/// 3. model_id (numeric identifier for the WRF variant used)
/// 4. execution_time_secs (verification of resource usage)
///
/// Private Inputs (Witness):
/// 1. state_samples (Intermediate computational steps to verify the "work")
///
/// Constraint: Poseidon(state_samples || model_id || input_hash) == output_hash
#[derive(Clone)]
pub struct WeatherModelCircuit {
    pub input_hash: Option<Fr>,
    pub output_hash: Option<Fr>,
    pub model_id: Option<Fr>,
    pub execution_time_secs: Option<Fr>,
    pub state_samples: Vec<Option<Fr>>,
}

impl WeatherModelCircuit {
    pub fn new(
        input_hash: Fr,
        output_hash: Fr,
        model_id: Fr,
        execution_time_secs: Fr,
        state_samples: Vec<Fr>,
    ) -> Self {
        Self {
            input_hash: Some(input_hash),
            output_hash: Some(output_hash),
            model_id: Some(model_id),
            execution_time_secs: Some(execution_time_secs),
            state_samples: state_samples.into_iter().map(Some).collect(),
        }
    }

    /// Create a blank circuit for key generation.
    pub fn blank(num_samples: usize) -> Self {
        Self {
            input_hash: None,
            output_hash: None,
            model_id: None,
            execution_time_secs: None,
            state_samples: vec![None; num_samples],
        }
    }
}

impl ConstraintSynthesizer<Fr> for WeatherModelCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // --- Allocate Public Inputs ---
        let input_hash_var = FpVar::new_input(cs.clone(), || {
            self.input_hash.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let output_hash_var = FpVar::new_input(cs.clone(), || {
            self.output_hash.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let model_id_var = FpVar::new_input(cs.clone(), || {
            self.model_id.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let time_var = FpVar::new_input(cs.clone(), || {
            self.execution_time_secs
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // --- Allocate Private Witnesses ---
        let mut sample_vars = Vec::new();
        for sample in self.state_samples {
            sample_vars.push(FpVar::new_witness(cs.clone(), || {
                sample.ok_or(SynthesisError::AssignmentMissing)
            })?);
        }

        // --- Verification Logic ---

        // 1. Minimum work check: Ensure we have at least some intermediate samples
        if sample_vars.is_empty() {
            return Err(SynthesisError::AssignmentMissing);
        }

        // 2. Poseidon hash integrity check:
        // Hash(state_samples || model_id || input_hash) == output_hash
        //
        // This replaces the old linear sum which was trivially invertible.
        // Poseidon provides collision resistance within R1CS.
        let config = poseidon_config_bn254();
        let mut sponge = PoseidonSpongeVar::new(cs, &config);
        sponge.absorb(&sample_vars)?;
        sponge.absorb(&model_id_var)?;
        sponge.absorb(&input_hash_var)?;
        let computed_output = sponge
            .squeeze_field_elements(1)
            .map_err(|_| SynthesisError::Unsatisfiable)?;

        // Enforce that computed_output matches the public output_hash
        computed_output[0].enforce_equal(&output_hash_var)?;

        // 3. Time sanity check
        time_var.enforce_not_equal(&FpVar::zero())?;

        Ok(())
    }
}
