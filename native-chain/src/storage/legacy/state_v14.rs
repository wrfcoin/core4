//! Frozen format14 at S4 72a1ef882. Never edit this retired layout.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Deserialize;
use wrfcoin_geo_grid::GeoCell;
use wrfcoin_shared_protocol::accounts::authorization::AccountAuthorizationDeclaration;
use wrfcoin_shared_protocol::accounts::paymaster::PaymasterAccount;
use wrfcoin_shared_protocol::accounts::recovery::AccountRecoveryState;
use wrfcoin_shared_protocol::accounts::session_keys::OwnerSessionKeys;
use wrfcoin_shared_protocol::blockchain::state::{
    Account, AuditEventRecord, CalibrationAttestationRecord, ContributionRewardRecord,
    GovernanceState, MiningPoolRecord, QualityPolicyEventArchiveBucket, QualityPolicyEventRecord,
};
use wrfcoin_shared_protocol::common::types::{Address, Hash, NFTId};
use wrfcoin_shared_protocol::consensus::hyper_local::OracleLocalAccuracy;
use wrfcoin_shared_protocol::consensus::producer_set::ProducerSetChange;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::EmissionPartitionLedger;
use wrfcoin_shared_protocol::contributions::commit_reveal::{
    CommitRecord, CommitRevealState, RevealRecord,
};
use wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig;
use wrfcoin_shared_protocol::governance::schedule_extension::AppliedExtension;
use wrfcoin_shared_protocol::nft::computational_nft::ComputationalNFT;
use wrfcoin_shared_protocol::oracle::attestation::{
    AttestationSet, AttesterSettlementDebt, AttesterSettlementRecord, OracleRegistry,
    ProducerSettlementRecord,
};
use wrfcoin_shared_protocol::security::key_registry::SigningKeyRegistry;
use wrfcoin_shared_protocol::security::slashing::SlashingReason;
use wrfcoin_shared_protocol::station_registry::StationRegistry;
use wrfcoin_shared_protocol::weather::validation::WeatherObservation;

/// The format-11 `State`, field for field, in serialization order.
#[derive(Deserialize)]
pub struct State {
    pub accounts: HashMap<Address, Account>,
    pub nfts: HashMap<NFTId, ComputationalNFT>,
    pub pending_commits: HashMap<String, CommitRecord>,
    pub pending_reveals: HashMap<String, RevealRecord>,
    pub contribution_reward_records: BTreeMap<String, ContributionRewardRecord>,
    pub finalized_contribution_rewards: BTreeSet<String>,
    pub oracle_set: HashSet<String>,
    pub oracle_quorum: usize,
    pub attestations: HashMap<String, AttestationSet>,
    pub privacy_commitments: HashSet<String>,
    pub privacy_commitment_amounts: HashMap<String, u128>,
    pub privacy_commitments_height: HashMap<String, u64>,
    pub privacy_spent_key_images: HashSet<String>,
    pub privacy_spent_key_images_height: HashMap<String, u64>,
    pub privacy_spent_nullifiers: HashSet<String>,
    pub privacy_spent_nullifiers_height: HashMap<String, u64>,
    pub privacy_disclosure_ids: HashSet<String>,
    pub privacy_disclosure_ids_height: HashMap<String, u64>,
    pub spent_oracle_nonces: HashSet<String>,
    pub spent_oracle_nonces_height: HashMap<String, u64>,
    pub reward_claim_key_images: HashSet<String>,
    pub reward_claim_key_images_height: HashMap<String, u64>,
    pub last_state_pruning_height: u64,
    pub feeless_worker_epoch_counts: HashMap<String, u32>,
    pub governance: GovernanceState,
    pub producer_set_history: Vec<ProducerSetChange>,
    pub low_quality_streaks: HashMap<Address, u32>,
    pub quality_policy_events: Vec<QualityPolicyEventRecord>,
    pub audit_events: Vec<AuditEventRecord>,
    pub quality_policy_event_archive: Vec<QualityPolicyEventArchiveBucket>,
    pub archived_quality_policy_event_count: u64,
    pub archived_quality_policy_event_through_event_id: Option<u64>,
    pub mining_pools: HashMap<String, MiningPoolRecord>,
    pub state_root: Hash,
    pub signing_keys: SigningKeyRegistry,
    pub calibration_attestations: HashMap<String, CalibrationAttestationRecord>,
    pub station_registry: StationRegistry,
    pub station_counts: BTreeMap<GeoCell, u32>,
    pub recent_weather_observations: BTreeMap<String, (WeatherObservation, u64)>,
    // determinism-ok: frozen format-8 decode mirror; bincode-deserialized and immediately migrated, never used in arithmetic
    pub spatial_scores: HashMap<String, f64>,
    pub oracle_local_accuracy: BTreeMap<(String, GeoCell), OracleLocalAccuracy>,
    /// Private in the live struct; serialized nonetheless.
    pub commit_reveal_state: CommitRevealState,
    /// Private in the live struct; serialized nonetheless.
    pub oracle_registry: OracleRegistry,
    pub zk_verified_proofs: HashMap<String, u64>,
    pub supply_genesis: u128,
    pub supply_authorized_minting: u128,
    pub supply_authorized_burning: u128,
    pub supply_baseline_initialized: bool,
    pub shielded_pool: u128,
    pub supply_violation: Option<String>,
    pub slashing_reason_counts: HashMap<SlashingReason, u64>,
    pub slashing_evidence_seen: HashSet<[u8; 32]>,
    pub slashing_evidence_seen_height: HashMap<[u8; 32], u64>,
    pub gamification_claims_seen: HashSet<String>,
    pub emission_window_start: u64,
    pub emission_window_spent: HashMap<Address, u128>,
    pub attester_settlement_due: BTreeMap<String, AttesterSettlementDebt>,
    pub attester_settlements: BTreeMap<String, AttesterSettlementRecord>,
    pub emission_ledger: EmissionPartitionLedger,
    pub genesis_emission_config: Option<GenesisEmissionConfig>,
    pub last_producer_settlement: Option<ProducerSettlementRecord>,
    pub account_recovery: BTreeMap<Address, AccountRecoveryState>,
    pub session_keys: BTreeMap<Address, OwnerSessionKeys>,
    pub authorization_declarations: BTreeMap<Address, AccountAuthorizationDeclaration>,
    pub paymasters: BTreeMap<Address, PaymasterAccount>,
    pub feature_schedule_extensions: Vec<AppliedExtension>,
    pub station_weather_accepted_block_unix: BTreeMap<String, i64>,
    pub station_owner_ledger: BTreeMap<
        String,
        wrfcoin_shared_protocol::blockchain::station_owner::StationOwnerLedgerEntry,
    >,
    pub node_bond_registry:
        Option<wrfcoin_shared_protocol::consensus::node_bond_registry::NodeBondRegistry>,
    pub node_receipt_registry:
        Option<wrfcoin_shared_protocol::consensus::node_receipt_registry::NodeReceiptRegistry>,
    pub node_settlement:
        Option<wrfcoin_shared_protocol::consensus::node_settlement::NodeSettlement>,
}
