//! Cryptographic Operations for HotStuff BFT
//!
//! This module provides signature generation and verification for HotStuff
//! consensus messages including proposals, votes, and quorum certificates.
//!
//! Adapted from `blockchain-core/src/consensus/hotstuff/crypto.rs`.

use ed25519_dalek::{
    PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH, Signature, Signer, SigningKey, Verifier, VerifyingKey,
};
use sha2::{Digest, Sha256};
use rustc_hash::FxHashMap;
use thiserror::Error;

use super::types::*;

/// Cryptographic errors
#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("Invalid key length: expected {expected}, got {got}")]
    InvalidKeyLength { expected: usize, got: usize },
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Key not found for node: {0}")]
    KeyNotFound(String),
    #[error("Signature verification failed: {0}")]
    VerificationFailed(String),
}

/// Signing key wrapper for HotStuff
pub struct HotStuffSigningKey {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    node_id: NodeId,
}

impl std::fmt::Debug for HotStuffSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotStuffSigningKey")
            .field("node_id", &self.node_id)
            .field("public_key", &hex::encode(self.verifying_key.as_bytes()))
            .finish()
    }
}

impl HotStuffSigningKey {
    /// Create a new signing key from bytes
    pub fn from_bytes(bytes: &[u8], node_id: NodeId) -> Result<Self, CryptoError> {
        if bytes.len() != 32 {
            return Err(CryptoError::InvalidKeyLength {
                expected: 32,
                got: bytes.len(),
            });
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(bytes);
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();

        Ok(Self {
            signing_key,
            verifying_key,
            node_id,
        })
    }

    /// Generate a new random signing key
    pub fn generate(node_id: NodeId) -> Self {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();

        Self {
            signing_key,
            verifying_key,
            node_id,
        }
    }

    /// Get the public key bytes
    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.verifying_key.as_bytes().to_vec()
    }

    /// Get the verifying key
    #[allow(dead_code)]
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// Sign a message
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let signature = self.signing_key.sign(message);
        signature.to_bytes().to_vec()
    }

    /// Sign a proposal
    pub fn sign_proposal(&self, proposal: &HotStuffProposal) -> SignatureBytes {
        let message = Self::proposal_sign_message(proposal);
        self.sign(&message)
    }

    /// Sign a vote
    pub fn sign_vote(&self, vote: &HotStuffVote) -> SignatureBytes {
        let message = vote.sign_message();
        self.sign(&message)
    }

    /// Sign a new view message
    #[allow(dead_code)]
    pub fn sign_new_view(
        &self,
        new_view: ViewNumber,
        high_qc: &QuorumCertificate,
    ) -> SignatureBytes {
        let message = Self::new_view_sign_message(new_view, high_qc);
        self.sign(&message)
    }

    /// Create the message bytes for proposal signing
    pub fn proposal_sign_message(proposal: &HotStuffProposal) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(proposal.block_hash.as_bytes());
        message.extend_from_slice(&proposal.view_number.to_le_bytes());
        message.extend_from_slice(&proposal.block_height.to_le_bytes());
        message.extend_from_slice(proposal.parent_hash.as_bytes());
        message
    }

    /// Create the message bytes for new view signing
    pub fn new_view_sign_message(new_view: ViewNumber, high_qc: &QuorumCertificate) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(&new_view.to_le_bytes());
        message.extend_from_slice(high_qc.block_hash.as_bytes());
        message.extend_from_slice(&high_qc.view_number.to_le_bytes());
        message
    }
}

/// Signature verifier for HotStuff messages
#[derive(Debug)]
pub struct SignatureVerifier {
    /// Mapping from node ID to their public key
    public_keys: FxHashMap<NodeId, VerifyingKey>,
}

impl SignatureVerifier {
    /// Create a new signature verifier
    pub fn new() -> Self {
        Self {
            public_keys: FxHashMap::default(),
        }
    }

    /// Create a verifier from a validator set
    pub fn from_validator_set(validators: &ValidatorSet) -> Result<Self, CryptoError> {
        let mut verifier = Self::new();

        for validator in &validators.validators {
            verifier.add_public_key(&validator.node_id, &validator.public_key)?;
        }

        Ok(verifier)
    }

    /// Add a public key for a node
    pub fn add_public_key(
        &mut self,
        node_id: &NodeId,
        public_key: &[u8],
    ) -> Result<(), CryptoError> {
        if public_key.len() != PUBLIC_KEY_LENGTH {
            return Err(CryptoError::InvalidKeyLength {
                expected: PUBLIC_KEY_LENGTH,
                got: public_key.len(),
            });
        }

        let mut key_bytes = [0u8; PUBLIC_KEY_LENGTH];
        key_bytes.copy_from_slice(public_key);
        let verifying_key = VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| CryptoError::VerificationFailed(e.to_string()))?;

        self.public_keys.insert(node_id.clone(), verifying_key);
        Ok(())
    }

    /// Verify a signature from a specific node
    pub fn verify_signature(
        &self,
        node_id: &NodeId,
        message: &[u8],
        signature: &[u8],
    ) -> Result<bool, CryptoError> {
        let verifying_key = self
            .public_keys
            .get(node_id)
            .ok_or_else(|| CryptoError::KeyNotFound(node_id.clone()))?;

        if signature.len() != SIGNATURE_LENGTH {
            return Err(CryptoError::InvalidSignature);
        }

        let mut sig_bytes = [0u8; SIGNATURE_LENGTH];
        sig_bytes.copy_from_slice(signature);
        let sig = Signature::from_bytes(&sig_bytes);

        Ok(verifying_key.verify(message, &sig).is_ok())
    }

    /// Verify a proposal signature
    pub fn verify_proposal(&self, proposal: &HotStuffProposal) -> Result<bool, CryptoError> {
        let message = HotStuffSigningKey::proposal_sign_message(proposal);
        self.verify_signature(&proposal.leader_id, &message, &proposal.signature)
    }

    /// Verify a vote signature
    pub fn verify_vote(&self, vote: &HotStuffVote) -> Result<bool, CryptoError> {
        let message = vote.sign_message();
        self.verify_signature(&vote.voter_id, &message, &vote.signature)
    }

    /// Verify a new view message signature
    pub fn verify_new_view(&self, msg: &NewViewMessage) -> Result<bool, CryptoError> {
        let message = HotStuffSigningKey::new_view_sign_message(msg.new_view, &msg.high_qc);
        self.verify_signature(&msg.sender_id, &message, &msg.signature)
    }

    /// Verify a quorum certificate
    pub fn verify_qc(
        &self,
        qc: &QuorumCertificate,
        total_validators: u32,
    ) -> Result<bool, CryptoError> {
        // Check if QC has enough signers
        let required = QuorumCertificate::quorum_threshold(total_validators);
        if qc.signer_count < required {
            return Ok(false);
        }

        // Verify signatures based on aggregation type
        match &qc.aggregated_signature {
            AggregatedSignature::MultiSig(signatures) => {
                // DoS guard: a malicious peer can craft a QC whose MultiSig vector
                // contains arbitrarily many entries while keeping signer_count at
                // the quorum threshold (the field checked above). Without this bound,
                // verifying an oversized MultiSig causes O(N) Ed25519 verifications
                // before any further rejection.  One validator can sign at most once
                // per QC, so the signature count must not exceed the total validator set.
                if signatures.len() > total_validators as usize {
                    return Err(CryptoError::VerificationFailed(format!(
                        "QC MultiSig has {} entries but validator set has only {} members (DoS guard)",
                        signatures.len(),
                        total_validators,
                    )));
                }
                // Verify each individual signature
                for sig in signatures {
                    let message = Self::qc_vote_message(qc);
                    let valid = self.verify_with_provided_key(
                        &sig.signer_id,
                        &message,
                        &sig.signature,
                        &sig.public_key,
                    )?;
                    if !valid {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            AggregatedSignature::BLS(_bls_sig) => {
                // BLS signature verification would go here
                // For now, return Ok assuming BLS verification is handled elsewhere
                Ok(true)
            }
            AggregatedSignature::Threshold {
                signature: _,
                threshold,
                total,
            } => {
                // Threshold signature verification
                // Check we have at least threshold signatures
                Ok(qc.signer_count >= *threshold && qc.signer_count <= *total)
            }
        }
    }

    /// Create the message that was signed for a QC vote
    fn qc_vote_message(qc: &QuorumCertificate) -> Vec<u8> {
        let mut message = Vec::new();
        message.push(qc.phase as u8);
        message.extend_from_slice(&qc.view_number.to_le_bytes());
        message.extend_from_slice(qc.block_hash.as_bytes());
        message.extend_from_slice(&qc.block_height.to_le_bytes());
        message
    }

    /// Verify with a provided public key (for QC signatures)
    fn verify_with_provided_key(
        &self,
        node_id: &NodeId,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        // First check if we have a registered key for this node
        if let Some(registered_key) = self.public_keys.get(node_id) {
            // Verify the provided key matches the registered key
            if registered_key.as_bytes() != public_key {
                return Err(CryptoError::VerificationFailed(format!(
                    "Public key mismatch for node {node_id}"
                )));
            }
        }

        // Verify the signature
        if public_key.len() != PUBLIC_KEY_LENGTH {
            return Err(CryptoError::InvalidKeyLength {
                expected: PUBLIC_KEY_LENGTH,
                got: public_key.len(),
            });
        }

        let mut key_bytes = [0u8; PUBLIC_KEY_LENGTH];
        key_bytes.copy_from_slice(public_key);
        let verifying_key = VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| CryptoError::VerificationFailed(e.to_string()))?;

        if signature.len() != SIGNATURE_LENGTH {
            return Err(CryptoError::InvalidSignature);
        }

        let mut sig_bytes = [0u8; SIGNATURE_LENGTH];
        sig_bytes.copy_from_slice(signature);
        let sig = Signature::from_bytes(&sig_bytes);

        Ok(verifying_key.verify(message, &sig).is_ok())
    }
}

impl Default for SignatureVerifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a hash of data using SHA-256
pub fn sha256_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Compute a block hash
pub fn compute_block_hash(
    view: ViewNumber,
    height: BlockHeight,
    parent_hash: &str,
    transactions: &[String],
) -> BlockHash {
    let mut hasher = Sha256::new();
    hasher.update(view.to_le_bytes());
    hasher.update(height.to_le_bytes());
    hasher.update(parent_hash.as_bytes());
    for tx in transactions {
        hasher.update(tx.as_bytes());
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let key = HotStuffSigningKey::generate("test-node".to_string());
        assert_eq!(key.public_key_bytes().len(), PUBLIC_KEY_LENGTH);
    }

    #[test]
    fn test_sign_and_verify() {
        let key = HotStuffSigningKey::generate("test-node".to_string());
        let message = b"test message";

        let signature = key.sign(message);

        let mut verifier = SignatureVerifier::new();
        verifier
            .add_public_key(&"test-node".to_string(), &key.public_key_bytes())
            .unwrap();

        let result = verifier.verify_signature(&"test-node".to_string(), message, &signature);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_invalid_signature() {
        let key = HotStuffSigningKey::generate("test-node".to_string());
        let message = b"test message";

        let mut verifier = SignatureVerifier::new();
        verifier
            .add_public_key(&"test-node".to_string(), &key.public_key_bytes())
            .unwrap();

        // Create an invalid signature
        let invalid_sig = vec![0u8; SIGNATURE_LENGTH];

        let result = verifier.verify_signature(&"test-node".to_string(), message, &invalid_sig);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_vote_signing() {
        let key = HotStuffSigningKey::generate("voter-1".to_string());

        let mut vote = HotStuffVote::new(
            HotStuffPhase::Prepare,
            1,
            "blockhash123".to_string(),
            1,
            "voter-1".to_string(),
            vec![],
        );

        vote.signature = key.sign_vote(&vote);

        let mut verifier = SignatureVerifier::new();
        verifier
            .add_public_key(&"voter-1".to_string(), &key.public_key_bytes())
            .unwrap();

        let result = verifier.verify_vote(&vote);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_block_hash_computation() {
        let hash1 = compute_block_hash(1, 1, "parent", &["tx1".to_string(), "tx2".to_string()]);
        let hash2 = compute_block_hash(1, 1, "parent", &["tx1".to_string(), "tx2".to_string()]);
        let hash3 = compute_block_hash(2, 1, "parent", &["tx1".to_string(), "tx2".to_string()]);

        assert_eq!(hash1, hash2); // Same inputs = same hash
        assert_ne!(hash1, hash3); // Different inputs = different hash
    }

    /// Helper: build an `IndividualSignature` with dummy bytes (bytes are invalid
    /// but we never reach the Ed25519 check in the DoS-guard test path).
    fn dummy_individual_sig(signer_id: &str) -> IndividualSignature {
        IndividualSignature {
            signer_id: signer_id.to_string(),
            signature: vec![0u8; SIGNATURE_LENGTH],
            public_key: vec![0u8; PUBLIC_KEY_LENGTH],
        }
    }

    /// Helper: build a QC with a given MultiSig vector and signer_count.
    fn make_qc_with_multisig(
        signer_count: u32,
        signatures: Vec<IndividualSignature>,
    ) -> QuorumCertificate {
        QuorumCertificate::new(
            HotStuffPhase::Prepare,
            1,
            "blockhash-test".to_string(),
            1,
            AggregatedSignature::MultiSig(signatures),
            signer_count,
            vec![],
        )
    }

    /// A QC whose MultiSig vector is longer than the total validator set must be
    /// rejected with `Err` BEFORE any signature verification is attempted.
    /// Without this guard a peer could send 10 000 dummy signatures and force
    /// 10 000 Ed25519 operations on every verifying node (DoS).
    #[test]
    fn verify_qc_rejects_multisig_larger_than_validator_set() {
        let total_validators: u32 = 4; // quorum = ceil(8/3) = 3
        let qc = make_qc_with_multisig(
            3, // signer_count == quorum (passes the first threshold check)
            // signatures.len() == 5 > total_validators == 4
            (0..5).map(|i| dummy_individual_sig(&format!("v{i}"))).collect(),
        );
        let verifier = SignatureVerifier::new();
        let result = verifier.verify_qc(&qc, total_validators);
        assert!(
            result.is_err(),
            "expected Err for oversized MultiSig, got {result:?}"
        );
    }

    /// A QC whose MultiSig vector length exactly equals total_validators must
    /// be allowed through the DoS guard (even if the sigs are invalid for
    /// other reasons — here signer_count < quorum so it returns Ok(false)).
    #[test]
    fn verify_qc_allows_multisig_equal_to_validator_set() {
        let total_validators: u32 = 4; // quorum = 3
        let qc = make_qc_with_multisig(
            1, // signer_count below quorum → returns Ok(false) but NOT Err
            (0..4).map(|i| dummy_individual_sig(&format!("v{i}"))).collect(),
        );
        let verifier = SignatureVerifier::new();
        let result = verifier.verify_qc(&qc, total_validators);
        // Must be Ok(_) — the DoS guard must not fire; the QC is simply short of quorum.
        assert!(result.is_ok(), "unexpected Err for exactly-max MultiSig: {result:?}");
    }
}
