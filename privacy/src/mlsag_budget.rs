//! Budget-aware MLSAG ring-signature batch verification.
//!
//! MLSAG verification performs ~8 EC operations per ring member, which can
//! stall block verification on constrained hardware (e.g., N100 CPU) when a
//! block contains many `RewardClaim` transactions with large rings. This module
//! provides a time-budgeted pre-pass: signatures are verified sequentially until
//! the wall-clock budget is exhausted, and remaining items are deferred to state
//! application (which already calls `ring_sig.verify()`).

use std::time::{Duration, Instant};

use crate::ring_signatures::MLSAGSignature;
use crate::error::PrivacyResult;

/// Default MLSAG verification budget per block (milliseconds).
/// 50 ms is conservative for N100 (4-core Intel Alder Lake-N).
pub const MLSAG_VERIFY_BUDGET_MS_DEFAULT: u64 = 50;

/// A single MLSAG verification work item.
pub struct MlsagVerifyItem {
    pub signature: MLSAGSignature,
    pub message: Vec<u8>,
}

/// Result of a budgeted MLSAG batch verification.
pub struct MlsagBudgetResult {
    /// Verification outcomes for items processed within budget.
    pub verified: Vec<PrivacyResult<bool>>,
    /// Items that exceeded the time budget (not yet verified).
    pub deferred: Vec<MlsagVerifyItem>,
    /// Wall-clock time spent on verification (ms).
    pub elapsed_ms: u64,
}

/// Verify MLSAG signatures sequentially until the time budget is exhausted.
/// Remaining items are returned in `deferred` for later verification.
pub fn verify_batch_budgeted(
    items: Vec<MlsagVerifyItem>,
    budget: Duration,
) -> MlsagBudgetResult {
    let start = Instant::now();
    let mut verified = Vec::new();
    let mut deferred = Vec::new();

    for item in items {
        if start.elapsed() >= budget {
            deferred.push(item);
            continue;
        }
        let result = item.signature.verify(&item.message);
        verified.push(result);
    }

    MlsagBudgetResult {
        verified,
        deferred,
        elapsed_ms: start.elapsed().as_millis() as u64,
    }
}

/// Verify deferred items without a budget constraint.
pub fn verify_deferred(items: &[MlsagVerifyItem]) -> Vec<PrivacyResult<bool>> {
    items
        .iter()
        .map(|item| item.signature.verify(&item.message))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring_signatures::{MLSAGSignature, RingMember, MIN_RING_SIZE};
    use curve25519_dalek::{constants::RISTRETTO_BASEPOINT_TABLE, scalar::Scalar};
    use rand_core::{OsRng, RngCore};

    /// Create a key pair: (private scalar bytes, compressed public key bytes).
    fn generate_keypair() -> ([u8; 32], [u8; 32]) {
        let mut priv_bytes = [0u8; 64];
        OsRng.fill_bytes(&mut priv_bytes);
        let scalar = Scalar::from_bytes_mod_order_wide(&priv_bytes);
        let public = (&scalar * RISTRETTO_BASEPOINT_TABLE).compress().to_bytes();
        (scalar.to_bytes(), public)
    }

    /// Create a valid MLSAG signature with the given ring size (total members).
    fn make_item(ring_size: usize) -> MlsagVerifyItem {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..ring_size - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();
        let message = b"budget_test_message".to_vec();
        let sig = MLSAGSignature::sign(&message, &signer_priv, &decoys)
            .expect("signing must succeed");
        MlsagVerifyItem { signature: sig, message }
    }

    #[test]
    fn budget_gate_triggers() {
        let items = vec![make_item(8), make_item(8), make_item(8)];
        let result = verify_batch_budgeted(items, Duration::ZERO);
        assert!(
            result.verified.is_empty(),
            "zero budget should verify nothing"
        );
        assert_eq!(result.deferred.len(), 3, "all items should be deferred");
    }

    #[test]
    fn deferred_verification_completes() {
        let items = vec![make_item(8), make_item(8)];
        let results = verify_deferred(&items);
        assert_eq!(results.len(), 2);
        for r in results {
            assert!(r.expect("verify must not error"), "signature must be valid");
        }
    }

    #[test]
    fn within_budget_all_verified() {
        let items = vec![make_item(MIN_RING_SIZE)];
        let result = verify_batch_budgeted(items, Duration::from_secs(5));
        assert_eq!(result.verified.len(), 1, "single item should be verified");
        assert!(result.deferred.is_empty(), "nothing should be deferred");
        assert!(
            result.verified[0].as_ref().expect("verify must not error"),
            "signature must be valid"
        );
    }

    #[test]
    fn benchmark_mlsag_verify_by_ring_size() {
        for ring_size in [8, 16, 32] {
            let item = make_item(ring_size);
            let start = Instant::now();
            let valid = item.signature.verify(&item.message).expect("verify must not error");
            let elapsed = start.elapsed();
            eprintln!("ring_size={ring_size}: verify={elapsed:?}");
            assert!(valid, "signature with ring_size={ring_size} must verify");
        }
    }
}
