use std::fmt::Debug;

/// Trait for verifying ZK proofs in the consensus layer.
///
/// This trait keeps Arkworks dependencies out of shared-protocol.
/// The real implementation lives in native-chain behind the `zk` feature flag;
/// when the feature is off, `RejectAllZkVerifier` rejects all proofs to prevent
/// consensus splits between zk-enabled and non-zk nodes.
pub trait ZkVerifier: Send + Sync + Debug {
    fn verify_range_proof(&self, proof_hex: &str, commitment_hex: &str) -> Result<bool, String>;

    fn verify_confidential_transfer(
        &self,
        proof_hex: &str,
        input_commitment_hex: &str,
        output_commitment_hex: &str,
        fee: u64,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<bool, String>;

    fn verify_selective_disclosure(
        &self,
        proof_hex: &str,
        commitment_hex: &str,
    ) -> Result<bool, String>;

    fn verify_weather_model(
        &self,
        proof_hex: &str,
        input_hash_hex: &str,
        output_hash_hex: &str,
        model_id: u64,
        time_secs: u64,
    ) -> Result<bool, String>;

    /// Verify a layer2 batch proof.
    ///
    /// `proof_hex` is the hex-encoded raw Groth16 proof bytes (i.e.
    /// `hex::encode(&zk_proof.proof_bytes)`).
    /// `commitment_hex` is the hex-encoded little-endian BN254 Fr commitment
    /// (the value stored in `Proof::public_inputs[0]` by `ZkVerifier::generate_batch_proof`).
    fn verify_batch_proof(&self, proof_hex: &str, commitment_hex: &str) -> Result<bool, String>;
}

/// Default verifier that rejects all proofs. Used when the `zk` feature is
/// disabled, ensuring non-zk nodes reject blocks containing ZK transactions
/// rather than silently accepting fabricated proofs (which would cause a
/// consensus split with zk-enabled nodes).
#[derive(Debug)]
pub struct RejectAllZkVerifier;

impl ZkVerifier for RejectAllZkVerifier {
    fn verify_range_proof(&self, _proof_hex: &str, _commitment_hex: &str) -> Result<bool, String> {
        Err("ZK verification unavailable: node compiled without zk feature".into())
    }

    fn verify_confidential_transfer(
        &self,
        _proof_hex: &str,
        _input_commitment_hex: &str,
        _output_commitment_hex: &str,
        _fee: u64,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<bool, String> {
        Err("ZK verification unavailable: node compiled without zk feature".into())
    }

    fn verify_selective_disclosure(
        &self,
        _proof_hex: &str,
        _commitment_hex: &str,
    ) -> Result<bool, String> {
        Err("ZK verification unavailable: node compiled without zk feature".into())
    }

    fn verify_weather_model(
        &self,
        _proof_hex: &str,
        _input_hash_hex: &str,
        _output_hash_hex: &str,
        _model_id: u64,
        _time_secs: u64,
    ) -> Result<bool, String> {
        Err("ZK verification unavailable: node compiled without zk feature".into())
    }

    fn verify_batch_proof(&self, _proof_hex: &str, _commitment_hex: &str) -> Result<bool, String> {
        Err("ZK verification unavailable: node compiled without zk feature".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_ERR: &str = "ZK verification unavailable: node compiled without zk feature";

    #[test]
    fn reject_all_verifier_rejects_range_proof() {
        let v = RejectAllZkVerifier;
        let err = v.verify_range_proof("deadbeef", "cafebabe").unwrap_err();
        assert_eq!(err, EXPECTED_ERR);
    }

    #[test]
    fn reject_all_verifier_rejects_confidential_transfer() {
        let v = RejectAllZkVerifier;
        let err = v
            .verify_confidential_transfer("deadbeef", "in_commit", "out_commit", 10, 1, 1)
            .unwrap_err();
        assert_eq!(err, EXPECTED_ERR);
    }

    #[test]
    fn reject_all_verifier_rejects_all_remaining_methods() {
        let v: &dyn ZkVerifier = &RejectAllZkVerifier;

        // verify_selective_disclosure
        assert_eq!(
            v.verify_selective_disclosure("proof", "commit").unwrap_err(),
            EXPECTED_ERR
        );

        // verify_weather_model
        assert_eq!(
            v.verify_weather_model("proof", "in_hash", "out_hash", 1, 1_700_000_000)
                .unwrap_err(),
            EXPECTED_ERR
        );

        // verify_batch_proof
        assert_eq!(
            v.verify_batch_proof("proof", "commit").unwrap_err(),
            EXPECTED_ERR
        );
    }
}
