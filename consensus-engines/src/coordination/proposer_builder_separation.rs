//! Proposer-Builder Separation (PBS)
//!
//! Separates the roles of block proposing and block building. Builders compete
//! to construct the most valuable block, submitting bids to a relay. The proposer
//! selects the winning bid and commits to the block header without seeing the
//! transactions, preventing censorship and MEV extraction by proposers.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use rustc_hash::FxHashMap;

/// Errors that can occur in PBS operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum PbsError {
    #[error("No bids available")]
    NoBids,

    #[error("Invalid bid: {0}")]
    InvalidBid(String),

    #[error("Builder not registered: {0}")]
    BuilderNotRegistered(String),

    #[error("Slot already assigned: {0}")]
    SlotAlreadyAssigned(u64),

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Bid value too low: {value}, minimum: {minimum}")]
    BidTooLow { value: u64, minimum: u64 },
}

/// Configuration for the PBS system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbsConfig {
    /// Minimum bid value accepted by the relay
    pub min_bid_value: u64,
    /// Maximum number of builders
    pub max_builders: usize,
    /// Bid submission deadline (seconds before slot start)
    pub bid_deadline_secs: u64,
    /// Whether to require builder collateral
    pub require_collateral: bool,
    /// Minimum builder collateral amount
    pub min_collateral: u64,
}

impl Default for PbsConfig {
    fn default() -> Self {
        Self {
            min_bid_value: 0,
            max_builders: 256,
            bid_deadline_secs: 4,
            require_collateral: false,
            min_collateral: 0,
        }
    }
}

/// A builder's bid for constructing a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderBid {
    /// Public key of the builder
    pub builder_pubkey: String,
    /// Value offered to the proposer (in base units)
    pub value: u64,
    /// Blinded block header (hash of the full block)
    pub header: BlindedBlockHeader,
    /// Signature over the bid by the builder
    pub signature: String,
    /// Timestamp when the bid was submitted
    pub timestamp: u64,
    /// Slot this bid is for
    pub slot: u64,
}

/// Blinded block header that hides transaction details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindedBlockHeader {
    /// Hash of the complete block (transactions hidden)
    pub block_hash: String,
    /// Parent block hash
    pub parent_hash: String,
    /// State root after applying transactions
    pub state_root: String,
    /// Gas used by the block
    pub gas_used: u64,
    /// Gas limit for the block
    pub gas_limit: u64,
    /// Timestamp of the block
    pub timestamp: u64,
    /// Transaction count (revealed, but not the transactions themselves)
    pub transaction_count: u32,
}

/// A blinded block that the proposer signs without seeing transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindedBlock {
    /// The blinded header
    pub header: BlindedBlockHeader,
    /// Builder who constructed this block
    pub builder_pubkey: String,
    /// Proposer's signature committing to this header
    pub proposer_signature: Option<String>,
    /// Value paid to the proposer
    pub proposer_payment: u64,
}

/// Duty assignment for a proposer at a specific slot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposerDuty {
    /// Slot number this proposer is assigned to
    pub slot: u64,
    /// Validator index of the proposer
    pub validator_index: u64,
    /// Public key of the proposer
    pub proposer_pubkey: String,
}

/// Information about a registered builder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderInfo {
    /// Builder public key
    pub pubkey: String,
    /// Collateral deposited
    pub collateral: u64,
    /// Number of blocks successfully built
    pub blocks_built: u64,
    /// Whether the builder is active
    pub is_active: bool,
    /// Builder's reliability score (0.0 to 1.0)
    pub reliability_score: f64,
}

/// MEV-Boost relay that connects proposers with builders
#[derive(Debug, Clone)]
pub struct MevBoostRelay {
    /// Registered builders
    builders: FxHashMap<String, BuilderInfo>,
    /// Current bids indexed by slot
    bids: FxHashMap<u64, Vec<BuilderBid>>,
    /// Proposer duties indexed by slot
    duties: FxHashMap<u64, ProposerDuty>,
    /// Relay configuration
    config: PbsConfig,
}

impl MevBoostRelay {
    /// Create a new relay with default configuration.
    pub fn new() -> Self {
        Self {
            builders: FxHashMap::default(),
            bids: FxHashMap::default(),
            duties: FxHashMap::default(),
            config: PbsConfig::default(),
        }
    }

    /// Create a new relay with custom configuration.
    pub fn with_config(config: PbsConfig) -> Self {
        Self {
            builders: FxHashMap::default(),
            bids: FxHashMap::default(),
            duties: FxHashMap::default(),
            config,
        }
    }

    /// Register a builder with the relay.
    pub fn register_builder(&mut self, pubkey: String, collateral: u64) -> Result<(), PbsError> {
        if self.builders.len() >= self.config.max_builders {
            return Err(PbsError::InvalidBid("Maximum builders reached".to_string()));
        }

        if self.config.require_collateral && collateral < self.config.min_collateral {
            return Err(PbsError::BidTooLow {
                value: collateral,
                minimum: self.config.min_collateral,
            });
        }

        self.builders.insert(
            pubkey.clone(),
            BuilderInfo {
                pubkey,
                collateral,
                blocks_built: 0,
                is_active: true,
                reliability_score: 1.0,
            },
        );

        Ok(())
    }

    /// Submit a bid from a builder.
    pub fn submit_bid(&mut self, bid: BuilderBid) -> Result<(), PbsError> {
        // Verify builder is registered
        if !self.builders.contains_key(&bid.builder_pubkey) {
            return Err(PbsError::BuilderNotRegistered(bid.builder_pubkey.clone()));
        }

        // Validate bid
        if !validate_builder_bid(&bid, &self.config) {
            return Err(PbsError::InvalidBid("Bid validation failed".to_string()));
        }

        self.bids.entry(bid.slot).or_default().push(bid);

        Ok(())
    }

    /// Assign a proposer duty for a slot.
    pub fn assign_proposer_duty(&mut self, duty: ProposerDuty) -> Result<(), PbsError> {
        if self.duties.contains_key(&duty.slot) {
            return Err(PbsError::SlotAlreadyAssigned(duty.slot));
        }
        self.duties.insert(duty.slot, duty);
        Ok(())
    }

    /// Get the winning bid for a slot.
    ///
    /// Returns the highest-value valid bid, or None if no bids exist.
    pub fn get_winning_bid(&self, slot: u64) -> Option<&BuilderBid> {
        self.bids
            .get(&slot)
            .and_then(|bids| select_winning_bid(bids))
    }

    /// Get a blinded block for the proposer to sign.
    ///
    /// The proposer commits to the block header without seeing the transactions.
    pub fn get_blinded_block(&self, slot: u64) -> Option<BlindedBlock> {
        let winning_bid = self.get_winning_bid(slot)?;

        Some(BlindedBlock {
            header: winning_bid.header.clone(),
            builder_pubkey: winning_bid.builder_pubkey.clone(),
            proposer_signature: None,
            proposer_payment: winning_bid.value,
        })
    }

    /// Record that a builder successfully produced a block.
    pub fn record_successful_build(&mut self, builder_pubkey: &str) {
        if let Some(builder) = self.builders.get_mut(builder_pubkey) {
            builder.blocks_built += 1;
        }
    }

    /// Get all bids for a slot.
    pub fn get_bids(&self, slot: u64) -> &[BuilderBid] {
        self.bids.get(&slot).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get builder info.
    pub fn get_builder(&self, pubkey: &str) -> Option<&BuilderInfo> {
        self.builders.get(pubkey)
    }

    /// Get proposer duty for a slot.
    pub fn get_duty(&self, slot: u64) -> Option<&ProposerDuty> {
        self.duties.get(&slot)
    }

    /// Clear bids for completed slots up to (not including) the given slot.
    pub fn clear_old_bids(&mut self, up_to_slot: u64) {
        self.bids.retain(|&slot, _| slot >= up_to_slot);
        self.duties.retain(|&slot, _| slot >= up_to_slot);
    }

    /// Get the relay configuration.
    pub fn config(&self) -> &PbsConfig {
        &self.config
    }
}

impl Default for MevBoostRelay {
    fn default() -> Self {
        Self::new()
    }
}

/// Select the winning bid from a set of bids.
///
/// The winning bid is the one with the highest value. Ties are broken
/// by earliest timestamp.
pub fn select_winning_bid(bids: &[BuilderBid]) -> Option<&BuilderBid> {
    if bids.is_empty() {
        return None;
    }

    bids.iter().max_by(|a, b| {
        a.value
            .cmp(&b.value)
            .then_with(|| b.timestamp.cmp(&a.timestamp)) // earlier timestamp wins ties
    })
}

/// Validate a builder bid.
///
/// Checks that the bid has valid structure, non-empty signature,
/// and meets minimum value requirements.
pub fn validate_builder_bid(bid: &BuilderBid, config: &PbsConfig) -> bool {
    // Check minimum value
    if bid.value < config.min_bid_value {
        return false;
    }

    // Check signature is present
    if bid.signature.is_empty() {
        return false;
    }

    // Check builder pubkey is present
    if bid.builder_pubkey.is_empty() {
        return false;
    }

    // Check header has valid hashes
    if bid.header.block_hash.is_empty() || bid.header.parent_hash.is_empty() {
        return false;
    }

    // Check gas constraints
    if bid.header.gas_used > bid.header.gas_limit {
        return false;
    }

    true
}

/// Proposer-Builder Separation coordinator.
///
/// High-level interface for the PBS system combining relay management
/// with proposer duties.
#[derive(Debug, Clone)]
pub struct ProposerBuilderSeparation {
    relay: MevBoostRelay,
}

impl ProposerBuilderSeparation {
    /// Create a new PBS coordinator with default configuration.
    pub fn new() -> Self {
        Self {
            relay: MevBoostRelay::new(),
        }
    }

    /// Create a new PBS coordinator with custom configuration.
    pub fn with_config(config: PbsConfig) -> Self {
        Self {
            relay: MevBoostRelay::with_config(config),
        }
    }

    /// Get a mutable reference to the relay.
    pub fn relay_mut(&mut self) -> &mut MevBoostRelay {
        &mut self.relay
    }

    /// Get a reference to the relay.
    pub fn relay(&self) -> &MevBoostRelay {
        &self.relay
    }

    /// Create a signed blinded block for a proposer.
    ///
    /// The proposer signs the blinded header, committing to it without
    /// knowing the transaction contents.
    pub fn sign_blinded_block(&self, slot: u64, proposer_pubkey: &str) -> Option<BlindedBlock> {
        let mut blinded = self.relay.get_blinded_block(slot)?;

        // Create a commitment signature (simplified: hash of header + proposer key)
        let signature = {
            let mut hasher = Sha256::new();
            hasher.update(b"wrfcoin-pbs-commit-v1|");
            hasher.update(proposer_pubkey.as_bytes());
            hasher.update(blinded.header.block_hash.as_bytes());
            hasher.update(slot.to_be_bytes());
            hex::encode(hasher.finalize())
        };

        blinded.proposer_signature = Some(signature);
        Some(blinded)
    }
}

impl Default for ProposerBuilderSeparation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bid(builder: &str, value: u64, slot: u64) -> BuilderBid {
        BuilderBid {
            builder_pubkey: builder.to_string(),
            value,
            header: BlindedBlockHeader {
                block_hash: format!("blockhash-{}-{}", builder, slot),
                parent_hash: "parent-hash-abc".to_string(),
                state_root: "state-root-def".to_string(),
                gas_used: 1_000_000,
                gas_limit: 30_000_000,
                timestamp: 1000 + slot,
                transaction_count: 100,
            },
            signature: format!("sig-{}", builder),
            timestamp: 1000 + slot,
            slot,
        }
    }

    fn make_duty(slot: u64) -> ProposerDuty {
        ProposerDuty {
            slot,
            validator_index: 42,
            proposer_pubkey: "proposer-key-1".to_string(),
        }
    }

    #[test]
    fn test_select_winning_bid_empty() {
        assert!(select_winning_bid(&[]).is_none());
    }

    #[test]
    fn test_select_winning_bid_single() {
        let bids = vec![make_bid("builder-1", 100, 1)];
        let winner = select_winning_bid(&bids).unwrap();
        assert_eq!(winner.builder_pubkey, "builder-1");
    }

    #[test]
    fn test_select_winning_bid_highest_value() {
        let bids = vec![
            make_bid("builder-1", 100, 1),
            make_bid("builder-2", 500, 1),
            make_bid("builder-3", 200, 1),
        ];
        let winner = select_winning_bid(&bids).unwrap();
        assert_eq!(winner.builder_pubkey, "builder-2");
        assert_eq!(winner.value, 500);
    }

    #[test]
    fn test_select_winning_bid_tiebreak_by_timestamp() {
        let mut bid1 = make_bid("builder-1", 100, 1);
        bid1.timestamp = 500; // earlier
        let mut bid2 = make_bid("builder-2", 100, 1);
        bid2.timestamp = 600; // later

        let bids = vec![bid2, bid1];
        let winner = select_winning_bid(&bids).unwrap();
        assert_eq!(winner.builder_pubkey, "builder-1"); // earlier timestamp wins
    }

    #[test]
    fn test_validate_builder_bid_valid() {
        let bid = make_bid("builder-1", 100, 1);
        let config = PbsConfig::default();
        assert!(validate_builder_bid(&bid, &config));
    }

    #[test]
    fn test_validate_builder_bid_below_min_value() {
        let bid = make_bid("builder-1", 50, 1);
        let config = PbsConfig {
            min_bid_value: 100,
            ..Default::default()
        };
        assert!(!validate_builder_bid(&bid, &config));
    }

    #[test]
    fn test_validate_builder_bid_empty_signature() {
        let mut bid = make_bid("builder-1", 100, 1);
        bid.signature = String::new();
        let config = PbsConfig::default();
        assert!(!validate_builder_bid(&bid, &config));
    }

    #[test]
    fn test_validate_builder_bid_empty_pubkey() {
        let mut bid = make_bid("builder-1", 100, 1);
        bid.builder_pubkey = String::new();
        let config = PbsConfig::default();
        assert!(!validate_builder_bid(&bid, &config));
    }

    #[test]
    fn test_validate_builder_bid_gas_overflow() {
        let mut bid = make_bid("builder-1", 100, 1);
        bid.header.gas_used = 50_000_000;
        bid.header.gas_limit = 30_000_000;
        let config = PbsConfig::default();
        assert!(!validate_builder_bid(&bid, &config));
    }

    #[test]
    fn test_validate_builder_bid_empty_block_hash() {
        let mut bid = make_bid("builder-1", 100, 1);
        bid.header.block_hash = String::new();
        let config = PbsConfig::default();
        assert!(!validate_builder_bid(&bid, &config));
    }

    #[test]
    fn test_relay_register_builder() {
        let mut relay = MevBoostRelay::new();
        assert!(relay.register_builder("builder-1".to_string(), 0).is_ok());
        assert!(relay.get_builder("builder-1").is_some());
    }

    #[test]
    fn test_relay_register_builder_collateral_required() {
        let config = PbsConfig {
            require_collateral: true,
            min_collateral: 1000,
            ..Default::default()
        };
        let mut relay = MevBoostRelay::with_config(config);
        let result = relay.register_builder("builder-1".to_string(), 500);
        assert!(result.is_err());

        let result = relay.register_builder("builder-1".to_string(), 1000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_relay_submit_bid() {
        let mut relay = MevBoostRelay::new();
        relay.register_builder("builder-1".to_string(), 0).unwrap();

        let bid = make_bid("builder-1", 100, 1);
        assert!(relay.submit_bid(bid).is_ok());
        assert_eq!(relay.get_bids(1).len(), 1);
    }

    #[test]
    fn test_relay_submit_bid_unregistered_builder() {
        let mut relay = MevBoostRelay::new();
        let bid = make_bid("unknown-builder", 100, 1);
        let result = relay.submit_bid(bid);
        assert!(matches!(result, Err(PbsError::BuilderNotRegistered(_))));
    }

    #[test]
    fn test_relay_winning_bid() {
        let mut relay = MevBoostRelay::new();
        relay.register_builder("builder-1".to_string(), 0).unwrap();
        relay.register_builder("builder-2".to_string(), 0).unwrap();

        relay.submit_bid(make_bid("builder-1", 100, 5)).unwrap();
        relay.submit_bid(make_bid("builder-2", 200, 5)).unwrap();

        let winner = relay.get_winning_bid(5).unwrap();
        assert_eq!(winner.builder_pubkey, "builder-2");
        assert_eq!(winner.value, 200);
    }

    #[test]
    fn test_relay_no_bids_for_slot() {
        let relay = MevBoostRelay::new();
        assert!(relay.get_winning_bid(1).is_none());
    }

    #[test]
    fn test_relay_blinded_block() {
        let mut relay = MevBoostRelay::new();
        relay.register_builder("builder-1".to_string(), 0).unwrap();
        relay.submit_bid(make_bid("builder-1", 300, 10)).unwrap();

        let blinded = relay.get_blinded_block(10).unwrap();
        assert_eq!(blinded.builder_pubkey, "builder-1");
        assert_eq!(blinded.proposer_payment, 300);
        assert!(blinded.proposer_signature.is_none());
    }

    #[test]
    fn test_relay_assign_proposer_duty() {
        let mut relay = MevBoostRelay::new();
        let duty = make_duty(1);
        assert!(relay.assign_proposer_duty(duty).is_ok());

        let result = relay.assign_proposer_duty(make_duty(1));
        assert!(matches!(result, Err(PbsError::SlotAlreadyAssigned(1))));
    }

    #[test]
    fn test_relay_record_successful_build() {
        let mut relay = MevBoostRelay::new();
        relay.register_builder("builder-1".to_string(), 0).unwrap();
        relay.record_successful_build("builder-1");

        let builder = relay.get_builder("builder-1").unwrap();
        assert_eq!(builder.blocks_built, 1);
    }

    #[test]
    fn test_relay_clear_old_bids() {
        let mut relay = MevBoostRelay::new();
        relay.register_builder("builder-1".to_string(), 0).unwrap();
        relay.submit_bid(make_bid("builder-1", 100, 1)).unwrap();
        relay.submit_bid(make_bid("builder-1", 200, 5)).unwrap();
        relay.submit_bid(make_bid("builder-1", 300, 10)).unwrap();

        relay.clear_old_bids(5);

        assert!(relay.get_bids(1).is_empty());
        assert_eq!(relay.get_bids(5).len(), 1);
        assert_eq!(relay.get_bids(10).len(), 1);
    }

    #[test]
    fn test_pbs_sign_blinded_block() {
        let mut pbs = ProposerBuilderSeparation::new();
        pbs.relay_mut()
            .register_builder("builder-1".to_string(), 0)
            .unwrap();
        pbs.relay_mut()
            .submit_bid(make_bid("builder-1", 100, 1))
            .unwrap();

        let blinded = pbs.sign_blinded_block(1, "proposer-key-abc").unwrap();
        assert!(blinded.proposer_signature.is_some());
        assert_eq!(blinded.proposer_payment, 100);
    }

    #[test]
    fn test_pbs_sign_blinded_block_no_bids() {
        let pbs = ProposerBuilderSeparation::new();
        let result = pbs.sign_blinded_block(1, "proposer-key-abc");
        assert!(result.is_none());
    }

    #[test]
    fn test_pbs_default_config() {
        let pbs = ProposerBuilderSeparation::new();
        assert_eq!(pbs.relay().config().max_builders, 256);
        assert_eq!(pbs.relay().config().bid_deadline_secs, 4);
    }

    #[test]
    fn test_blinded_block_hides_transactions() {
        // Verify that BlindedBlock does not contain transaction data
        let blinded = BlindedBlock {
            header: BlindedBlockHeader {
                block_hash: "hash".to_string(),
                parent_hash: "parent".to_string(),
                state_root: "state".to_string(),
                gas_used: 1000,
                gas_limit: 30000,
                timestamp: 100,
                transaction_count: 50,
            },
            builder_pubkey: "builder".to_string(),
            proposer_signature: None,
            proposer_payment: 100,
        };

        // Only the count is visible, not the actual transactions
        assert_eq!(blinded.header.transaction_count, 50);
    }

    #[test]
    fn test_multiple_builders_multiple_slots() {
        let mut relay = MevBoostRelay::new();
        relay.register_builder("b1".to_string(), 0).unwrap();
        relay.register_builder("b2".to_string(), 0).unwrap();
        relay.register_builder("b3".to_string(), 0).unwrap();

        // Slot 1: b1 wins
        relay.submit_bid(make_bid("b1", 500, 1)).unwrap();
        relay.submit_bid(make_bid("b2", 300, 1)).unwrap();

        // Slot 2: b3 wins
        relay.submit_bid(make_bid("b2", 200, 2)).unwrap();
        relay.submit_bid(make_bid("b3", 400, 2)).unwrap();

        assert_eq!(relay.get_winning_bid(1).unwrap().builder_pubkey, "b1");
        assert_eq!(relay.get_winning_bid(2).unwrap().builder_pubkey, "b3");
    }
}
