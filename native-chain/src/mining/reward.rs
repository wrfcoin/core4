use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::hash::Hash;
type Timestamp = u64;
use anyhow::Result;
use log::info;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sha2::{Sha256, Digest};
use serde::{Serialize, Deserialize};

/// WRF base units per coin (10^8). All currency amounts are u64 in base units.
pub const WRF_UNITS_PER_COIN: u64 = 100_000_000;

/// Mining contribution type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContributionType {
    /// Block mining
    BlockMining,

    /// Transaction verification
    TransactionVerification,

    /// Computational NFT creation
    ComputationalNFT,

    /// Data provision
    DataProvision,

    /// Network maintenance
    NetworkMaintenance,
}

/// Mining contribution record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningContribution {
    /// Contributor address
    pub contributor: Address,

    /// Contribution type
    pub contribution_type: ContributionType,

    /// Contribution timestamp
    pub timestamp: Timestamp,

    /// Contribution hash (for verification)
    pub hash: Hash,

    /// Contribution weight
    pub weight: u64,

    /// Contribution proof
    pub proof: Vec<u8>,

    /// Verified by validators (addresses and signatures)
    pub verifications: HashMap<Address, Vec<u8>>,

    /// Block height where contribution was recorded
    pub block_height: Option<u64>,

    /// Block hash where contribution was recorded
    pub block_hash: Option<Hash>,
}

/// Reward distribution record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardDistribution {
    /// Block height
    pub block_height: u64,

    /// Block hash
    pub block_hash: Hash,

    /// Block timestamp
    pub timestamp: Timestamp,

    /// Total reward amount
    pub total_reward: u64,

    /// Distribution per contributor
    pub distributions: HashMap<Address, u64>,

    /// Mathematical proof of fair distribution
    pub distribution_proof: Vec<u8>,

    /// Merkle root of all contributions included
    pub contributions_merkle_root: Hash,

    /// Signatures of validators
    pub validator_signatures: HashMap<Address, Vec<u8>>,
}

/// Reward distribution proof generator
#[derive(Debug)]
pub struct RewardProofGenerator {
    /// Contributions pool
    contributions: Arc<RwLock<Vec<MiningContribution>>>,

    /// Historical reward distributions
    distributions: Arc<RwLock<Vec<RewardDistribution>>>,

    /// Required verifications for a contribution to be valid
    required_verifications: usize,

    /// Validator addresses
    validators: Vec<Address>,
}

impl RewardProofGenerator {
    /// Create a new reward proof generator
    pub fn new(required_verifications: usize, validators: Vec<Address>) -> Self {
        Self {
            contributions: Arc::new(RwLock::new(Vec::new())),
            distributions: Arc::new(RwLock::new(Vec::new())),
            required_verifications,
            validators,
        }
    }

    /// Add a mining contribution to the pool
    pub async fn add_contribution(&self, contribution: MiningContribution) -> Result<()> {
        // Validate contribution has proper proof
        if !self.verify_contribution_proof(&contribution) {
            return Err(anyhow::anyhow!("Invalid contribution proof"));
        }

        // Check if contribution has enough verifications
        if contribution.verifications.len() < self.required_verifications {
            return Err(anyhow::anyhow!(
                "Contribution does not have enough verifications (has {}, needs {})",
                contribution.verifications.len(),
                self.required_verifications
            ));
        }

        // Verify all verification signatures
        for (validator, signature) in &contribution.verifications {
            if !self.verify_validator_signature(validator, &contribution.hash, signature) {
                return Err(anyhow::anyhow!("Invalid validator signature from {}", hex::encode(validator.as_bytes())));
            }
        }

        // Add contribution to pool
        let contributor_hex = hex::encode(contribution.contributor.as_bytes());
        let mut contributions = self.contributions.write().await;
        contributions.push(contribution);

        info!("Added new mining contribution from {}", contributor_hex);

        Ok(())
    }

    /// Calculate a fair reward distribution for a new block
    pub async fn calculate_reward_distribution(&self,
                                              block: &Block,
                                              total_reward: u64) -> Result<RewardDistribution> {
        let contributions = self.contributions.read().await;

        // Filter contributions that haven't been included in previous blocks
        let pending_contributions: Vec<&MiningContribution> = contributions
            .iter()
            .filter(|c| c.block_height.is_none())
            .collect();

        if pending_contributions.is_empty() {
            return Err(anyhow::anyhow!("No pending contributions to include in reward distribution"));
        }

        // Calculate total contribution weight
        let total_weight: u64 = pending_contributions.iter().map(|c| c.weight).sum();

        // Calculate distribution based on proportional contribution
        let mut distributions = HashMap::new();
        let mut contribution_hashes = Vec::new();

        for contribution in &pending_contributions {
            // Pure integer arithmetic avoids f64 divergence between x86_64 and
            // ARM (issue #1849).  u128 prevents overflow for realistic weights
            // and reward totals; division truncates toward zero (rounds down).
            let reward_amount =
                (contribution.weight as u128 * total_reward as u128 / total_weight as u128) as u64;

            // Add reward to contributor's distribution
            *distributions.entry(contribution.contributor).or_insert(0) += reward_amount;

            // Add contribution hash for merkle tree
            contribution_hashes.push(contribution.hash);
        }

        // Generate merkle root for all included contributions
        let contributions_merkle_root = self.generate_merkle_root(&contribution_hashes)?;

        // Generate mathematical proof of fair distribution
        let distribution_proof = self.generate_distribution_proof(
            &pending_contributions,
            &distributions,
            total_reward,
            total_weight
        )?;

        // block.hash is Option<Hash>; fall back to default if not yet set
        let block_hash = block.hash.unwrap_or_default();
        let block_timestamp = block.timestamp.timestamp() as u64;

        // Create reward distribution
        let distribution = RewardDistribution {
            block_height: block.height,
            block_hash,
            timestamp: block_timestamp,
            total_reward,
            distributions,
            distribution_proof,
            contributions_merkle_root,
            validator_signatures: HashMap::new(), // To be filled by validators
        };

        Ok(distribution)
    }

    /// Apply a reward distribution to update contribution records
    pub async fn apply_reward_distribution(&self, distribution: RewardDistribution) -> Result<()> {
        // Verify distribution has enough validator signatures
        if distribution.validator_signatures.len() < self.required_verifications {
            return Err(anyhow::anyhow!(
                "Distribution does not have enough validator signatures (has {}, needs {})",
                distribution.validator_signatures.len(),
                self.required_verifications
            ));
        }

        // Verify all validator signatures
        for (validator, signature) in &distribution.validator_signatures {
            if !self.verify_validator_signature(validator, &distribution.block_hash, signature) {
                return Err(anyhow::anyhow!("Invalid validator signature from {}", hex::encode(validator.as_bytes())));
            }
        }

        // Verify distribution proof
        if !self.verify_distribution_proof(&distribution) {
            return Err(anyhow::anyhow!("Invalid distribution proof"));
        }

        // Update contributions to mark them as included in this block
        let mut contributions = self.contributions.write().await;
        for contribution in contributions.iter_mut() {
            if contribution.block_height.is_none() &&
               distribution.distributions.contains_key(&contribution.contributor) {
                contribution.block_height = Some(distribution.block_height);
                contribution.block_hash = Some(distribution.block_hash);
            }
        }

        // Add distribution to history
        let mut distributions = self.distributions.write().await;
        distributions.push(distribution.clone());

        info!("Applied reward distribution for block {} with total reward {}",
              distribution.block_height, distribution.total_reward);

        Ok(())
    }

    /// Generate a merkle root from a list of hashes
    fn generate_merkle_root(&self, hashes: &[Hash]) -> Result<Hash> {
        if hashes.is_empty() {
            return Err(anyhow::anyhow!("Cannot generate merkle root from empty hash list"));
        }

        if hashes.len() == 1 {
            return Ok(hashes[0]);
        }

        let mut parent_hashes = Vec::new();

        // Generate parent hashes by combining pairs
        for i in (0..hashes.len()).step_by(2) {
            let left = hashes[i];
            let right = if i + 1 < hashes.len() { hashes[i + 1] } else { left };

            let mut hasher = Sha256::new();
            hasher.update(left.as_bytes());
            hasher.update(right.as_bytes());
            let result: [u8; 32] = hasher.finalize().into();
            let parent = Hash::from(result);

            parent_hashes.push(parent);
        }

        // Recursively compute the merkle root
        self.generate_merkle_root(&parent_hashes)
    }

    /// Generate mathematical proof of fair distribution
    fn generate_distribution_proof(&self,
                                 contributions: &[&MiningContribution],
                                 distributions: &HashMap<Address, u64>,
                                 total_reward: u64,
                                 total_weight: u64) -> Result<Vec<u8>> {
        // Create a proof structure that can be independently verified
        let mut proof = Vec::new();

        // Add total reward and weight to proof
        proof.extend_from_slice(&total_reward.to_le_bytes());
        proof.extend_from_slice(&total_weight.to_le_bytes());

        // Add number of contributions
        proof.extend_from_slice(&(contributions.len() as u64).to_le_bytes());

        // Add contribution data (address, weight, calculated reward)
        for contribution in contributions {
            // Add contributor address
            proof.extend_from_slice(contribution.contributor.as_bytes());

            // Add contribution weight
            proof.extend_from_slice(&contribution.weight.to_le_bytes());

            // Calculate expected reward — integer arithmetic, no f64 (issue #1849).
            let reward_amount =
                (contribution.weight as u128 * total_reward as u128 / total_weight as u128) as u64;

            // Add calculated reward
            proof.extend_from_slice(&reward_amount.to_le_bytes());

            // Add actual reward (from distributions)
            let actual_reward = distributions.get(&contribution.contributor).unwrap_or(&0);
            proof.extend_from_slice(&actual_reward.to_le_bytes());
        }

        // Hash the entire proof for integrity
        let mut hasher = Sha256::new();
        hasher.update(&proof);

        // Append the hash to the proof
        proof.extend_from_slice(&hasher.finalize());

        Ok(proof)
    }

    /// Verify a contribution proof
    fn verify_contribution_proof(&self, contribution: &MiningContribution) -> bool {
        // For a real implementation, this would verify zero-knowledge proofs
        // or other cryptographic evidence of actual work performed

        // For demonstration, we'll assume all proofs are valid
        !contribution.proof.is_empty()
    }

    /// Verify a distribution proof
    fn verify_distribution_proof(&self, distribution: &RewardDistribution) -> bool {
        // For a real implementation, this would verify the mathematical proof
        // of fair distribution by reconstructing the distribution calculation

        // For demonstration, we'll assume all proofs are valid
        !distribution.distribution_proof.is_empty()
    }

    /// Verify a validator signature
    fn verify_validator_signature(&self,
                                validator: &Address,
                                _message: &Hash,
                                signature: &[u8]) -> bool {
        // In a real implementation, this would verify the cryptographic signature
        // using the validator's public key

        // For demonstration, we'll check if the validator is in our list
        // and assume all signatures from valid validators are correct
        self.validators.contains(validator) && !signature.is_empty()
    }

    /// Get contribution statistics
    pub async fn get_contribution_stats(&self) -> HashMap<ContributionType, u64> {
        let contributions = self.contributions.read().await;
        let mut stats = HashMap::new();

        for contribution in contributions.iter() {
            *stats.entry(contribution.contribution_type).or_insert(0) += 1;
        }

        stats
    }

    /// Get reward distribution statistics
    pub async fn get_distribution_stats(&self) -> (u64, u64, f64) {
        let distributions = self.distributions.read().await;

        let total_distributions = distributions.len() as u64;
        let mut total_reward = 0u64;
        let mut contributors = HashMap::new();

        for distribution in distributions.iter() {
            total_reward += distribution.total_reward;

            for (address, _) in distribution.distributions.iter() {
                *contributors.entry(address).or_insert(0u64) += 1;
            }
        }

        let avg_reward_per_block = if total_distributions > 0 {
            total_reward as f64 / total_distributions as f64
        } else {
            0.0
        };

        (total_distributions, contributors.len() as u64, avg_reward_per_block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------

    /// Build a deterministic `Hash` from a single byte (for test fixtures).
    fn make_hash(seed: u8) -> Hash {
        Hash::from([seed; 32])
    }

    /// Build an `Address` from a single byte.
    fn make_addr(seed: u8) -> Address {
        Address::new([seed; 20])
    }

    /// Build a minimal `RewardProofGenerator` with no validators and no required
    /// verifications — suitable for testing internal helpers that don't touch
    /// the validators list.
    fn gen() -> RewardProofGenerator {
        RewardProofGenerator::new(0, vec![])
    }

    // ---------------------------------------------------------------------------
    // MerkleRoot tests
    // ---------------------------------------------------------------------------

    #[test]
    fn merkle_single_hash_returns_itself() {
        let g = gen();
        let h = make_hash(0x01);
        let root = g.generate_merkle_root(&[h]).unwrap();
        assert_eq!(root, h, "single-element merkle root must equal the element itself");
    }

    #[test]
    fn merkle_empty_input_returns_err() {
        let g = gen();
        let result = g.generate_merkle_root(&[]);
        assert!(result.is_err(), "empty input must return Err");
    }

    #[test]
    fn merkle_two_hashes_deterministic() {
        let g = gen();
        let h0 = make_hash(0x01);
        let h1 = make_hash(0x02);

        let root_a = g.generate_merkle_root(&[h0, h1]).unwrap();
        let root_b = g.generate_merkle_root(&[h0, h1]).unwrap();
        assert_eq!(root_a, root_b, "same inputs must produce the same root");
        // Root must differ from the individual leaves
        assert_ne!(root_a, h0);
        assert_ne!(root_a, h1);
    }

    #[test]
    fn merkle_two_hashes_order_matters() {
        let g = gen();
        let h0 = make_hash(0x01);
        let h1 = make_hash(0x02);

        let root_ab = g.generate_merkle_root(&[h0, h1]).unwrap();
        let root_ba = g.generate_merkle_root(&[h1, h0]).unwrap();
        assert_ne!(root_ab, root_ba, "swapping inputs must change the root");
    }

    /// Power-of-two: four leaves should produce a deterministic root.
    #[test]
    fn merkle_four_hashes_power_of_two() {
        let g = gen();
        let hashes: Vec<Hash> = (1u8..=4).map(make_hash).collect();

        let root1 = g.generate_merkle_root(&hashes).unwrap();
        let root2 = g.generate_merkle_root(&hashes).unwrap();
        assert_eq!(root1, root2, "power-of-two input must be deterministic");
    }

    /// Non-power-of-two: three leaves — the last leaf is duplicated internally.
    /// The result must still be deterministic.
    #[test]
    fn merkle_three_hashes_non_power_of_two() {
        let g = gen();
        let hashes: Vec<Hash> = (1u8..=3).map(make_hash).collect();

        let root1 = g.generate_merkle_root(&hashes).unwrap();
        let root2 = g.generate_merkle_root(&hashes).unwrap();
        assert_eq!(root1, root2, "non-power-of-two input must be deterministic");
    }

    /// Changing any single leaf must change the root (tamper-sensitivity).
    #[test]
    fn merkle_tamper_sensitivity() {
        let g = gen();
        let hashes: Vec<Hash> = (1u8..=4).map(make_hash).collect();
        let original_root = g.generate_merkle_root(&hashes).unwrap();

        for i in 0..hashes.len() {
            let mut tampered = hashes.clone();
            tampered[i] = make_hash(0xFF); // replace with a different hash
            let tampered_root = g.generate_merkle_root(&tampered).unwrap();
            assert_ne!(
                tampered_root, original_root,
                "modifying leaf {i} must change the root"
            );
        }
    }

    /// Larger batch: eight leaves — confirm determinism holds at scale.
    #[test]
    fn merkle_eight_hashes_deterministic() {
        let g = gen();
        let hashes: Vec<Hash> = (1u8..=8).map(make_hash).collect();

        let r1 = g.generate_merkle_root(&hashes).unwrap();
        let r2 = g.generate_merkle_root(&hashes).unwrap();
        assert_eq!(r1, r2);
    }

    // ---------------------------------------------------------------------------
    // Reward distribution math tests
    //
    // `calculate_reward_distribution` is async and requires a `Block` plus valid
    // contributions in the pool.  We test the proportional-weight distribution
    // math directly by replicating the algorithm.
    // ---------------------------------------------------------------------------

    /// Replicate the reward-share integer arithmetic from `calculate_reward_distribution`
    /// and return the per-address distribution map.
    fn compute_distribution(
        weights: &[(Address, u64)],
        total_reward: u64,
    ) -> HashMap<Address, u64> {
        let total_weight: u64 = weights.iter().map(|(_, w)| *w).sum();
        let mut dist: HashMap<Address, u64> = HashMap::new();
        for (addr, weight) in weights {
            // Mirror the production path exactly — integer only (issue #1849).
            let amount =
                (*weight as u128 * total_reward as u128 / total_weight as u128) as u64;
            *dist.entry(*addr).or_insert(0) += amount;
        }
        dist
    }

    #[test]
    fn reward_single_contributor_gets_full_reward() {
        let addr = make_addr(0x01);
        let total_reward = 50 * WRF_UNITS_PER_COIN;
        let dist = compute_distribution(&[(addr, 100)], total_reward);

        assert_eq!(dist[&addr], total_reward, "sole contributor must receive the full reward");
    }

    #[test]
    fn reward_two_equal_contributors_split_evenly() {
        let a = make_addr(0x01);
        let b = make_addr(0x02);
        let total_reward = 100 * WRF_UNITS_PER_COIN;
        let dist = compute_distribution(&[(a, 50), (b, 50)], total_reward);

        assert_eq!(dist[&a], total_reward / 2);
        assert_eq!(dist[&b], total_reward / 2);
    }

    /// Conservation: total distributed must equal total_reward (within rounding).
    /// The implementation uses f64 rounding so we allow at most N satoshi of slack
    /// where N is the number of distinct contributors.
    #[test]
    fn reward_conservation_two_contributors() {
        let a = make_addr(0x01);
        let b = make_addr(0x02);
        let total_reward = 7 * WRF_UNITS_PER_COIN + 3; // odd amount to stress rounding
        let dist = compute_distribution(&[(a, 3), (b, 7)], total_reward);

        let distributed: u64 = dist.values().sum();
        let n_contributors = dist.len() as u64;
        // Allow ≤ n_contributors base-unit rounding drift
        let diff = (distributed as i64 - total_reward as i64).unsigned_abs();
        assert!(
            diff <= n_contributors,
            "conservation: distributed={distributed} total_reward={total_reward} diff={diff}"
        );
    }

    /// Conservation holds with many contributors (100).
    #[test]
    fn reward_conservation_many_contributors() {
        let weights: Vec<(Address, u64)> = (1u8..=100)
            .map(|i| (make_addr(i), i as u64))
            .collect();
        let total_reward = 1_000 * WRF_UNITS_PER_COIN;
        let dist = compute_distribution(&weights, total_reward);

        let distributed: u64 = dist.values().sum();
        let n_contributors = dist.len() as u64;
        let diff = (distributed as i64 - total_reward as i64).unsigned_abs();
        assert!(
            diff <= n_contributors,
            "conservation with 100 contributors: diff={diff} > n_contributors={n_contributors}"
        );
    }

    /// Zero-reward edge case: every contributor receives zero.
    #[test]
    fn reward_zero_total_reward() {
        let a = make_addr(0x01);
        let b = make_addr(0x02);
        let dist = compute_distribution(&[(a, 10), (b, 20)], 0);
        assert_eq!(*dist.get(&a).unwrap_or(&0), 0);
        assert_eq!(*dist.get(&b).unwrap_or(&0), 0);
    }

    /// A single contributor with weight=1 still receives the full reward.
    #[test]
    fn reward_single_contributor_weight_one() {
        let addr = make_addr(0xAA);
        let total_reward = WRF_UNITS_PER_COIN; // 1 WRF
        let dist = compute_distribution(&[(addr, 1)], total_reward);
        assert_eq!(dist[&addr], total_reward);
    }

    /// Proportional: contributor with 3x the weight gets ~3x the reward.
    #[test]
    fn reward_proportional_weights() {
        let a = make_addr(0x01);
        let b = make_addr(0x02);
        let total_reward = 400 * WRF_UNITS_PER_COIN;
        // a has 3x the weight of b
        let dist = compute_distribution(&[(a, 300), (b, 100)], total_reward);

        let a_reward = dist[&a];
        let b_reward = dist[&b];
        // a should get ~300 WRF, b ~100 WRF
        let expected_a = 300 * WRF_UNITS_PER_COIN;
        let expected_b = 100 * WRF_UNITS_PER_COIN;
        assert!(
            (a_reward as i64 - expected_a as i64).abs() <= 1,
            "a_reward={a_reward} expected={expected_a}"
        );
        assert!(
            (b_reward as i64 - expected_b as i64).abs() <= 1,
            "b_reward={b_reward} expected={expected_b}"
        );
    }

    // ---------------------------------------------------------------------------
    // WRF_UNITS_PER_COIN constant
    // ---------------------------------------------------------------------------

    #[test]
    fn wrf_units_per_coin_is_one_hundred_million() {
        assert_eq!(WRF_UNITS_PER_COIN, 100_000_000u64);
    }
}
