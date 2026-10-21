use crate::crypto::hash::Hash;
use anyhow::Result;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Zero-Knowledge proof for state transitions
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZKProof {
    /// Proof data
    pub proof_data: Vec<u8>,

    /// Public inputs
    pub public_inputs: Vec<u8>,

    /// Verification key
    pub verification_key: Vec<u8>,
}

/// Domain-separated deterministic digest for transitional proof verification.
/// This is not a substitute for a full proof system, but it prevents accepting
/// arbitrary opaque proof bytes.
#[allow(dead_code)]
pub fn derive_proof_digest(public_inputs: &[u8], verification_key: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"wrfcoin.zkproof.v1");
    hasher.update((public_inputs.len() as u64).to_be_bytes());
    hasher.update(public_inputs);
    hasher.update((verification_key.len() as u64).to_be_bytes());
    hasher.update(verification_key);
    let mut out = [0u8; 32];
    out.copy_from_slice(&hasher.finalize());
    out
}

#[allow(dead_code)]
impl ZKProof {
    /// Create a new ZK proof
    pub fn new(proof_data: Vec<u8>, public_inputs: Vec<u8>, verification_key: Vec<u8>) -> Self {
        Self {
            proof_data,
            public_inputs,
            verification_key,
        }
    }

    /// Serialize the proof to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::new();

        let proof_len = self.proof_data.len() as u32;
        result.extend_from_slice(&proof_len.to_be_bytes());
        result.extend_from_slice(&self.proof_data);

        let inputs_len = self.public_inputs.len() as u32;
        result.extend_from_slice(&inputs_len.to_be_bytes());
        result.extend_from_slice(&self.public_inputs);

        let key_len = self.verification_key.len() as u32;
        result.extend_from_slice(&key_len.to_be_bytes());
        result.extend_from_slice(&self.verification_key);

        result
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 12 {
            return Err(anyhow::anyhow!("Proof data too short"));
        }

        let mut offset = 0;

        let proof_len = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        if offset + proof_len > bytes.len() {
            return Err(anyhow::anyhow!("Proof data truncated"));
        }

        let proof_data = bytes[offset..offset + proof_len].to_vec();
        offset += proof_len;

        if offset + 4 > bytes.len() {
            return Err(anyhow::anyhow!("Public inputs length missing"));
        }

        let inputs_len = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        if offset + inputs_len > bytes.len() {
            return Err(anyhow::anyhow!("Public inputs truncated"));
        }

        let public_inputs = bytes[offset..offset + inputs_len].to_vec();
        offset += inputs_len;

        if offset + 4 > bytes.len() {
            return Err(anyhow::anyhow!("Verification key length missing"));
        }

        let key_len = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        if offset + key_len > bytes.len() {
            return Err(anyhow::anyhow!("Verification key truncated"));
        }

        let verification_key = bytes[offset..offset + key_len].to_vec();

        Ok(Self {
            proof_data,
            public_inputs,
            verification_key,
        })
    }
}

/// Types of proof systems
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofSystem {
    /// Groth16
    Groth16,

    /// PLONK
    Plonk,

    /// Bulletproofs
    Bulletproofs,

    /// Halo2
    Halo2,

    /// Custom proof system
    Custom,
}

/// Proof generation metrics
#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ZKProofMetrics {
    /// Number of proofs generated
    pub proofs_generated: u64,

    /// Total generation time in milliseconds
    pub total_generation_time_ms: u64,

    /// Average generation time in milliseconds
    pub avg_generation_time_ms: f64,

    /// Average proof size in bytes
    pub avg_proof_size: f64,

    /// Last generation timestamp (unix seconds)
    pub last_generation: Option<u64>,
}

/// Zero-Knowledge proof generator
#[allow(dead_code)]
pub struct ZKGenerator {
    /// Proof system being used
    proof_system: ProofSystem,

    /// Cached verification keys
    verification_keys: HashMap<String, Vec<u8>>,

    /// Generator metrics
    metrics: ZKProofMetrics,
}

#[allow(dead_code)]
impl ZKGenerator {
    /// Create a new ZK generator
    pub fn new(proof_system: ProofSystem) -> Self {
        Self {
            proof_system,
            verification_keys: HashMap::new(),
            metrics: ZKProofMetrics::default(),
        }
    }

    /// Generate a proof
    pub async fn generate_proof(
        &mut self,
        circuit_id: &str,
        public_inputs: Vec<u8>,
        private_inputs: Vec<u8>,
    ) -> Result<ZKProof> {
        let start_time = std::time::Instant::now();

        let verification_key = self.get_or_generate_verification_key(circuit_id).await?;

        let delay = (public_inputs.len() + private_inputs.len()) as u64 / 1024;
        tokio::time::sleep(tokio::time::Duration::from_millis(10 + delay)).await;

        let private_commitment = Sha256::digest(&private_inputs);
        let mut expanded_inputs = public_inputs.clone();
        expanded_inputs.extend_from_slice(&private_commitment);

        let proof_data = derive_proof_digest(&expanded_inputs, &verification_key).to_vec();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let proof = ZKProof::new(proof_data, expanded_inputs, verification_key.clone());

        let generation_time = start_time.elapsed().as_millis() as u64;
        self.metrics.proofs_generated += 1;
        self.metrics.total_generation_time_ms += generation_time;
        self.metrics.avg_generation_time_ms =
            self.metrics.total_generation_time_ms as f64 / self.metrics.proofs_generated as f64;
        self.metrics.avg_proof_size = (self.metrics.avg_proof_size
            * (self.metrics.proofs_generated - 1) as f64
            + proof.to_bytes().len() as f64)
            / self.metrics.proofs_generated as f64;
        self.metrics.last_generation = Some(timestamp);

        info!(
            "Generated ZK proof for circuit {} in {} ms",
            circuit_id, generation_time
        );

        Ok(proof)
    }

    /// Get or generate verification key
    async fn get_or_generate_verification_key(&mut self, circuit_id: &str) -> Result<Vec<u8>> {
        if let Some(key) = self.verification_keys.get(circuit_id) {
            return Ok(key.clone());
        }

        let mut key = Vec::with_capacity(64);
        key.extend_from_slice(circuit_id.as_bytes());

        for i in 0..32 {
            key.push((i % 256) as u8);
        }

        self.verification_keys
            .insert(circuit_id.to_string(), key.clone());

        Ok(key)
    }

    /// Get metrics
    pub fn get_metrics(&self) -> &ZKProofMetrics {
        &self.metrics
    }
}

/// Proof verification metrics
#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ZKVerificationMetrics {
    /// Number of proofs verified
    pub proofs_verified: u64,

    /// Number of successful verifications
    pub successful_verifications: u64,

    /// Number of failed verifications
    pub failed_verifications: u64,

    /// Total verification time in milliseconds
    pub total_verification_time_ms: u64,

    /// Average verification time in milliseconds
    pub avg_verification_time_ms: f64,

    /// Last verification timestamp (unix seconds)
    pub last_verification: Option<u64>,
}

/// Zero-Knowledge proof verifier
#[allow(dead_code)]
pub struct ZKVerifier {
    /// Cached verification keys
    verification_keys: Arc<RwLock<HashMap<String, Vec<u8>>>>,

    /// Verifier metrics
    metrics: Arc<RwLock<ZKVerificationMetrics>>,
}

#[allow(dead_code)]
impl ZKVerifier {
    /// Create a new ZK verifier
    pub fn new() -> Self {
        Self {
            verification_keys: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(ZKVerificationMetrics::default())),
        }
    }

    /// Verify a proof using from/to state hashes
    pub async fn verify(&self, proof: &ZKProof, from_state: Hash, to_state: Hash) -> Result<bool> {
        let start_time = std::time::Instant::now();

        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

        let has_min_inputs = proof.public_inputs.len() >= 64;
        let roots_match = has_min_inputs
            && proof.public_inputs[0..32] == *from_state.as_bytes()
            && proof.public_inputs[32..64] == *to_state.as_bytes();
        let digest_matches = proof.proof_data
            == derive_proof_digest(&proof.public_inputs, &proof.verification_key).to_vec();
        let is_valid =
            has_min_inputs && roots_match && !proof.verification_key.is_empty() && digest_matches;

        let verification_time = start_time.elapsed().as_millis() as u64;
        let mut metrics = self.metrics.write().await;
        metrics.proofs_verified += 1;
        if is_valid {
            metrics.successful_verifications += 1;
        } else {
            metrics.failed_verifications += 1;
        }
        metrics.total_verification_time_ms += verification_time;
        metrics.avg_verification_time_ms =
            metrics.total_verification_time_ms as f64 / metrics.proofs_verified as f64;
        metrics.last_verification = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );

        debug!(
            "Verified ZK proof in {} ms, result: {}",
            verification_time, is_valid
        );

        Ok(is_valid)
    }

    /// Get metrics
    pub async fn get_metrics(&self) -> ZKVerificationMetrics {
        self.metrics.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hash::Hash;

    #[test]
    fn derive_proof_digest_is_deterministic_and_input_sensitive() {
        let inputs = b"public_inputs";
        let key = b"verification_key";

        let d1 = derive_proof_digest(inputs, key);
        let d2 = derive_proof_digest(inputs, key);
        assert_eq!(d1, d2, "digest must be deterministic");

        let d3 = derive_proof_digest(b"different_inputs", key);
        assert_ne!(d1, d3, "different inputs must produce different digest");
    }

    #[test]
    fn zk_proof_serializes_and_deserializes_roundtrip() {
        let proof = ZKProof::new(
            vec![0x01, 0x02, 0x03],
            vec![0x04, 0x05],
            vec![0x06, 0x07, 0x08, 0x09],
        );

        let bytes = proof.to_bytes();
        let recovered = ZKProof::from_bytes(&bytes).expect("deserialize");
        assert_eq!(proof.proof_data, recovered.proof_data);
        assert_eq!(proof.public_inputs, recovered.public_inputs);
        assert_eq!(proof.verification_key, recovered.verification_key);

        // Truncated bytes must fail cleanly
        assert!(ZKProof::from_bytes(&bytes[..5]).is_err());
    }

    #[tokio::test]
    async fn zk_verifier_accepts_correctly_constructed_proof_and_rejects_tampered_inputs() {
        let from_state = Hash::from_bytes([0x11u8; 32]);
        let to_state = Hash::from_bytes([0x22u8; 32]);

        // Build a valid proof: public_inputs = from_state || to_state,
        // proof_data = derive_proof_digest(public_inputs, key)
        let mut public_inputs = Vec::with_capacity(64);
        public_inputs.extend_from_slice(from_state.as_bytes());
        public_inputs.extend_from_slice(to_state.as_bytes());
        let verification_key = vec![0xabu8; 16];
        let proof_data = derive_proof_digest(&public_inputs, &verification_key).to_vec();

        let valid_proof = ZKProof::new(proof_data.clone(), public_inputs.clone(), verification_key.clone());
        let verifier = ZKVerifier::new();
        assert!(
            verifier.verify(&valid_proof, from_state, to_state).await.unwrap(),
            "valid proof must verify"
        );

        // Tamper: swap from/to state in public_inputs
        let mut bad_inputs = Vec::with_capacity(64);
        bad_inputs.extend_from_slice(to_state.as_bytes()); // swapped
        bad_inputs.extend_from_slice(from_state.as_bytes());
        let bad_proof_data = derive_proof_digest(&bad_inputs, &verification_key).to_vec();
        let tampered = ZKProof::new(bad_proof_data, bad_inputs, verification_key);
        assert!(
            !verifier.verify(&tampered, from_state, to_state).await.unwrap(),
            "tampered proof must fail"
        );
    }
}
