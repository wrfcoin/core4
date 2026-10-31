//! A common rejection journal for the funded-node apply seam.
//!
//! At most one undo record per State field (77), selected by an exhaustive
//! transition write inventory. Accounts save their persistent trie root, not
//! all accounts. Other records own the before-image of a touched subsystem;
//! untouched subsystems, notably the node registry on ordinary account paths,
//! are never copied. Whole hash-table before-images preserve bincode iteration
//! order even if a rejected insertion resized a table.
use super::{State, StateTransition};

type Undo = Box<dyn FnOnce(&mut State)>;
pub(super) struct TransitionUndo {
    records: [Option<Undo>; 77],
}

impl TransitionUndo {
    pub(super) fn capture(state: &State, transition: &StateTransition) -> Self {
        // No `..`: a new State field requires an explicit inventory decision.
        let State {
            accounts: _,
            nfts: _,
            pending_commits: _,
            pending_reveals: _,
            contribution_reward_records: _,
            finalized_contribution_rewards: _,
            oracle_set: _,
            oracle_quorum: _,
            attestations: _,
            privacy_commitments: _,
            privacy_commitment_amounts: _,
            privacy_commitments_height: _,
            privacy_spent_key_images: _,
            privacy_spent_key_images_height: _,
            privacy_spent_nullifiers: _,
            privacy_spent_nullifiers_height: _,
            privacy_disclosure_ids: _,
            privacy_disclosure_ids_height: _,
            spent_oracle_nonces: _,
            spent_oracle_nonces_height: _,
            reward_claim_key_images: _,
            reward_claim_key_images_height: _,
            last_state_pruning_height: _,
            feeless_worker_epoch_counts: _,
            governance: _,
            producer_set_history: _,
            low_quality_streaks: _,
            quality_policy_events: _,
            audit_events: _,
            quality_policy_event_archive: _,
            archived_quality_policy_event_count: _,
            archived_quality_policy_event_through_event_id: _,
            mining_pools: _,
            state_root: _,
            signing_keys: _,
            calibration_attestations: _,
            station_registry: _,
            station_counts: _,
            recent_weather_observations: _,
            spatial_scores: _,
            oracle_local_accuracy: _,
            commit_reveal_state: _,
            oracle_registry: _,
            zk_verified_proofs: _,
            supply_genesis: _,
            supply_authorized_minting: _,
            supply_authorized_burning: _,
            supply_baseline_initialized: _,
            shielded_pool: _,
            supply_violation: _,
            supply_invariant: _,
            zk_verifier: _,
            slashing_circuit_breaker: _,
            slashing_engine: _,
            deferred_slashing_triggers: _,
            slashing_reason_counts: _,
            slashing_evidence_seen: _,
            slashing_evidence_seen_height: _,
            gamification_claims_seen: _,
            emission_window_start: _,
            emission_window_spent: _,
            attester_settlement_due: _,
            attester_settlements: _,
            emission_ledger: _,
            genesis_emission_config: _,
            last_producer_settlement: _,
            account_recovery: _,
            session_keys: _,
            authorization_declarations: _,
            paymasters: _,
            feature_schedule_extensions: _,
            station_weather_accepted_block_unix: _,
            station_owner_ledger: _,
            node_bond_registry: _,
            node_receipt_registry: _,
            node_settlement: _,
            node_fraud: _,
        } = state;
        let fields = write_fields(transition);
        macro_rules! capture {
            ($field:ident) => {
                if COMMON.contains(&stringify!($field)) || fields.contains(&stringify!($field)) {
                    let before = state.$field.clone();
                    Some(Box::new(move |state: &mut State| state.$field = before) as Undo)
                } else {
                    None
                }
            };
        }
        let records = [
            capture!(accounts),
            capture!(nfts),
            capture!(pending_commits),
            capture!(pending_reveals),
            capture!(contribution_reward_records),
            capture!(finalized_contribution_rewards),
            capture!(oracle_set),
            capture!(oracle_quorum),
            capture!(attestations),
            capture!(privacy_commitments),
            capture!(privacy_commitment_amounts),
            capture!(privacy_commitments_height),
            capture!(privacy_spent_key_images),
            capture!(privacy_spent_key_images_height),
            capture!(privacy_spent_nullifiers),
            capture!(privacy_spent_nullifiers_height),
            capture!(privacy_disclosure_ids),
            capture!(privacy_disclosure_ids_height),
            capture!(spent_oracle_nonces),
            capture!(spent_oracle_nonces_height),
            capture!(reward_claim_key_images),
            capture!(reward_claim_key_images_height),
            capture!(last_state_pruning_height),
            capture!(feeless_worker_epoch_counts),
            capture!(governance),
            capture!(producer_set_history),
            capture!(low_quality_streaks),
            capture!(quality_policy_events),
            capture!(audit_events),
            capture!(quality_policy_event_archive),
            capture!(archived_quality_policy_event_count),
            capture!(archived_quality_policy_event_through_event_id),
            capture!(mining_pools),
            capture!(state_root),
            capture!(signing_keys),
            capture!(calibration_attestations),
            capture!(station_registry),
            capture!(station_counts),
            capture!(recent_weather_observations),
            capture!(spatial_scores),
            capture!(oracle_local_accuracy),
            capture!(commit_reveal_state),
            capture!(oracle_registry),
            capture!(zk_verified_proofs),
            capture!(supply_genesis),
            capture!(supply_authorized_minting),
            capture!(supply_authorized_burning),
            capture!(supply_baseline_initialized),
            capture!(shielded_pool),
            capture!(supply_violation),
            capture!(supply_invariant),
            capture!(zk_verifier),
            capture!(slashing_circuit_breaker),
            capture!(slashing_engine),
            capture!(deferred_slashing_triggers),
            capture!(slashing_reason_counts),
            capture!(slashing_evidence_seen),
            capture!(slashing_evidence_seen_height),
            capture!(gamification_claims_seen),
            capture!(emission_window_start),
            capture!(emission_window_spent),
            capture!(attester_settlement_due),
            capture!(attester_settlements),
            capture!(emission_ledger),
            capture!(genesis_emission_config),
            capture!(last_producer_settlement),
            capture!(account_recovery),
            capture!(session_keys),
            capture!(authorization_declarations),
            capture!(paymasters),
            capture!(feature_schedule_extensions),
            capture!(station_weather_accepted_block_unix),
            capture!(station_owner_ledger),
            capture!(node_bond_registry),
            capture!(node_receipt_registry),
            capture!(node_settlement),
            capture!(node_fraud),
        ];
        Self { records }
    }
    pub(super) fn restore(self, state: &mut State) {
        for record in self.records.into_iter().rev().flatten() {
            record(state);
        }
    }
}

const COMMON: &[&str] = &[
    "accounts",
    "state_root",
    "emission_ledger",
    "supply_authorized_minting",
    "supply_authorized_burning",
    "shielded_pool",
];
const PRIVACY: &[&str] = &[
    "privacy_commitments",
    "privacy_commitment_amounts",
    "privacy_commitments_height",
    "privacy_spent_key_images",
    "privacy_spent_key_images_height",
    "privacy_spent_nullifiers",
    "privacy_spent_nullifiers_height",
    "privacy_disclosure_ids",
    "privacy_disclosure_ids_height",
    "reward_claim_key_images",
    "reward_claim_key_images_height",
];
const ORACLE_CONFIG: &[&str] = &[
    "oracle_set",
    "oracle_quorum",
    "oracle_registry",
    "signing_keys",
    "governance",
];
const GOVERNANCE: &[&str] = &[
    "governance",
    "audit_events",
    "oracle_set",
    "oracle_quorum",
    "oracle_registry",
    "signing_keys",
    "commit_reveal_state",
    "quality_policy_events",
    "quality_policy_event_archive",
    "archived_quality_policy_event_count",
    "archived_quality_policy_event_through_event_id",
];
const PROTOCOL: &[&str] = &[
    // Sponsored first sends: a transfer carrying a sponsorship updates the
    // sponsor's rate-limit record once `paymaster` is active.
    "paymasters",
    "session_keys",
    "governance",
    // `SecurityKeyAction` (security_key_spend) writes the account's
    // declaration through the governance sink.
    "authorization_declarations",
    "audit_events",
    "oracle_set",
    "oracle_quorum",
    "oracle_registry",
    "signing_keys",
    "commit_reveal_state",
    "privacy_commitments",
    "privacy_commitment_amounts",
    "privacy_commitments_height",
    "privacy_spent_key_images",
    "privacy_spent_key_images_height",
    "privacy_spent_nullifiers",
    "privacy_spent_nullifiers_height",
    "privacy_disclosure_ids",
    "privacy_disclosure_ids_height",
    "reward_claim_key_images",
    "reward_claim_key_images_height",
    "slashing_evidence_seen",
    "slashing_evidence_seen_height",
    "slashing_reason_counts",
    "slashing_circuit_breaker",
    "slashing_engine",
    "deferred_slashing_triggers",
    "quality_policy_events",
    "quality_policy_event_archive",
    "archived_quality_policy_event_count",
    "archived_quality_policy_event_through_event_id",
];
const STATION: &[&str] = &["station_registry", "station_counts", "station_owner_ledger"];
const NODE: &[&str] = &[
    "node_bond_registry",
    "node_receipt_registry",
    "node_settlement",
    "node_fraud",
];

// This declares write footprints, not rejection reasons. Every failure from
// the unchecked path or the trailing supply-cache refresh uses the SAME undo.
fn write_fields(transition: &StateTransition) -> &'static [&'static str] {
    use StateTransition::*;
    match transition {
        BalanceTransfer { .. }
        | LockToStake { .. }
        | ReleaseToSpendable { .. }
        | ApplyReward { .. }
        | ApplyFaucetReward
        | RejectInvalidContributionRewardMetadata { .. }
        | Slash { .. } => &[],
        ApplyContributionRewardDecision { .. } => &[
            "contribution_reward_records",
            "finalized_contribution_rewards",
            "emission_window_start",
            "emission_window_spent",
        ],
        ApplyGamificationReward { .. } => &["gamification_claims_seen"],
        ApplyProducerSettlement { .. } => &["last_producer_settlement"],
        ApplyAttesterSettlements { .. } => &["attester_settlements", "attester_settlement_due"],
        RegisterCommit { .. } => &["commit_reveal_state", "pending_commits", "pending_reveals"],
        ApplyReveal { .. } => &[
            "commit_reveal_state",
            "pending_commits",
            "pending_reveals",
            "oracle_registry",
            "recent_weather_observations",
            "spatial_scores",
            "feeless_worker_epoch_counts",
            "station_weather_accepted_block_unix",
            "deferred_slashing_triggers",
            "slashing_circuit_breaker",
            "slashing_engine",
            "slashing_reason_counts",
            "zk_verified_proofs",
        ],
        ApplyConfidentialTransfer { .. }
        | ApplyShieldedDeposit { .. }
        | ApplyShieldedWithdrawal { .. }
        | ApplyRewardClaim { .. } => PRIVACY,
        RecordOracleAttestation { .. } => &[
            "oracle_registry",
            "attestations",
            "spent_oracle_nonces",
            "spent_oracle_nonces_height",
            "attester_settlement_due",
            "deferred_slashing_triggers",
            "slashing_circuit_breaker",
            "slashing_engine",
            "slashing_reason_counts",
        ],
        SetOracleConfig { .. } | ApplyOracleRegistration { .. } => ORACLE_CONFIG,
        ApplyProducerSetChange { .. } => &["producer_set_history"],
        RegisterSigningKeyEpoch { .. } | RevokeSigningKeyEpoch { .. } => &["signing_keys"],
        CreateGovernanceProposal { .. }
        | VoteGovernanceProposal { .. }
        | FinalizeGovernanceProposal { .. } => GOVERNANCE,
        ProcessGovernanceTransaction { .. } | ProcessSlashingReport { .. } => PROTOCOL,
        RecordAuditEvent { .. } => &["audit_events"],
        RecordQualityPolicyEvent { .. } => &[
            "quality_policy_events",
            "quality_policy_event_archive",
            "archived_quality_policy_event_count",
            "archived_quality_policy_event_through_event_id",
        ],
        RecordOracleLocalAccuracy { .. } => &["oracle_local_accuracy"],
        SetLowQualityStreak { .. } => &["low_quality_streaks"],
        CreateNFT { .. } | TransferNFT { .. } | UpdateNFT { .. } | VerifyNFT { .. } => &["nfts"],
        RecordCalibrationAttestation { .. } => &["calibration_attestations", "commit_reveal_state"],
        CreateMiningPool { .. } | JoinMiningPool { .. } | LeaveMiningPool { .. } => {
            &["mining_pools"]
        },
        RecordGenesisEmissionConfig { .. } => &["genesis_emission_config"],
        ApplyAccountRecovery { .. } => &["account_recovery"],
        ApplySessionKeyAction { .. } => &["session_keys"],
        DeclareAuthorization { .. } => &["authorization_declarations"],
        ApplyPaymasterAction { .. } => &["paymasters"],
        ApplyFeatureScheduleExtension { .. } => &["feature_schedule_extensions"],
        ApplyStationRegistration { .. }
        | ApplyStationRegistrationWithOwnerConsent { .. }
        | ApplyStationRegistrationV2 { .. }
        | ApplyStationOwnerTransfer { .. }
        | ApplyStationOwnerAction { .. }
        | ApplyStationOwnerRecoveryFiling { .. }
        | ApplyStationOwnerRecoveryCancel { .. }
        | ApplyStationOwnerRecoveryExecute { .. } => STATION,
        BeginNodeServiceBlock { .. }
        | ApplyNodeBondLifecycle { .. }
        | ApplyNodeServiceBatch { .. }
        | ApplyNodeFraudReport { .. } => NODE,
    }
}

// This oracle deliberately lives behind a compile-time gate: deployed release
// libraries retain the field journal and never build this diagnostic pre-image.
// Use exactly the canonical bytes consumed by State::recalculate_state_root;
// this diagnostic must not introduce another serialization boundary.
#[cfg(any(test, debug_assertions))]
pub(super) struct CompletenessGuard {
    bytes: Vec<u8>,
    root: super::Hash,
    variant: &'static str,
}

#[cfg(any(test, debug_assertions))]
impl CompletenessGuard {
    pub(super) fn capture(state: &State, transition: &StateTransition) -> Self {
        Self {
            bytes: state.state_root_material(),
            root: state.state_root,
            variant: transition_name(transition),
        }
    }

    #[inline(never)]
    pub(super) fn assert_restored(self, state: &State) {
        assert!(
            self.root == state.state_root
                && self.bytes == state.state_root_material(),
            "SP_TRANSITION_UNDO_INCOMPLETE: {}",
            self.variant,
        );
    }
}

#[cfg(any(test, debug_assertions))]
fn transition_name(t: &StateTransition) -> &'static str {
    match t {
        StateTransition::BalanceTransfer { .. } => "BalanceTransfer",
        StateTransition::LockToStake { .. } => "LockToStake",
        StateTransition::ReleaseToSpendable { .. } => "ReleaseToSpendable",
        StateTransition::ApplyReward { .. } => "ApplyReward",
        StateTransition::ApplyContributionRewardDecision { .. } => {
            "ApplyContributionRewardDecision"
        },
        StateTransition::RejectInvalidContributionRewardMetadata { .. } => {
            "RejectInvalidContributionRewardMetadata"
        },
        StateTransition::ApplyGamificationReward { .. } => "ApplyGamificationReward",
        StateTransition::ApplyAttesterSettlements { .. } => "ApplyAttesterSettlements",
        StateTransition::RegisterCommit { .. } => "RegisterCommit",
        StateTransition::ApplyReveal { .. } => "ApplyReveal",
        StateTransition::ApplyConfidentialTransfer { .. } => "ApplyConfidentialTransfer",
        StateTransition::ApplyShieldedDeposit { .. } => "ApplyShieldedDeposit",
        StateTransition::ApplyShieldedWithdrawal { .. } => "ApplyShieldedWithdrawal",
        StateTransition::ApplyRewardClaim { .. } => "ApplyRewardClaim",
        StateTransition::RecordOracleAttestation { .. } => "RecordOracleAttestation",
        StateTransition::SetOracleConfig { .. } => "SetOracleConfig",
        StateTransition::ApplyProducerSetChange { .. } => "ApplyProducerSetChange",
        StateTransition::ApplyOracleRegistration { .. } => "ApplyOracleRegistration",
        StateTransition::ApplyStationRegistration { .. } => "ApplyStationRegistration",
        StateTransition::RegisterSigningKeyEpoch { .. } => "RegisterSigningKeyEpoch",
        StateTransition::RevokeSigningKeyEpoch { .. } => "RevokeSigningKeyEpoch",
        StateTransition::CreateGovernanceProposal { .. } => "CreateGovernanceProposal",
        StateTransition::VoteGovernanceProposal { .. } => "VoteGovernanceProposal",
        StateTransition::FinalizeGovernanceProposal { .. } => "FinalizeGovernanceProposal",
        StateTransition::ProcessGovernanceTransaction { .. } => "ProcessGovernanceTransaction",
        StateTransition::ProcessSlashingReport { .. } => "ProcessSlashingReport",
        StateTransition::RecordAuditEvent { .. } => "RecordAuditEvent",
        StateTransition::RecordQualityPolicyEvent { .. } => "RecordQualityPolicyEvent",
        StateTransition::RecordOracleLocalAccuracy { .. } => "RecordOracleLocalAccuracy",
        StateTransition::Slash { .. } => "Slash",
        StateTransition::CreateNFT { .. } => "CreateNFT",
        StateTransition::TransferNFT { .. } => "TransferNFT",
        StateTransition::UpdateNFT { .. } => "UpdateNFT",
        StateTransition::VerifyNFT { .. } => "VerifyNFT",
        StateTransition::RecordCalibrationAttestation { .. } => "RecordCalibrationAttestation",
        StateTransition::CreateMiningPool { .. } => "CreateMiningPool",
        StateTransition::JoinMiningPool { .. } => "JoinMiningPool",
        StateTransition::LeaveMiningPool { .. } => "LeaveMiningPool",
        StateTransition::SetLowQualityStreak { .. } => "SetLowQualityStreak",
        StateTransition::RecordGenesisEmissionConfig { .. } => "RecordGenesisEmissionConfig",
        StateTransition::ApplyFaucetReward => "ApplyFaucetReward",
        StateTransition::ApplyProducerSettlement { .. } => "ApplyProducerSettlement",
        StateTransition::ApplyAccountRecovery { .. } => "ApplyAccountRecovery",
        StateTransition::ApplySessionKeyAction { .. } => "ApplySessionKeyAction",
        StateTransition::DeclareAuthorization { .. } => "DeclareAuthorization",
        StateTransition::ApplyPaymasterAction { .. } => "ApplyPaymasterAction",
        StateTransition::ApplyFeatureScheduleExtension { .. } => "ApplyFeatureScheduleExtension",
        StateTransition::ApplyStationRegistrationWithOwnerConsent { .. } => {
            "ApplyStationRegistrationWithOwnerConsent"
        },
        StateTransition::ApplyStationOwnerTransfer { .. } => "ApplyStationOwnerTransfer",
        StateTransition::ApplyStationOwnerAction { .. } => "ApplyStationOwnerAction",
        StateTransition::ApplyStationOwnerRecoveryFiling { .. } => {
            "ApplyStationOwnerRecoveryFiling"
        },
        StateTransition::ApplyStationOwnerRecoveryCancel { .. } => {
            "ApplyStationOwnerRecoveryCancel"
        },
        StateTransition::ApplyStationOwnerRecoveryExecute { .. } => {
            "ApplyStationOwnerRecoveryExecute"
        },
        StateTransition::ApplyStationRegistrationV2 { .. } => "ApplyStationRegistrationV2",
        StateTransition::BeginNodeServiceBlock { .. } => "BeginNodeServiceBlock",
        StateTransition::ApplyNodeBondLifecycle { .. } => "ApplyNodeBondLifecycle",
        StateTransition::ApplyNodeServiceBatch { .. } => "ApplyNodeServiceBatch",
        StateTransition::ApplyNodeFraudReport { .. } => "ApplyNodeFraudReport",
    }
}
