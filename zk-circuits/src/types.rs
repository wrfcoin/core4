use ark_bn254::Bn254;
use ark_groth16::{PreparedVerifyingKey, ProvingKey as ArkProvingKey};
use serde::{Deserialize, Serialize};

use crate::error::ZkError;

/// Identifies a confidential-transfer circuit instantiation by its I/O count.
/// Used as a HashMap key for per-config proving/verification key caching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CircuitConfig {
    pub num_inputs: usize,
    pub num_outputs: usize,
}

impl CircuitConfig {
    pub fn new(num_inputs: usize, num_outputs: usize) -> Self {
        Self { num_inputs, num_outputs }
    }

    /// Canonical circuit_id embedded in ZkProof bytes.
    /// Example: "confidential-transfer-1x1", "confidential-transfer-2x2"
    pub fn circuit_id(&self, base: &str) -> String {
        format!("{}-{}x{}", base, self.num_inputs, self.num_outputs)
    }
}

/// Serialized ZK proof bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZkProof {
    pub proof_bytes: Vec<u8>,
    pub circuit_id: String,
}

/// Serialized verification key.
#[derive(Debug, Clone)]
pub struct VerificationKey {
    pub vk: PreparedVerifyingKey<Bn254>,
    pub circuit_id: String,
}

/// Serialized proving key.
#[derive(Debug, Clone)]
pub struct ProvingKeyWrapper {
    pub pk: ArkProvingKey<Bn254>,
    pub circuit_id: String,
}

impl ZkProof {
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ZkError> {
        serde_json::from_slice(bytes)
            .map_err(|e| ZkError::SerializationError(e.to_string()))
    }
}
