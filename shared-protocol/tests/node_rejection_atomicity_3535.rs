#[path = "common/node_bond_fixture.rs"]
mod fixture;

use wrfcoin_shared_protocol::{
    blockchain::{
        state::{State, StateTransition},
        transaction::{
            ConfidentialTransferPayload, PrivacyLevel, REWARD_CLAIM_EPOCH_BLOCKS,
            RewardClaimPayload, ShieldedDepositPayload, ShieldedWithdrawalPayload,
        },
    },
    common::types::Balance,
    consensus::node_share_contracts::NodeBondAction,
    station_registry::{ContributionProvenanceClass, StationLocation, WeatherStation},
};

fn funded() -> State {
    let mut state = fixture::state();
    fixture::apply(
        &mut state,
        fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1),
        fixture::A,
    )
    .unwrap();
    assert_eq!(
        state
            .node_bond_registry
            .as_ref()
            .unwrap()
            .registrations
            .len(),
        1
    );
    state
}

fn deposit() -> StateTransition {
    StateTransition::ApplyShieldedDeposit {
        from: fixture::address(20),
        payload: ShieldedDepositPayload {
            shielded_commitment: "11".repeat(32),
            range_proof: "22".repeat(16),
            transparent_amount: 7,
        },
        current_block: fixture::A,
    }
}

fn withdrawal(recipient: String) -> StateTransition {
    StateTransition::ApplyShieldedWithdrawal {
        payload: ShieldedWithdrawalPayload {
            nullifier: "33".repeat(16),
            shielded_commitment: "11".repeat(32),
            range_proof: "22".repeat(16),
            authorization_proof: "44".repeat(16),
            transparent_recipient: recipient,
            transparent_amount: 7,
            selective_disclosure: None,
        },
        current_block: fixture::A,
    }
}

fn rejected_unchanged(state: &mut State, transition: StateTransition, expected: &str) {
    let before = bincode::serialize(&*state).unwrap();
    let material = state.state_root_material();
    let root = state.state_root;
    let error = state.apply_transition(transition).unwrap_err();
    assert!(
        error.to_string().contains(expected),
        "unexpected rejection: {error}"
    );
    assert_eq!(state.state_root, root, "rejected transition changed root");
    assert!(
        state.state_root_material() == material,
        "rejected transition changed root material"
    );
    assert!(
        bincode::serialize(&*state).unwrap() == before,
        "rejected transition changed serialized state, accounts, escrow or locks"
    );
}

fn confidential(keys: Vec<String>, commitments: Vec<String>) -> StateTransition {
    StateTransition::ApplyConfidentialTransfer {
        payload: ConfidentialTransferPayload {
            output_commitments: commitments,
            range_proof: Some("33".repeat(32)),
            stealth_data: None,
            ring_signature: None,
            key_images: keys,
            encrypted_payload: None,
            privacy_level: PrivacyLevel::Standard,
            selective_disclosures: vec![],
            signed_message: None,
        },
        current_block: fixture::A,
    }
}

#[test]
fn funded_confidential_lists_reject_without_publishing_any_entry() {
    use curve25519_dalek::{constants::RISTRETTO_BASEPOINT_POINT, scalar::Scalar};
    use wrfcoin_privacy::ring_signatures::KeyImage;
    let image = |n| {
        hex::encode(
            KeyImage::generate(&Scalar::from(n).to_bytes())
                .unwrap()
                .to_bytes(),
        )
    };
    let commitment = |n| {
        hex::encode(
            (Scalar::from(n) * RISTRETTO_BASEPOINT_POINT)
                .compress()
                .to_bytes(),
        )
    };
    let fresh = image(101u64);
    let spent = image(102u64);
    let first = commitment(201u64);
    let second = commitment(202u64);
    let existing = commitment(203u64);
    let cases = [
        (
            vec![fresh.clone()],
            vec![first.clone(), first.clone()],
            "DUPLICATE_COMMITMENT",
        ),
        (
            vec![fresh.clone(), fresh.clone()],
            vec![first.clone()],
            "KEY_IMAGE_DOUBLE_SPEND",
        ),
        (
            vec![fresh.clone(), "not-hex".into()],
            vec![first.clone()],
            "KEY_IMAGE_INVALID",
        ),
        (
            vec![fresh.clone()],
            vec![first.clone(), "not-hex".into()],
            "COMMITMENT_INVALID",
        ),
        (
            vec![fresh.clone(), spent.clone()],
            vec![first.clone()],
            "KEY_IMAGE_DOUBLE_SPEND",
        ),
        (
            vec![fresh.clone()],
            vec![first.clone(), existing.clone()],
            "DUPLICATE_COMMITMENT",
        ),
    ];
    for (keys, outputs, error) in cases {
        let mut state = funded();
        // Populate both sets through a valid transition, including their height records.
        state
            .apply_transition(confidential(vec![spent.clone()], vec![existing.clone()]))
            .unwrap();
        rejected_unchanged(&mut state, confidential(keys, outputs), error);
        // Fresh entries from the rejected prefix must still be usable.
        state
            .apply_transition(confidential(
                vec![fresh.clone()],
                vec![first.clone(), second.clone()],
            ))
            .unwrap();
        assert_eq!(state.privacy_spent_key_images.len(), 2);
        assert_eq!(state.privacy_commitments.len(), 3);
    }
}

#[test]
fn funded_account_late_rejections_preserve_every_byte() {
    let mut state = funded();
    state
        .get_account_mut(&fixture::address(20))
        .unwrap()
        .stake_locked = Balance::new(u128::MAX);
    rejected_unchanged(
        &mut state,
        StateTransition::LockToStake {
            address: fixture::address(20),
            amount: Balance::new(1),
        },
        "Arithmetic overflow",
    );
}

#[test]
fn funded_insufficient_slash_preserves_every_byte() {
    let mut state = funded();
    let balance = state.get_account(&fixture::address(20)).unwrap().balance;
    rejected_unchanged(
        &mut state,
        StateTransition::Slash {
            address: fixture::address(20),
            amount: balance + Balance::new(1),
            reason: "synthetic atomicity control".into(),
        },
        "Insufficient balance",
    );
}

#[test]
fn funded_rejected_oracle_registration_preserves_every_byte() {
    let mut state = funded();
    rejected_unchanged(
        &mut state,
        StateTransition::ApplyOracleRegistration {
            oracle_set: vec![hex::encode(fixture::pk(20))],
            oracle_quorum: 1,
            identity: hex::encode(fixture::pk(30)),
            public_key: hex::encode(fixture::pk(30)),
            epoch: 0,
            activated_at: 0,
        },
        "not in the registered oracle_set",
    );
}

#[test]
fn funded_invalid_withdrawal_recipient_preserves_every_byte() {
    let mut state = funded();
    state.apply_transition(deposit()).unwrap();
    rejected_unchanged(
        &mut state,
        withdrawal("invalid-recipient".into()),
        "SP_PRIVACY_SHIELDED_WITHDRAWAL_RECIPIENT_INVALID",
    );
    // The rejection must not consume the spend: the same commitment/nullifier
    // must remain usable by the successful control.
    state
        .apply_transition(withdrawal(fixture::address(20).to_string()))
        .unwrap();
    assert_eq!(state.shielded_pool, 0);
    assert!(!state.privacy_commitments.contains(&"11".repeat(32)));
    assert!(state.privacy_spent_nullifiers.contains(&"33".repeat(16)));
}

#[test]
fn funded_withdrawal_pool_and_recipient_overflows_preserve_every_byte() {
    for overflow_recipient in [false, true] {
        let mut state = funded();
        state.apply_transition(deposit()).unwrap();
        if overflow_recipient {
            state
                .accounts
                .get_mut(&fixture::address(20))
                .unwrap()
                .spendable_balance = Balance::new(u128::MAX);
        } else {
            state.shielded_pool = 0;
        }
        state.recalculate_state_root();
        rejected_unchanged(
            &mut state,
            withdrawal(fixture::address(20).to_string()),
            if overflow_recipient {
                "overflow"
            } else {
                "SP_PRIVACY_SHIELDED_WITHDRAWAL_EXCEEDS_POOL"
            },
        );
    }
}

#[test]
fn funded_duplicate_and_overflowing_deposit_preserve_every_byte() {
    let mut state = funded();
    state.apply_transition(deposit()).unwrap();
    rejected_unchanged(
        &mut state,
        deposit(),
        "SP_PRIVACY_SHIELDED_DEPOSIT_DUPLICATE_COMMITMENT",
    );
    let mut state = funded();
    state.shielded_pool = u128::MAX;
    state.recalculate_state_root();
    rejected_unchanged(&mut state, deposit(), "overflow");
}

fn reward_claim(state: &mut State) -> RewardClaimPayload {
    use wrfcoin_privacy::ring_signatures::{KeyImage, MLSAGSignature, RingMember};
    use wrfcoin_privacy::{Scalar, scalar_to_public_key};
    let pairs: Vec<_> = (1..=8_u64)
        .map(|n| {
            let scalar = Scalar::from(n);
            (scalar.to_bytes(), scalar_to_public_key(&scalar))
        })
        .collect();
    for (i, (_, public)) in pairs.iter().enumerate() {
        state
            .station_registry
            .register(WeatherStation {
                station_id: format!("synthetic-ring-{i}"),
                owner_address: fixture::address(20).to_string(),
                location: StationLocation {
                    latitude: 0.,
                    longitude: 0.,
                    elevation_meters: None,
                },
                hardware_model: "synthetic".into(),
                calibration_date: None,
                is_active: true,
                reputation_score: 50,
                data_quality_tier: Default::default(),
                provenance_class: ContributionProvenanceClass::Real,
                ring_public_key: Some(hex::encode(public)),
            })
            .unwrap();
    }
    let mut payload = RewardClaimPayload {
        epoch: fixture::A / REWARD_CLAIM_EPOCH_BLOCKS,
        ring_signature: String::new(),
        key_image: hex::encode(KeyImage::generate(&pairs[0].0).unwrap().to_bytes()),
        amount_commitment: "ab".repeat(32),
        range_proof: "ff".repeat(32),
        destination: fixture::address(20).to_string(),
    };
    let decoys: Vec<_> = pairs[1..]
        .iter()
        .map(|(_, public)| RingMember {
            public_key: *public,
        })
        .collect();
    let signature =
        MLSAGSignature::sign(&payload.authorization_message(), &pairs[0].0, &decoys).unwrap();
    payload.ring_signature = hex::encode(serde_json::to_vec(&signature).unwrap());
    state.recalculate_state_root();
    payload
}

fn claim(payload: RewardClaimPayload) -> StateTransition {
    StateTransition::ApplyRewardClaim {
        payload,
        current_block: fixture::A,
    }
}

#[test]
fn funded_invalid_and_duplicate_claim_preserve_every_byte() {
    let mut state = funded();
    let valid = reward_claim(&mut state);
    let mut invalid = valid.clone();
    invalid.ring_signature = "not-hex".into();
    rejected_unchanged(
        &mut state,
        claim(invalid),
        "SP_PRIVACY_REWARD_CLAIM_RING_SIG_INVALID",
    );
    let mut invalid = valid.clone();
    invalid.destination = fixture::address(30).to_string();
    rejected_unchanged(
        &mut state,
        claim(invalid),
        "SP_PRIVACY_REWARD_CLAIM_SIG_VERIFY_FAILED",
    );
    state.apply_transition(claim(valid.clone())).unwrap();
    assert!(
        state
            .reward_claim_key_images
            .contains(&format!("{}:{}", valid.epoch, valid.key_image))
    );
    assert!(state.privacy_commitments.contains(&valid.amount_commitment));
    rejected_unchanged(
        &mut state,
        claim(valid),
        "SP_PRIVACY_REWARD_CLAIM_DOUBLE_CLAIM",
    );
}

#[test]
fn funded_early_node_withdrawal_preserves_every_byte() {
    use wrfcoin_shared_protocol::consensus::{
        node_bond_registry::{BondExitBoundary, epoch_at},
        node_share_contracts::{NODE_EPOCH_BLOCKS, TESTNET_CHALLENGE_BLOCKS},
    };
    let mut state = funded();
    fixture::advance(&mut state, fixture::A + NODE_EPOCH_BLOCKS);
    fixture::apply(
        &mut state,
        fixture::auth(10, 1, 1, NodeBondAction::Exit, 2),
        fixture::A + NODE_EPOCH_BLOCKS,
    )
    .unwrap();
    let boundary = BondExitBoundary::checked(
        fixture::A,
        fixture::A + 2 * NODE_EPOCH_BLOCKS,
        TESTNET_CHALLENGE_BLOCKS,
    )
    .unwrap();
    let early = boundary.earliest_withdrawal_height - 1;
    fixture::advance(&mut state, early);
    rejected_unchanged(
        &mut state,
        StateTransition::ApplyNodeBondLifecycle {
            payload: fixture::auth(
                10,
                1,
                2,
                NodeBondAction::Withdraw,
                epoch_at(fixture::A, early).unwrap(),
            ),
            block_height: early,
        },
        "SP_NODE_BOND_REFUSED",
    );
    let ready = boundary.earliest_withdrawal_height;
    fixture::advance(&mut state, ready);
    let supply = state.total_minted_supply();
    fixture::apply(
        &mut state,
        fixture::auth(
            10,
            1,
            2,
            NodeBondAction::Withdraw,
            epoch_at(fixture::A, ready).unwrap(),
        ),
        ready,
    )
    .unwrap();
    assert_eq!(state.total_minted_supply(), supply);
    assert!(
        state
            .node_bond_registry
            .as_ref()
            .unwrap()
            .registrations
            .is_empty()
    );
    assert_eq!(
        state.accounts[&fixture::address(10)].balance.0,
        u128::from(wrfcoin_shared_protocol::consensus::node_bond_registry::NODE_BOND_UNITS) * 3
    );
}

#[test]
#[ignore = "bounded N32 timing; run explicitly"]
fn atomicity_escrow_guard_timing_cap32() {
    use std::{hint::black_box, time::Instant};
    use wrfcoin_shared_protocol::consensus::node_bond_registry::node_bond_escrow_address;
    let mut state = funded();
    let prototype = state.node_bond_registry.as_ref().unwrap().registrations[&[1; 32]].clone();
    for i in 2u32..=32 {
        let mut id = [0; 32];
        id[..4].copy_from_slice(&i.to_le_bytes());
        let mut record = prototype.clone();
        record.binding.deployment_id = id;
        record.binding.escrow_account = node_bond_escrow_address(&id);
        state.accounts.insert(
            record.binding.escrow_account,
            state.accounts[&prototype.binding.escrow_account].clone(),
        );
        state
            .node_bond_registry
            .as_mut()
            .unwrap()
            .registrations
            .insert(id, record);
    }
    for trial in 0..5 {
        let start = Instant::now();
        for _ in 0..100 {
            black_box(state.apply_transition(StateTransition::LockToStake {
                address: fixture::address(20),
                amount: Balance::new(0),
            }))
            .unwrap();
        }
        println!(
            "ESCROW_GUARD_TIMING N=32 transitions=100 trial={} elapsed_us={}",
            trial,
            start.elapsed().as_micros()
        );
    }
}

// Exhaustive enum match: a new wire variant cannot silently escape this sweep.
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

fn sweep_metadata() -> wrfcoin_shared_protocol::common::types::NFTMetadata {
    use wrfcoin_shared_protocol::common::types::NFTMetadata;
    NFTMetadata {
        name: "synthetic atomicity sweep".into(),
        description: None,
        data_type: "synthetic".into(),
        region: None,
        accuracy: None,
        creation_time: wrfcoin_shared_protocol::common::types::Timestamp::new(0),
        creator: fixture::address(20),
        metadata_uri: None,
        properties: serde_json::json!({}),
    }
}

fn sweep_nft(id: &str) -> wrfcoin_shared_protocol::blockchain::state::ComputationalNFT {
    use wrfcoin_shared_protocol::{blockchain::state::ComputationalNFT, common::types::NFTId};
    ComputationalNFT {
        id: NFTId::new(id.into()),
        owner: fixture::address(20),
        creator: fixture::address(20),
        metadata: sweep_metadata(),
        price: None,
        verified: false,
        verification_count: 0,
        avg_confidence: None,
        created_at: 0,
        updated_at: 0,
    }
}

fn sweep_transition(ordinal: u32) -> StateTransition {
    use wrfcoin_shared_protocol::{
        blockchain::{
            state::GovernanceParameterUpdate,
            transaction::{Transaction, TransactionType},
        },
        common::types::NFTId,
        security::key_registry::SigningRole,
    };
    let owner = fixture::address(20);
    match ordinal {
        0 => StateTransition::BalanceTransfer {
            from: owner,
            to: fixture::address(30),
            amount: Balance::new(1),
            nonce: Some(0),
            current_block: fixture::A,
        },
        1 => StateTransition::LockToStake {
            address: owner,
            amount: Balance::new(1),
        },
        2 => StateTransition::ReleaseToSpendable {
            address: owner,
            amount: Balance::new(1),
        },
        3 => StateTransition::ApplyReward {
            address: owner,
            amount: Balance::new(1),
            reason: "synthetic".into(),
        },
        9 => StateTransition::ApplyReveal {
            payload: serde_json::from_value(serde_json::json!({
                "commit_id":"missing", "station_id":"synthetic", "timestamp":0,
                "contribution_type":"weather", "data":{}, "nonce":"synthetic"
            }))
            .unwrap(),
            block_height: fixture::A,
            block_timestamp: 0,
        },
        10 => confidential(vec!["66".repeat(32)], vec!["11".repeat(32)]),
        11 => deposit(),
        15 => StateTransition::SetOracleConfig {
            oracle_set: vec![hex::encode(fixture::pk(20))],
            oracle_quorum: 1,
        },
        17 => StateTransition::ApplyOracleRegistration {
            oracle_set: vec![hex::encode(fixture::pk(20))],
            oracle_quorum: 1,
            identity: hex::encode(fixture::pk(20)),
            public_key: hex::encode(fixture::pk(20)),
            epoch: 1,
            activated_at: 0,
        },
        19 => StateTransition::RegisterSigningKeyEpoch {
            identity: hex::encode(fixture::pk(20)),
            role: SigningRole::Oracle,
            epoch: 1,
            public_key: hex::encode(fixture::pk(20)),
            activated_at: 0,
        },
        20 => StateTransition::RevokeSigningKeyEpoch {
            identity: hex::encode(fixture::pk(20)),
            role: SigningRole::Oracle,
            epoch: 0,
            revoked_at: 1,
        },
        21 => StateTransition::CreateGovernanceProposal {
            proposer: owner,
            proposal_id: "new-sweep".into(),
            update: GovernanceParameterUpdate::EmissionRate(1),
            voting_deadline: 100,
            deposit: Balance::new(
                wrfcoin_shared_protocol::common::constants::GOVERNANCE_DEFAULT_DEPOSIT,
            ),
            created_at_block: fixture::A,
            current_timestamp: 0,
        },
        22 => StateTransition::VoteGovernanceProposal {
            proposal_id: "sweep".into(),
            voter: owner,
            approve: false,
            voting_power: 1,
            current_block: fixture::A,
        },
        23 => StateTransition::FinalizeGovernanceProposal {
            proposal_id: "sweep".into(),
            current_block: 101,
            current_timestamp: 0,
        },
        24 | 25 => {
            let mut tx = Transaction::new(vec![], vec![]);
            tx.timestamp = chrono::DateTime::from_timestamp(0, 0).unwrap();
            tx.sender_address = owner;
            tx.tx_type = if ordinal == 24 {
                TransactionType::GovernanceVote
            } else {
                TransactionType::SlashingReport
            };
            tx.data = Some(serde_json::json!({"proposal_id":"sweep","approve":false}).to_string());
            if ordinal == 24 {
                StateTransition::ProcessGovernanceTransaction {
                    tx,
                    current_block: fixture::A,
                }
            } else {
                StateTransition::ProcessSlashingReport {
                    tx,
                    current_block: fixture::A,
                }
            }
        },
        29 => StateTransition::Slash {
            address: owner,
            amount: Balance::new(1),
            reason: "synthetic".into(),
        },
        30 => StateTransition::CreateNFT {
            nft: sweep_nft("new-sweep"),
        },
        31 => StateTransition::TransferNFT {
            nft_id: NFTId::new("sweep".into()),
            from: owner,
            to: fixture::address(30),
            updated_at: 1,
        },
        32 => StateTransition::UpdateNFT {
            nft_id: NFTId::new("sweep".into()),
            metadata: sweep_metadata(),
            updated_at: 1,
        },
        33 => StateTransition::VerifyNFT {
            nft_id: NFTId::new("sweep".into()),
            verifier: owner,
            confidence: 0.9,
            result: true,
            updated_at: 1,
        },
        35 => StateTransition::CreateMiningPool {
            id: "new-sweep".into(),
            name: "synthetic".into(),
            operator: owner.to_string(),
            fee_percentage: 0,
            created_at_block: fixture::A,
        },
        36 => StateTransition::JoinMiningPool {
            pool_id: "sweep".into(),
            member_address: fixture::address(30).to_string(),
        },
        37 => StateTransition::LeaveMiningPool {
            pool_id: "sweep".into(),
            member_address: owner.to_string(),
        },
        38 => StateTransition::SetLowQualityStreak {
            address: owner,
            streak: Some(1),
        },
        43 => StateTransition::ApplySessionKeyAction {
            action: wrfcoin_shared_protocol::accounts::session_keys::SessionKeyAction::Revoke {
                owner,
                session_key: fixture::address(30),
                owner_public_key: hex::encode(fixture::pk(20)),
                owner_signature: String::new(),
            },
            block_height: fixture::A,
            security_key_auth: None,
        },
        54 => StateTransition::BeginNodeServiceBlock {
            block_height: fixture::A + 1,
            parent_hash: [3; 32],
        },
        55 => StateTransition::ApplyNodeBondLifecycle {
            payload: fixture::auth(20, 2, 0, NodeBondAction::Deposit, 1),
            block_height: fixture::A,
        },
        _ => {
            // Synthetic zero-value fixtures, preserving the original sweep inputs.
            // StateTransition is not a protocol bincode boundary; keep fixtures JSON.
            let fixtures: serde_json::Value = serde_json::from_str(include_str!(
                "fixtures/node_rejection_sweep_3535.json"
            ))
            .unwrap();
            serde_json::from_value(fixtures[ordinal.to_string()].clone()).unwrap()
        },
    }
}

#[test]
fn funded_rejection_sweep_covers_every_transition_variant() {
    use wrfcoin_shared_protocol::blockchain::state::GovernanceParameterUpdate;
    let names = [
        "BalanceTransfer",
        "LockToStake",
        "ReleaseToSpendable",
        "ApplyReward",
        "ApplyContributionRewardDecision",
        "RejectInvalidContributionRewardMetadata",
        "ApplyGamificationReward",
        "ApplyAttesterSettlements",
        "RegisterCommit",
        "ApplyReveal",
        "ApplyConfidentialTransfer",
        "ApplyShieldedDeposit",
        "ApplyShieldedWithdrawal",
        "ApplyRewardClaim",
        "RecordOracleAttestation",
        "SetOracleConfig",
        "ApplyProducerSetChange",
        "ApplyOracleRegistration",
        "ApplyStationRegistration",
        "RegisterSigningKeyEpoch",
        "RevokeSigningKeyEpoch",
        "CreateGovernanceProposal",
        "VoteGovernanceProposal",
        "FinalizeGovernanceProposal",
        "ProcessGovernanceTransaction",
        "ProcessSlashingReport",
        "RecordAuditEvent",
        "RecordQualityPolicyEvent",
        "RecordOracleLocalAccuracy",
        "Slash",
        "CreateNFT",
        "TransferNFT",
        "UpdateNFT",
        "VerifyNFT",
        "RecordCalibrationAttestation",
        "CreateMiningPool",
        "JoinMiningPool",
        "LeaveMiningPool",
        "SetLowQualityStreak",
        "RecordGenesisEmissionConfig",
        "ApplyFaucetReward",
        "ApplyProducerSettlement",
        "ApplyAccountRecovery",
        "ApplySessionKeyAction",
        "DeclareAuthorization",
        "ApplyPaymasterAction",
        "ApplyFeatureScheduleExtension",
        "ApplyStationRegistrationWithOwnerConsent",
        "ApplyStationOwnerTransfer",
        "ApplyStationOwnerAction",
        "ApplyStationOwnerRecoveryFiling",
        "ApplyStationOwnerRecoveryCancel",
        "ApplyStationOwnerRecoveryExecute",
        "ApplyStationRegistrationV2",
        "BeginNodeServiceBlock",
        "ApplyNodeBondLifecycle",
        "ApplyNodeServiceBatch",
        "ApplyNodeFraudReport",
    ];
    let mut late_rejections = 0;
    for (ordinal, name) in names.into_iter().enumerate() {
        let mut state = funded();
        state
            .apply_transition(StateTransition::LockToStake {
                address: fixture::address(20),
                amount: Balance::new(7),
            })
            .unwrap();
        state
            .apply_transition(StateTransition::CreateNFT {
                nft: sweep_nft("sweep"),
            })
            .unwrap();
        state
            .apply_transition(StateTransition::CreateMiningPool {
                id: "sweep".into(),
                name: "synthetic".into(),
                operator: fixture::address(20).to_string(),
                fee_percentage: 0,
                created_at_block: fixture::A,
            })
            .unwrap();
        state
            .apply_transition(StateTransition::JoinMiningPool {
                pool_id: "sweep".into(),
                member_address: fixture::address(20).to_string(),
            })
            .unwrap();
        state
            .apply_transition(StateTransition::SetOracleConfig {
                oracle_set: vec![hex::encode(fixture::pk(20))],
                oracle_quorum: 1,
            })
            .unwrap();
        state
            .apply_transition(StateTransition::CreateGovernanceProposal {
                proposer: fixture::address(20),
                proposal_id: "sweep".into(),
                update: GovernanceParameterUpdate::EmissionRate(1),
                voting_deadline: 100,
                deposit: Balance::new(
                    wrfcoin_shared_protocol::common::constants::GOVERNANCE_DEFAULT_DEPOSIT,
                ),
                created_at_block: fixture::A,
                current_timestamp: 0,
            })
            .unwrap();
        let transition = sweep_transition(ordinal as u32);
        assert_eq!(transition_name(&transition), name, "variant inventory drift");
        // Force rejection at the common trailing cache refresh for variants
        // that otherwise accept. This exercises rollback AFTER successful
        // account and metadata writes, as well as each variant's own refusals.
        state.create_account(fixture::address(95), Balance::new(u128::MAX));
        state.recalculate_state_root();
        let bytes = bincode::serialize(&state).unwrap();
        let material = state.state_root_material();
        let root = state.state_root;
        let error = state.apply_transition(transition).expect_err(name);
        if matches!(
            error,
            wrfcoin_shared_protocol::blockchain::state::StateError::ArithmeticOverflow
        ) {
            late_rejections += 1;
        }
        assert_eq!(state.state_root, root, "{name}: cached root changed");
        assert!(
            state.state_root_material() == material,
            "{name}: root material changed"
        );
        assert!(
            bincode::serialize(&state).unwrap() == bytes,
            "{name}: serialized state changed"
        );
    }
    assert!(
        late_rejections >= 15,
        "sweep failed to reach the post-write rejection seam"
    );
    println!("FUNDED_REJECTION_SWEEP variants=58 late_arithmetic_rejections={late_rejections}");
}
