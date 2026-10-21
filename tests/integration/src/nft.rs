#![allow(dead_code)]
#![allow(clippy::disallowed_types)]

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Represents a Computable NFT which contains analysis data and metadata
#[derive(Debug, Clone)]
pub struct ComputableNFT {
    pub id: String,
    pub creator: String,
    pub timestamp: u64,
    pub title: String,
    pub description: String,
    pub input_data_hashes: Vec<String>,
    pub executable_code: String,
    pub outputs: String,
    pub metadata: HashMap<String, String>,
    #[allow(dead_code)]
    pub provenance: Vec<String>,
    pub verification_status: VerificationStatus,
    pub engagement_metrics: EngagementMetrics,
}

/// Represents the verification status of a Computable NFT
#[derive(Debug, Clone)]
pub enum VerificationStatus {
    Pending,
    Verified(Vec<Verification>),
    #[allow(dead_code)]
    Rejected(String), // Reason for rejection
}

/// Represents a single verification of a Computable NFT
#[derive(Debug, Clone)]
pub struct Verification {
    pub verifier: String,
    pub timestamp: u64,
    #[allow(dead_code)]
    pub signature: String,
    pub feedback: String,
}

/// Tracks engagement with a Computable NFT
#[derive(Debug, Clone, Default)]
pub struct EngagementMetrics {
    pub views: u64,
    pub reuses: u64,
    pub endorsements: HashMap<String, String>, // address -> endorsement message
}

impl ComputableNFT {
    /// Create a new Computable NFT
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        creator: String,
        title: String,
        description: String,
        input_data_hashes: Vec<String>,
        executable_code: String,
        outputs: String,
        metadata: HashMap<String, String>,
        provenance: Vec<String>,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        // Generate a simple ID using timestamp and creator
        let id = format!("nft-{timestamp}-{creator}");

        ComputableNFT {
            id,
            creator,
            timestamp,
            title,
            description,
            input_data_hashes,
            executable_code,
            outputs,
            metadata,
            provenance,
            verification_status: VerificationStatus::Pending,
            engagement_metrics: EngagementMetrics::default(),
        }
    }

    /// Add a verification to this NFT
    pub fn add_verification(&mut self, verifier: String, signature: String, feedback: String) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        let verification = Verification {
            verifier,
            timestamp,
            signature,
            feedback,
        };

        match &mut self.verification_status {
            VerificationStatus::Pending => {
                self.verification_status = VerificationStatus::Verified(vec![verification]);
            },
            VerificationStatus::Verified(verifications) => {
                verifications.push(verification);
            },
            VerificationStatus::Rejected(_) => {
                // If previously rejected, we can update to verified
                self.verification_status = VerificationStatus::Verified(vec![verification]);
            },
        }
    }

    /// Reject this NFT
    pub fn reject(&mut self, reason: String) {
        self.verification_status = VerificationStatus::Rejected(reason);
    }

    /// Record a view of this NFT
    pub fn record_view(&mut self) {
        self.engagement_metrics.views += 1;
    }

    /// Record a reuse of this NFT
    pub fn record_reuse(&mut self) {
        self.engagement_metrics.reuses += 1;
    }

    /// Add an endorsement to this NFT
    pub fn add_endorsement(&mut self, endorser: String, message: String) {
        self.engagement_metrics
            .endorsements
            .insert(endorser, message);
    }

    /// Calculate the fixed reward for this NFT
    pub fn calculate_fixed_reward(&self) -> u64 {
        // Base fixed reward
        let mut reward = 10;

        // Add reward based on verification count
        if let VerificationStatus::Verified(verifications) = &self.verification_status {
            reward += verifications.len() as u64;
        }

        reward
    }

    /// Calculate the dynamic reward for this NFT based on engagement
    pub fn calculate_dynamic_reward(&self) -> u64 {
        let mut reward = 0;

        // Add reward based on views (1 reward per 10 views)
        reward += self.engagement_metrics.views / 10;

        // Add reward based on reuses (2 rewards per reuse)
        reward += self.engagement_metrics.reuses * 2;

        // Add reward based on endorsements (5 rewards per endorsement)
        reward += (self.engagement_metrics.endorsements.len() as u64) * 5;

        reward
    }

    /// Get total reward for this NFT
    pub fn calculate_total_reward(&self) -> u64 {
        match &self.verification_status {
            VerificationStatus::Verified(_) => {
                // If verified, combine fixed and dynamic rewards
                self.calculate_fixed_reward() + self.calculate_dynamic_reward()
            },
            _ => 0, // No rewards if not verified
        }
    }
}

/// Manages a collection of Computable NFTs
#[derive(Debug)]
pub struct NFTRegistry {
    pub nfts: HashMap<String, ComputableNFT>,
}

impl NFTRegistry {
    /// Create a new NFT registry
    pub fn new() -> Self {
        NFTRegistry {
            nfts: HashMap::new(),
        }
    }

    /// Add an NFT to the registry
    pub fn add_nft(&mut self, nft: ComputableNFT) {
        self.nfts.insert(nft.id.clone(), nft);
    }

    /// Get an NFT by ID
    pub fn get_nft(&self, id: &str) -> Option<&ComputableNFT> {
        self.nfts.get(id)
    }

    /// Get a mutable reference to an NFT by ID
    pub fn get_nft_mut(&mut self, id: &str) -> Option<&mut ComputableNFT> {
        self.nfts.get_mut(id)
    }

    /// Get all NFTs
    pub fn get_all_nfts(&self) -> Vec<&ComputableNFT> {
        self.nfts.values().collect()
    }

    /// Get all NFTs by creator
    pub fn get_nfts_by_creator(&self, creator: &str) -> Vec<&ComputableNFT> {
        self.nfts
            .values()
            .filter(|nft| nft.creator == creator)
            .collect()
    }

    /// Get verified NFTs
    pub fn get_verified_nfts(&self) -> Vec<&ComputableNFT> {
        self.nfts
            .values()
            .filter(|nft| matches!(nft.verification_status, VerificationStatus::Verified(_)))
            .collect()
    }

    /// Get pending NFTs
    pub fn get_pending_nfts(&self) -> Vec<&ComputableNFT> {
        self.nfts
            .values()
            .filter(|nft| matches!(nft.verification_status, VerificationStatus::Pending))
            .collect()
    }

    /// Get rejected NFTs
    pub fn get_rejected_nfts(&self) -> Vec<&ComputableNFT> {
        self.nfts
            .values()
            .filter(|nft| matches!(nft.verification_status, VerificationStatus::Rejected(_)))
            .collect()
    }

    /// Verify an NFT
    pub fn verify_nft(
        &mut self,
        id: &str,
        verifier: String,
        signature: String,
        feedback: String,
    ) -> bool {
        if let Some(nft) = self.get_nft_mut(id) {
            nft.add_verification(verifier, signature, feedback);
            true
        } else {
            false
        }
    }

    /// Reject an NFT
    pub fn reject_nft(&mut self, id: &str, reason: String) -> bool {
        if let Some(nft) = self.get_nft_mut(id) {
            nft.reject(reason);
            true
        } else {
            false
        }
    }

    /// Record a view of an NFT
    pub fn record_view(&mut self, id: &str) -> bool {
        if let Some(nft) = self.get_nft_mut(id) {
            nft.record_view();
            true
        } else {
            false
        }
    }

    /// Record a reuse of an NFT
    pub fn record_reuse(&mut self, id: &str) -> bool {
        if let Some(nft) = self.get_nft_mut(id) {
            nft.record_reuse();
            true
        } else {
            false
        }
    }

    /// Add an endorsement to an NFT
    pub fn add_endorsement(&mut self, id: &str, endorser: String, message: String) -> bool {
        if let Some(nft) = self.get_nft_mut(id) {
            nft.add_endorsement(endorser, message);
            true
        } else {
            false
        }
    }
}
