//! A fresh receiver must derive timing from its pinned genesis before decoding
//! bonded state. Source and receiver run in different processes: an authority
//! installed by the source fixture cannot accidentally make the receiver pass.
use super::*;
use crate::routes::snapshot::{
    GenesisExpectation, SnapshotSyncIngestRequest, TestGenesisPin, install_verified_snapshot,
    verify_and_apply_snapshot,
};
use wrfcoin_shared_protocol::{
    blockchain::{
        block::{Block, STATE_ROOT_METADATA_KEY},
        chain_snapshot::{export_block_entries, split_snapshot_entries, window_floor},
        snapshot_sync::{build_snapshot_sync_payload, build_state_snapshot, sign_snapshot_sync_payload},
        state_snapshot::export_state_entries,
        state::State,
    },
    common::types::Balance,
    consensus::{node_bond_registry::*, node_share_contracts::*},
    crypto::{chain_binding as cb, domain_signing::{DomainSeparated, sign_domain_for}},
    governance::schedule_extension::schedule_extension_authority,
};

const A: u64 = 9;
const SCHEDULE: &str = "0:v1,2:schedule_extension,3:reward_gross_outlier_guard,4:weather_skew_block_time_allowance,5:station_active_reward_gate,6:station_owner_actions,7:station_owner_consent,8:station_registration_nonce,9:node_share_settlement";
const SOURCE_OUTPUT: &str = "WRFCOIN_SNAPSHOT_AUTHORITY_SOURCE_OUTPUT";

fn key(byte: u8) -> PrivateKey { PrivateKey::from_bytes(&[byte; 32]).unwrap() }
fn pk(byte: u8) -> [u8; 32] {
    PublicKey::from_private_key(&key(byte)).as_bytes().try_into().unwrap()
}
fn owner(byte: u8) -> Address { Address::from_public_key(&PublicKey::from_private_key(&key(byte))) }
fn signature<T: DomainSeparated>(subject: &T, byte: u8, chain: &str) -> Vec<u8> {
    sign_domain_for(&key(byte), subject, &cb::ChainIdentity::new(chain, "testnet"),
        cb::SigningProtocolVersion::V2).as_bytes().to_vec()
}

fn deposit(owner_byte: u8, id: u8, genesis: [u8; 32], chain: &str) -> NodeBondLifecycle {
    let authorization = NodeBondAuthorization {
        genesis_hash: genesis, deployment_id: [id; 32], owner_account: owner(owner_byte),
        escrow_account: node_bond_escrow_address(&[id; 32]), payout_account: owner(owner_byte),
        server_key: pk(owner_byte + 1), consumer_key: pk(owner_byte + 2),
        service_alias: format!("synthetic-{id}"), action: NodeBondAction::Deposit,
        amount_units: NODE_BOND_UNITS,
        // The 888 faucet owner has already funded the second synthetic owner.
        expected_nonce: if chain == "888" && owner_byte == 10 { 1 } else { 0 }, effective_epoch: 1,
    };
    let digest = node_contract_digest(&authorization, &cb::ChainIdentity::new(chain, "testnet"));
    NodeBondLifecycle {
        signed: SignedNodeBondAuthorization {
            owner_signature: signature(&authorization, owner_byte, chain),
            server_signature: signature(&NodeBondPossession {
                authorization_preimage_hash: digest, role: NodeBondKeyRole::Server,
            }, owner_byte + 1, chain),
            consumer_signature: signature(&NodeBondPossession {
                authorization_preimage_hash: digest, role: NodeBondKeyRole::Consumer,
            }, owner_byte + 2, chain), authorization: authorization.clone(),
        },
        owner_key: pk(owner_byte), governance_key: Some(pk(99)),
        governance_signature: signature(&authorization, 99, chain),
        payout_key: Some(pk(owner_byte)), payout_signature: signature(&authorization, owner_byte, chain),
    }
}

fn block1(chain: &str) -> Block {
    let mut b = Block::new(3, Hash::default(), vec![], 1, 1);
    b.timestamp = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    for (k, v) in [
        (cb::GENESIS_CHAIN_ID_KEY, chain.to_string()),
        (cb::GENESIS_NETWORK_ID_KEY, "testnet".into()),
        (cb::GENESIS_PROTOCOL_VERSION_KEY, "v1".into()),
        (cb::GENESIS_ACTIVATION_SCHEDULE_KEY, SCHEDULE.into()),
        (cb::GENESIS_FAUCET_ADDRESS_KEY, owner(10).to_hex()),
        (cb::GENESIS_FLAT_REWARD_KEY, "600".into()),
        (cb::GENESIS_FLAT_REWARD_UNITS_KEY, "2222222222".into()),
        (cb::GENESIS_FAUCET_ALLOCATION_KEY, "10000000000000".into()),
        (cb::GENESIS_GOVERNANCE_KEYS_KEY, hex::encode(pk(99))),
        (cb::GENESIS_NODE_EPOCH_KEY, "512".into()),
        (cb::GENESIS_NODE_CHALLENGE_KEY, "512".into()),
    ] { b.metadata.insert(k.into(), v); }
    cb::validate_testnet_genesis(&b).unwrap();
    b.hash = Some(b.calculate_hash());
    b
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SourceFixture {
    genesis: Block,
    request: SnapshotSyncIngestRequest,
    // Canonical consensus preimage, independent of map insertion order or
    // runtime-only fields in the checkpoint codec.
    expected_state_bytes: String,
}

fn build_source(chain: &str) -> SourceFixture {
    let genesis = block1(chain);
    cb::install_protocol_from_block1(&genesis).unwrap();
    let genesis_hash = genesis.hash.unwrap();
    if chain == "777" {
        // Preserve the legacy fixture's compiled/default 1024 timing. This
        // synthetic genesis deliberately has no production bootstrap pin.
        wrfcoin_shared_protocol::governance::schedule_extension::install_schedule_extension_authority_for_tests(
            "777", "testnet", &genesis_hash.to_hex(), vec![hex::encode(pk(99))]);
    } else {
        assert!(wrfcoin_native_chain::upgrade::install_schedule_extension_authority_for_block1(&genesis));
    }
    let mut state = State::new();
    if chain == "777" {
        for byte in [10, 20] {
            state.create_account(owner(byte), Balance::new(u128::from(NODE_BOND_UNITS) * 3));
        }
    }
    state.apply_transitions_batch_at_height(1, vec![StateTransition::RecordGenesisEmissionConfig {
        config: cb::GenesisEmissionConfig::from_block(&genesis).unwrap(),
    }]).unwrap();
    if chain == "888" {
        state.apply_transition(StateTransition::BalanceTransfer {
            from: owner(10), to: owner(20), amount: Balance::new(u128::from(NODE_BOND_UNITS) * 2),
            nonce: Some(0), current_block: 1,
        }).unwrap();
    }
    let height = A + if chain == "888" { 512 } else { 1024 } + 2;
    let mut blocks = vec![genesis.clone()];
    for h in 2..=height {
        let parent = blocks.last().unwrap().hash.unwrap();
        state.open_emission_block(h);
        state.begin_node_service_block(h, *parent.as_bytes()).unwrap();
        if h == A {
            for (byte, id) in [(10, 1), (20, 2)] {
                state.apply_transition(StateTransition::ApplyNodeBondLifecycle {
                    payload: deposit(byte, id, *genesis_hash.as_bytes(), chain), block_height: h,
                }).unwrap();
            }
        }
        state.apply_block_finalization(h);
        assert!(state.supply_violation.is_none(), "synthetic source supply at {h}");
        let mut block = Block::new(3, parent, vec![], 1, h);
        block.timestamp = chrono::DateTime::from_timestamp(1_700_000_000 + h as i64, 0).unwrap();
        if h == height {
            block.metadata.insert(STATE_ROOT_METADATA_KEY.into(), hex::encode(state.state_root.0));
        }
        block.hash = Some(block.calculate_hash());
        blocks.push(block);
    }
    state.validate_node_bond_anchor(height).unwrap();
    assert_eq!(state.node_bond_registry.as_ref().unwrap().frozen_epochs[&1].bindings.len(), 2);
    let floor = window_floor(height, crate::routes::snapshot::snapshot_block_window(&state, height));
    let carried: Vec<_> = blocks.iter().filter(|b| b.height == 1 || b.height >= floor).cloned().collect();
    let mut entries = export_state_entries(&state).unwrap();
    entries.extend(export_block_entries(&carried).unwrap());
    let snapshot = build_state_snapshot(height, hex::encode(state.state_root.0), &entries);
    let payload = build_snapshot_sync_payload(&snapshot, blocks.last().unwrap().hash.unwrap().to_hex(), 0);
    let signed = sign_snapshot_sync_payload(payload, &key(11), 1_700_000_523, "synthetic-receiver-authority");
    SourceFixture {
        genesis,
        expected_state_bytes: hex::encode(state.state_root_material()),
        request: SnapshotSyncIngestRequest {
            signed_payload: signed,
            entries: entries.into_iter().map(|(k, v)| crate::routes::snapshot::SnapshotEntryKV {
                key_hex: hex::encode(k), value_hex: hex::encode(v),
            }).collect(), served_by: Some("synthetic-server".into()),
        },
    }
}

fn isolated(name: &str, chain: &str) -> bool {
    crate::helpers::test_env::isolated_chain_domain(name, chain, "testnet", &owner(10).to_hex(), "none")
}

fn fixture(name: &str, chain: &str) -> Option<SourceFixture> {
    // determinism-ok: test-only subprocess output path; never a block, state or consensus input.
    if let Ok(path) = std::env::var(SOURCE_OUTPUT) {
        serde_json::to_writer(std::fs::File::create(path).unwrap(), &build_source(chain)).unwrap();
        return None;
    }
    assert!(schedule_extension_authority().is_none(), "receiver must start without source authority");
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("synthetic-source.json");
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(SOURCE_OUTPUT, &file).output().unwrap();
    assert!(result.status.success(), "source fixture: {}{}",
        String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    let fixture: SourceFixture = serde_json::from_reader(std::fs::File::open(file).unwrap()).unwrap(); // key-file-exempt: test-only JSON source fixture, not key material
    // Models the fresh receiver's startup schedule, WITHOUT installing the
    // authority. Source process globals cannot leak across this boundary.
    cb::install_protocol_from_block1(&fixture.genesis).unwrap();
    assert!(schedule_extension_authority().is_none());
    Some(fixture)
}

fn raw_entries(request: &SnapshotSyncIngestRequest) -> Vec<(Vec<u8>, Vec<u8>)> {
    request.entries.iter().map(|e| (hex::decode(&e.key_hex).unwrap(), hex::decode(&e.value_hex).unwrap())).collect()
}

fn receiver() -> web::Data<AppState> {
    let mut inner = test_state(Some(key(22))).into_inner();
    Arc::get_mut(&mut inner).unwrap().snapshot_trusted_signers =
        Arc::new([hex::encode(pk(11))].into_iter().collect());
    web::Data::from(inner)
}

#[actix_web::test]
async fn fresh_888_receiver_accepts_epoch_one_bonded_snapshot() {
    const NAME: &str = "tests::snapshot_authority_888::fresh_888_receiver_accepts_epoch_one_bonded_snapshot";
    if isolated(NAME, "888") { return; }
    let Some(mut f) = fixture(NAME, "888") else { return; };
    // The source fixture is deterministic; only the HTTP envelope needs to
    // satisfy the real route's freshness window before the acceptance test.
    // determinism-ok: test-only HTTP admission timestamp; never a block, state or consensus input.
    let request_timestamp = chrono::Utc::now().timestamp();
    f.request.signed_payload = sign_snapshot_sync_payload(
        f.request.signed_payload.payload.clone(), &key(11), request_timestamp, "synthetic-receiver-authority");
    let _pin = TestGenesisPin::set(f.genesis.hash.unwrap().to_hex());
    // Executable RED control at the same head: state import before authority
    // still refuses the actual epoch-one source entries with legacy timing.
    let (state_entries, _) = split_snapshot_entries(&raw_entries(&f.request));
    let error = State::new().apply_snapshot_entries(&state_entries).unwrap_err();
    assert!(error.to_string().contains("node_bond_registry"), "pre-bootstrap control: {error}");
    assert_eq!(node_epoch_blocks(), 1024);
    let receiver = receiver();
    let app = test::init_service(test_app(receiver.clone())).await;
    let response = test::call_service(&app, test::TestRequest::post()
        .uri("/network/snapshot-sync/receiver-ingest")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
        .set_json(&f.request).to_request()).await;
    let status = response.status();
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(status, StatusCode::OK, "receiver ingest: {body}");
    assert_eq!(body["applied"], true);
    assert!(body["receipt"].is_object(), "receiver signs only after real install: {body}");
    let chain = receiver.blockchain.read().await;
    assert_eq!(chain.height, 523);
    assert_eq!(chain.state.node_bond_registry.as_ref().unwrap().frozen_epochs[&1].bindings.len(), 2);
    assert_eq!(node_epoch_blocks(), 512);
    assert_eq!(node_challenge_blocks(Some("888")), 512);
    assert_eq!(hex::encode(chain.state.state_root_material()), f.expected_state_bytes);
}

#[actix_web::test]
async fn fresh_888_snapshot_join_installs_the_same_epoch_one_state() {
    const NAME: &str = "tests::snapshot_authority_888::fresh_888_snapshot_join_installs_the_same_epoch_one_state";
    if isolated(NAME, "888") { return; }
    let Some(f) = fixture(NAME, "888") else { return; };
    let _pin = TestGenesisPin::set(f.genesis.hash.unwrap().to_hex());
    let verified = verify_and_apply_snapshot(State::new(), &f.request.signed_payload.payload, &raw_entries(&f.request))
        .expect("pinned 888 join must import after epoch 1");
    assert_eq!(hex::encode(verified.state.state_root_material()), f.expected_state_bytes);
    assert_eq!(node_epoch_blocks(), 512);
    assert_eq!(schedule_extension_authority().unwrap().genesis_hash(), f.genesis.hash.unwrap().to_hex());
    let receiver = receiver();
    install_verified_snapshot(&receiver.blockchain, &receiver.storage, verified, 523).await.unwrap();
    assert_eq!(receiver.blockchain.read().await.height, 523);
}

#[actix_web::test]
async fn forged_888_carried_genesis_cannot_install_authority() {
    const NAME: &str = "tests::snapshot_authority_888::forged_888_carried_genesis_cannot_install_authority";
    if isolated(NAME, "888") { return; }
    let Some(f) = fixture(NAME, "888") else { return; };
    let _pin = TestGenesisPin::set(f.genesis.hash.unwrap().to_hex());
    let entries = raw_entries(&f.request);
    let (state_entries, chain_entries) = split_snapshot_entries(&entries);
    for recompute_hash in [false, true] {
        let mut forged = f.genesis.clone();
        forged.metadata.insert(cb::GENESIS_GOVERNANCE_KEYS_KEY.into(), hex::encode(pk(98)));
        if recompute_hash { forged.hash = Some(forged.calculate_hash()); }
        let replacement = export_block_entries(&[forged]).unwrap();
        let mut bad_entries = state_entries.clone();
        bad_entries.extend(chain_entries.iter().filter(|(k, _)| k != &replacement[0].0).cloned());
        bad_entries.extend(replacement);
        let mut payload = f.request.signed_payload.payload.clone();
        payload.snapshot_root_hex = build_state_snapshot(payload.height, payload.state_root_hex.clone(), &bad_entries).snapshot_root_hex;
        assert!(verify_and_apply_snapshot(State::new(), &payload, &bad_entries).is_err());
        assert!(schedule_extension_authority().is_none(), "forgery must not poison process authority");
        assert_eq!(node_epoch_blocks(), 1024);
    }
    let unpinned = GenesisExpectation { pinned_hash: None, identity: cb::ChainIdentity::new("888", "testnet") };
    assert_eq!(crate::routes::snapshot::verify_and_apply_snapshot_at(cb::SigningProtocolVersion::V1, // secret-leak-ok: compares a refusal enum using synthetic public genesis metadata, no signing material.
        &unpinned, State::new(), &f.request.signed_payload.payload, &entries).err(),
        Some(crate::routes::snapshot::SnapshotApplyRefusal::GenesisUnpinned));
    assert!(schedule_extension_authority().is_none());
}

#[actix_web::test]
async fn legacy_777_receiver_verifier_keeps_default_timing_and_exact_state_bytes() {
    const NAME: &str = "tests::snapshot_authority_888::legacy_777_receiver_verifier_keeps_default_timing_and_exact_state_bytes";
    if isolated(NAME, "777") { return; }
    let Some(f) = fixture(NAME, "777") else { return; };
    let _pin = TestGenesisPin::set(f.genesis.hash.unwrap().to_hex());
    let before = node_epoch_blocks();
    let verified = verify_and_apply_snapshot(State::new(), &f.request.signed_payload.payload, &raw_entries(&f.request))
        .expect("existing 777 bonded import remains accepted without early authority");
    assert_eq!(before, 1024);
    assert_eq!(node_epoch_blocks(), before);
    assert!(schedule_extension_authority().is_none(), "777 must not take the new authority path");
    assert_eq!(hex::encode(verified.state.state_root_material()), f.expected_state_bytes);
    assert_eq!(verified.state.node_bond_registry.as_ref().unwrap().frozen_epochs[&1].bindings.len(), 2);
}

#[actix_web::test]
async fn conflicting_888_authority_is_refused_without_replacement() {
    const NAME: &str = "tests::snapshot_authority_888::conflicting_888_authority_is_refused_without_replacement";
    if isolated(NAME, "888") { return; }
    let Some(f) = fixture(NAME, "888") else { return; };
    let mut other = f.genesis.clone();
    other.metadata.insert(cb::GENESIS_GOVERNANCE_KEYS_KEY.into(), hex::encode(pk(98)));
    other.hash = Some(other.calculate_hash());
    assert!(wrfcoin_native_chain::upgrade::install_schedule_extension_authority_for_block1(&other));
    let _pin = TestGenesisPin::set(f.genesis.hash.unwrap().to_hex());
    let error = verify_and_apply_snapshot(State::new(), &f.request.signed_payload.payload, &raw_entries(&f.request)).unwrap_err();
    assert!(matches!(error, crate::routes::snapshot::SnapshotApplyRefusal::GenesisNotOurs(_)));
    assert_eq!(schedule_extension_authority().unwrap().genesis_hash(), other.hash.unwrap().to_hex());
}
