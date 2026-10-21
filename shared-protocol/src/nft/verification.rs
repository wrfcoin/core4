use serde::{Deserialize, Serialize};

use crate::common::types::{Address, NFTId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationMethod {
    Consensus,
    Oracle,
    ZkProof,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationStatus {
    Pending,
    InProgress,
    Verified,
    Rejected,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verification {
    pub nft_id: NFTId,
    pub verifier: Address,
    pub confidence: f64,
    pub result: bool,
    pub timestamp: u64,
    pub method: VerificationMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationSummary {
    pub nft_id: NFTId,
    pub status: VerificationStatus,
    pub verification_count: u32,
    pub avg_confidence: Option<f64>,
    pub accepted_count: u32,
    pub rejected_count: u32,
}

pub fn derive_verification_status(
    accepted_count: u32,
    rejected_count: u32,
    avg_confidence: Option<f64>,
    min_confidence: f64,
    min_verifications: u32,
) -> VerificationStatus {
    if rejected_count > accepted_count {
        return VerificationStatus::Rejected;
    }
    if accepted_count + rejected_count < min_verifications {
        return VerificationStatus::InProgress;
    }
    if accepted_count == 0 {
        return VerificationStatus::Pending;
    }
    if avg_confidence.unwrap_or(0.0) >= min_confidence {
        VerificationStatus::Verified
    } else {
        VerificationStatus::InProgress
    }
}

pub fn summarize_verifications(
    nft_id: NFTId,
    verifications: &[Verification],
    min_confidence: f64,
    min_verifications: u32,
) -> VerificationSummary {
    let mut accepted_count = 0u32;
    let mut rejected_count = 0u32;
    let mut confidence_sum = 0.0f64;
    let mut confidence_samples = 0u32;

    for verification in verifications {
        if verification.result {
            accepted_count = accepted_count.saturating_add(1);
            confidence_sum += verification.confidence;
            confidence_samples = confidence_samples.saturating_add(1);
        } else {
            rejected_count = rejected_count.saturating_add(1);
        }
    }

    let avg_confidence = if confidence_samples == 0 {
        None
    } else {
        Some((confidence_sum / confidence_samples as f64).clamp(0.0, 1.0))
    };
    let status = derive_verification_status(
        accepted_count,
        rejected_count,
        avg_confidence,
        min_confidence,
        min_verifications,
    );

    VerificationSummary {
        nft_id,
        status,
        verification_count: verifications.len() as u32,
        avg_confidence,
        accepted_count,
        rejected_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(seed: u8) -> Address {
        Address::new([seed; 20])
    }

    #[test]
    fn summary_marks_verified_after_threshold() {
        let nft_id = NFTId::new("nft-1".to_string());
        let verifications = vec![
            Verification {
                nft_id: nft_id.clone(),
                verifier: addr(1),
                confidence: 0.8,
                result: true,
                timestamp: 1,
                method: VerificationMethod::Oracle,
            },
            Verification {
                nft_id: nft_id.clone(),
                verifier: addr(2),
                confidence: 0.9,
                result: true,
                timestamp: 2,
                method: VerificationMethod::Oracle,
            },
        ];
        let summary = summarize_verifications(nft_id, &verifications, 0.7, 2);
        assert_eq!(summary.status, VerificationStatus::Verified);
        assert_eq!(summary.verification_count, 2);
        assert_eq!(summary.accepted_count, 2);
    }

    #[test]
    fn summary_marks_rejected_when_rejections_dominate() {
        let nft_id = NFTId::new("nft-2".to_string());
        let verifications = vec![
            Verification {
                nft_id: nft_id.clone(),
                verifier: addr(1),
                confidence: 0.9,
                result: false,
                timestamp: 1,
                method: VerificationMethod::Manual,
            },
            Verification {
                nft_id: nft_id.clone(),
                verifier: addr(2),
                confidence: 0.9,
                result: false,
                timestamp: 2,
                method: VerificationMethod::Manual,
            },
            Verification {
                nft_id: nft_id.clone(),
                verifier: addr(3),
                confidence: 0.9,
                result: true,
                timestamp: 3,
                method: VerificationMethod::Manual,
            },
        ];
        let summary = summarize_verifications(nft_id, &verifications, 0.7, 2);
        assert_eq!(summary.status, VerificationStatus::Rejected);
        assert_eq!(summary.rejected_count, 2);
    }
}
