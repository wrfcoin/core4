//! Fresh testnet genesis policy and legacy replay compatibility controls.
use wrfcoin_shared_protocol::{
    blockchain::{block::Block, state::{State, StateTransition}},
    common::types::{Address, Balance},
    consensus::proof_of_publishing::{self as pop, EmissionRole},
    crypto::{chain_binding::*, hash::Hash},
    governance::schedule_extension::{authority_from_genesis, install_schedule_extension_authority_from_block1},
};

const SCHEDULE: &str = "0:v1,2:schedule_extension,3:reward_gross_outlier_guard,4:weather_skew_block_time_allowance,5:station_active_reward_gate,6:station_owner_actions,7:station_owner_consent,8:station_registration_nonce,9:node_share_settlement";
fn genesis() -> Block {
    let mut b = Block::new(3, Hash::default(), vec![], 1, 1);
    b.timestamp = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    for (k, v) in [
        (GENESIS_CHAIN_ID_KEY, "888"), (GENESIS_NETWORK_ID_KEY, "testnet"),
        (GENESIS_PROTOCOL_VERSION_KEY, "v1"), (GENESIS_ACTIVATION_SCHEDULE_KEY, SCHEDULE),
        (GENESIS_FAUCET_ADDRESS_KEY, "0x2323232323232323232323232323232323232323"),
        (GENESIS_FLAT_REWARD_KEY, "600"), (GENESIS_FLAT_REWARD_UNITS_KEY, "2222222222"), (GENESIS_FAUCET_ALLOCATION_KEY, "1000000"),
        (GENESIS_GOVERNANCE_KEYS_KEY, "e7bec166ad446314d26fb7526bfa8cec1b363b438440262fb001f7b04b20bf00"), // pragma: allowlist secret -- existing public governance key
        (GENESIS_NODE_EPOCH_KEY, "512"), (GENESIS_NODE_CHALLENGE_KEY, "512"),
    ] { b.metadata.insert(k.into(), v.into()); }
    b
}

#[test]
fn v1_schedule_and_recorded_authority_accept_888_and_reject_tampering() {
    let b = genesis();
    validate_testnet_genesis(&b).unwrap();
    let protocol = resolve_uncut_chain_protocol_with_schedule(Some(SigningProtocolVersion::V1), &ChainIdentity::new("888", "testnet"), Some(SCHEDULE)).unwrap();
    assert_eq!(protocol.version, SigningProtocolVersion::V1);
    assert!(resolve_uncut_chain_protocol_with_schedule(Some(SigningProtocolVersion::V1), &ChainIdentity::new("888", "testnet"), None).is_err());
    let h = b.calculate_hash().to_hex();
    let authority = authority_from_genesis(&h, Some(&b)).unwrap();
    assert_eq!(authority.chain_id(), "888");
    assert_eq!(authority.network_id(), "testnet");
    assert_eq!(authority.node_timing(), Some((512, 512)));
    let mut changed = b.clone();
    changed.metadata.insert(GENESIS_GOVERNANCE_KEYS_KEY.into(), "11".repeat(32));
    assert!(authority_from_genesis(&h, Some(&changed)).is_none());
    for (key, value) in [
        (GENESIS_ACTIVATION_SCHEDULE_KEY, ""),
        (GENESIS_ACTIVATION_SCHEDULE_KEY, "0:v1,1:node_share_settlement"),
        (GENESIS_FLAT_REWARD_KEY, "601"), (GENESIS_FAUCET_ALLOCATION_KEY, "-1"),
        (GENESIS_FAUCET_ALLOCATION_KEY, "99999999999999999999999999999999999999999"),
        (GENESIS_GOVERNANCE_KEYS_KEY, "not-a-key"), (GENESIS_NODE_EPOCH_KEY, "0"),
        (GENESIS_FAUCET_ADDRESS_KEY, "0x0000000000000000000000000000000000000000"),
    ] {
        let mut bad = b.clone(); bad.metadata.insert(key.into(), value.into());
        assert!(validate_testnet_genesis(&bad).is_err(), "{key}={value}");
    }
}

#[test]
fn allocation_is_once_and_flat_reward_uses_recorded_network() {
    let b = genesis(); let config = GenesisEmissionConfig::from_block(&b).unwrap();
    let sink = config.faucet_address.unwrap();
    let mut s = State::new();
    let record = StateTransition::RecordGenesisEmissionConfig { config: config.clone() };
    s.apply_transitions_batch_at_height(1, vec![record.clone()]).unwrap();
    assert_eq!(s.get_account(&sink).unwrap().balance, Balance::new(1_000_000));
    let root = s.state_root_material();
    s.apply_transitions_batch_at_height(1, vec![record]).unwrap();
    assert_eq!(s.state_root_material(), root);
    let mut altered = config.clone(); altered.policy.as_mut().unwrap().faucet_allocation += 1;
    assert!(s.apply_transitions_batch_at_height(1, vec![StateTransition::RecordGenesisEmissionConfig { config: altered }]).is_err());
    for h in [0, 1, 99, 150, 25_000, 100_000, 1_000_000] {
        let p = config.emission_partition(h);
        assert_eq!(p.emission(), 2_222_222_222);
        assert!(p.slice(EmissionRole::Faucet) > 0);
        assert!(pop::genesis_coinbase_allowance(h, Some(&config)) <= p.slice(EmissionRole::Contribution));
    }
    let mut later = b.clone(); later.height = 2;
    assert!(GenesisEmissionConfig::from_block(&later).is_none());
    let mut other = b; other.metadata.insert(GENESIS_CHAIN_ID_KEY.into(), "889".into());
    assert_eq!(GenesisEmissionConfig::from_block(&other).unwrap().emission_partition(1), config.emission_partition(1));
    other.metadata.insert(GENESIS_NETWORK_ID_KEY.into(), "mainnet".into());
    assert_eq!(GenesisEmissionConfig::from_block(&other).unwrap().emission_partition(1).slice(EmissionRole::Faucet), 0);
}

#[test]
fn legacy_777_config_serialization_and_emission_are_unchanged() {
    let mut b = genesis(); b.metadata.insert(GENESIS_CHAIN_ID_KEY.into(), "777".into());
    b.metadata.remove(GENESIS_ACTIVATION_SCHEDULE_KEY);
    let config = GenesisEmissionConfig::from_block(&b).unwrap();
    assert!(config.policy.is_none());
    let json = serde_json::to_value(&config).unwrap();
    assert_eq!(json.as_object().unwrap().len(), 2);
    for h in [0, 1, 999, 1000, 25_000, 50_000, 100_000, 1_000_000] {
        assert_eq!(config.emission_partition(h), pop::EmissionPartition::at_height(h, Some("777")));
        assert_eq!(pop::genesis_coinbase_allowance(h, Some(&config)), pop::coinbase_allowance(h, Some("777")));
    }
    validate_testnet_genesis(&b).unwrap();
}

#[test]
fn node_pay_888_closes_epoch_and_crosses_recorded_challenge_window() {
    let b = genesis();
    install_protocol_from_block1(&b).unwrap();
    let h = b.calculate_hash().to_hex();
    install_schedule_extension_authority_from_block1(&h, Some(&b)).unwrap();
    let mut state = State::new();
    state.apply_transitions_batch_at_height(1, vec![StateTransition::RecordGenesisEmissionConfig { config: GenesisEmissionConfig::from_block(&b).unwrap() }]).unwrap();
    for height in 2..=1040 {
        state.open_emission_block(height);
        state.begin_node_service_block(height, [height as u8; 32]).unwrap();
        state.apply_block_finalization(height);
        assert!(state.supply_violation.is_none(), "height {height}: {:?}", state.supply_violation);
        if [519, 520, 1032, 1033, 1040].contains(&height) {
            use wrfcoin_shared_protocol::consensus::node_settlement::EpochStatus;
            let epoch = &state.node_settlement.as_ref().unwrap().epochs[&0];
            assert_eq!(epoch.maturity_height, 1033);
            assert_eq!(epoch.status, if height == 519 { EpochStatus::Open } else { EpochStatus::Unissued });
            assert!(epoch.payouts.is_empty());
            if height >= 520 { assert_eq!(epoch.unissued, epoch.scheduled); }
        }
    }
    assert!(state.node_bond_registry.is_some());
    use wrfcoin_shared_protocol::consensus::{node_share_contracts::*, node_bond_registry::*};
    assert_eq!(node_epoch_blocks(), 512);
    assert_eq!(node_challenge_blocks(Some("888")), 512);
    assert_eq!(node_challenge_blocks(Some("777")), 1024);
    assert_eq!(epoch_at(9, 520), Some(0));
    assert_eq!(epoch_at(9, 521), Some(1));
    assert_eq!(epoch_close(9, 0), Some(520));
}

#[test]
fn legacy_and_new_policy_survive_positional_checkpoint_and_snapshot_codecs() {
    for chain in ["777", "888"] {
        let mut b = genesis(); b.metadata.insert(GENESIS_CHAIN_ID_KEY.into(), chain.into());
        let config = GenesisEmissionConfig::from_block(&b).unwrap();
        // The sentinel immediately after the nested config catches an extra
        // field consuming the next positional checkpoint field.
        let packet = (config.clone(), 0x12345678u64);
        let bytes = bincode::serialize(&packet).unwrap();
        let decoded: (GenesisEmissionConfig, u64) = bincode::deserialize(&bytes).unwrap();
        assert_eq!(packet, decoded);
        if chain == "777" {
            #[derive(serde::Serialize)]
            struct Old { chain_id: String, faucet_address: Option<Address> }
            let old = (Old { chain_id: chain.into(), faucet_address: config.faucet_address }, 0x12345678u64);
            assert_eq!(bytes, bincode::serialize(&old).unwrap());
        }
        let mut state = State::new();
        state.apply_transitions_batch_at_height(1, vec![StateTransition::RecordGenesisEmissionConfig { config }]).unwrap();
        let bytes = bincode::serialize(&state).unwrap();
        let restored: State = bincode::deserialize(&bytes).unwrap();
        assert_eq!(state.state_root_material(), restored.state_root_material());
        use wrfcoin_shared_protocol::blockchain::state_snapshot::{encode_snapshot_value, decode_snapshot_value};
        let snapshot = encode_snapshot_value(&state).unwrap();
        let restored: State = decode_snapshot_value(&snapshot).unwrap();
        assert_eq!(state.state_root_material(), restored.state_root_material());
    }
}

#[test]
fn new_chain_governance_signatures_bind_recorded_keys_and_genesis() {
    use wrfcoin_shared_protocol::{crypto::signature::{PrivateKey, PublicKey}, governance::schedule_extension::*};
    let key = PrivateKey::from_bytes(&[91; 32]).unwrap(); // synthetic test key
    let public = PublicKey::from_private_key(&key).to_hex();
    let mut b = genesis(); b.metadata.insert(GENESIS_GOVERNANCE_KEYS_KEY.into(), public.clone());
    let hash = b.calculate_hash().to_hex();
    let authority = authority_from_genesis(&hash, Some(&b)).unwrap();
    let extension = FeatureScheduleExtension {
        chain_id: "888".into(), network_id: "testnet".into(), genesis_hash: hash,
        sequence: 0, entries: vec![ScheduleEntry::new("station_owner_slash_identity", 5000)],
        version_steps: vec![], min_protocol_version: 3, release_commit: "synthetic-test-release".into(),
    };
    let mut signed = SignedFeatureScheduleExtension {
        signatures: vec![GovernanceSignature { signer_pubkey: public, signature: key.sign(&extension.preimage()).to_hex() }], extension,
    };
    verify_feature_schedule_extension(&signed, &authority.context()).unwrap();
    signed.extension.chain_id = "777".into();
    assert!(verify_feature_schedule_extension(&signed, &authority.context()).is_err());
    signed.extension.chain_id = "888".into();
    signed.extension.genesis_hash = "ab".repeat(32);
    assert!(verify_feature_schedule_extension(&signed, &authority.context()).is_err());
}

#[test]
fn an_explicit_flat_budget_can_match_777_after_its_first_reduction() {
    assert_eq!(pop::adjusted_block_reward(43_000), 2_222_222_222);
    assert_eq!(pop::adjusted_block_reward(49_709), 2_222_222_222);
    let mut b = genesis(); b.metadata.insert(GENESIS_FLAT_REWARD_UNITS_KEY.into(), "2222222222".into());
    validate_testnet_genesis(&b).unwrap();
    let config = GenesisEmissionConfig::from_block(&b).unwrap();
    for h in [0, 1, 150, 43_000, 100_000] { assert_eq!(config.emission_partition(h).emission(), 2_222_222_222); }
    for amount in ["0", "3333333334", "-1", "02222222222"] {
        b.metadata.insert(GENESIS_FLAT_REWARD_UNITS_KEY.into(), amount.into());
        assert!(validate_testnet_genesis(&b).is_err());
    }
}

#[tokio::test]
async fn missing_explicit_rate_is_rejected_at_every_policy_boundary() {
    let mut b = genesis();
    b.metadata.remove(GENESIS_FLAT_REWARD_UNITS_KEY);
    assert!(validate_testnet_genesis(&b).is_err());
    assert!(pop::ProofOfPublishing::check_block_one_genesis_faucet(&b).is_err());
    let mut verifier = pop::ProofOfPublishing::new(1, 600, 0, 100);
    assert!(!verifier.verify_block(&b, &Default::default(), 0, &[]).await.unwrap());
    let invalid = GenesisEmissionConfig::from_block(&b).unwrap();
    let mut state = State::new();
    assert!(state.apply_transitions_batch_at_height(1, vec![
        StateTransition::RecordGenesisEmissionConfig { config: invalid.clone() }
    ]).is_err());
    let wire = bincode::serialize(&invalid).unwrap();
    assert!(bincode::deserialize::<GenesisEmissionConfig>(&wire).is_err());
    for amount in ["2222222222", "3333333333"] {
        b.metadata.insert(GENESIS_FLAT_REWARD_UNITS_KEY.into(), amount.into());
        validate_testnet_genesis(&b).unwrap();
        assert_eq!(GenesisEmissionConfig::from_block(&b).unwrap().emission_partition(1).emission(), amount.parse::<u128>().unwrap());
    }
}
