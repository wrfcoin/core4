//! `passkey_bond_payout`: a node-bond Deposit/Update payout approval may be a
//! WebAuthn P-256 assertion from the passkey account named as
//! `payout_account`, from activation only.
//!
//! This binary installs its own process-wide schedule (node-share settlement
//! at `A` = 3, `passkey_bond_payout` at `P` = 4) and never calls
//! `fixture::state()`, so every test sees the same schedule: height 3 is
//! pre-activation, 4 and later are active.

#[path = "common/node_bond_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use fixture::{A, address, advance, apply, pk, resign};
use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
use wrfcoin_shared_protocol::accounts::passkey::{FLAG_USER_PRESENT, P256Signature, PasskeyAuthorization};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::Balance;
use wrfcoin_shared_protocol::consensus::node_bond_registry::{
    NODE_BOND_UNITS, NodeBondLifecycle, passkey_bond_payout_active,
};
use wrfcoin_shared_protocol::consensus::node_share_contracts::{
    NodeBondAction, node_bond_payout_passkey_challenge, node_epoch_blocks,
};
use wrfcoin_shared_protocol::consensus::verify_cost::VerifyCost;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FeatureSchedule, feature_active, install_feature_schedule,
};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    AppliedExtension, ScheduleEntry, install_schedule_extension_authority_for_tests,
};

const P: u64 = A + 1;
const SECRET: [u8; 32] = [0x3c; 32];
const OTHER_SECRET: [u8; 32] = [0x4d; 32];

fn identity() -> ChainIdentity {
    ChainIdentity::new("777", "testnet")
}

fn state() -> State {
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("node_share_settlement".into(), A),
            ("account_authorization_dispatch".into(), A),
            ("passkey_bond_payout".into(), P),
        ])
        .unwrap(),
    );
    install_schedule_extension_authority_for_tests(
        "777",
        "testnet",
        &hex::encode([7u8; 32]),
        vec![hex::encode(pk(99))],
    );
    let mut s = State::new();
    s.genesis_emission_config = Some(wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig {
        chain_id: "777".into(),
        faucet_address: None,
        ..Default::default()
    });
    s.feature_schedule_extensions.push(AppliedExtension {
        sequence: 0,
        carried_at_height: 0,
        entries: vec![ScheduleEntry::new("node_share_settlement", A)],
        version_steps: vec![],
        min_protocol_version: 2,
        release_commit: "test-only-explicit-schedule".into(),
        signers: vec![hex::encode(pk(99))],
    });
    s.create_account(address(10), Balance::new(u128::from(NODE_BOND_UNITS) * 3));
    s.create_account(address(20), Balance::new(u128::from(NODE_BOND_UNITS) * 3));
    s.apply_transition(StateTransition::BeginNodeServiceBlock { block_height: A, parent_hash: [2; 32] })
        .unwrap();
    s.apply_block_finalization(A);
    s
}

fn authenticator() -> SoftwareAuthenticator {
    SoftwareAuthenticator::new(&SECRET, "wrfcoin.com")
}

/// The fixture's ordinary-key lifecycle, re-pointed at `auth`'s passkey
/// account and re-signed by every other role.
fn to_passkey_account(mut p: NodeBondLifecycle, auth: &SoftwareAuthenticator) -> NodeBondLifecycle {
    p.signed.authorization.payout_account = auth.account();
    let owner = owner_of(&p);
    resign(&mut p, owner);
    p
}

fn owner_of(p: &NodeBondLifecycle) -> u8 {
    (10..=40).find(|b| address(*b) == p.signed.authorization.owner_account).unwrap()
}

fn approve(p: &NodeBondLifecycle, auth: &SoftwareAuthenticator) -> PasskeyAuthorization {
    let challenge = node_bond_payout_passkey_challenge(&p.signed.authorization, &identity());
    auth.authorization_for(&auth.assert(&challenge))
}

/// Passkey-only payout approval: no ed25519 payout key or signature.
fn passkey_payload(owner: u8, id: u8, nonce: u64, action: NodeBondAction, epoch: u64) -> NodeBondLifecycle {
    let auth = authenticator();
    let mut p = to_passkey_account(fixture::auth(owner, id, nonce, action, epoch), &auth);
    p.payout_key = None;
    p.payout_signature.clear();
    p.payout_passkey = Some(approve(&p, &auth));
    p
}

fn verify_at(p: &NodeBondLifecycle, height: u64) -> bool {
    p.verify(&identity(), &[7; 32], &[hex::encode(pk(99))], height).is_ok()
}

fn root(s: &mut State) -> wrfcoin_shared_protocol::common::types::Hash {
    s.recalculate_state_root();
    s.state_root
}

#[test]
fn feature_is_registered_appended_unscheduled_by_default_and_schedulable() {
    let _ = state();
    let f = ConsensusFeature::PasskeyBondPayout;
    assert_eq!(f.name(), "passkey_bond_payout");
    assert_eq!(ConsensusFeature::parse("passkey_bond_payout"), Some(f));
    assert_eq!(ConsensusFeature::ALL.last(), Some(&f));
    assert_eq!(ConsensusFeature::ALL.len(), 30);
    assert_eq!(bincode::serialize(&f).unwrap(), 29u32.to_le_bytes());
    assert!(f.schedulable(None));
    assert!(f.has_behaviour());
    assert!(wrfcoin_shared_protocol::crypto::protocol_support::KNOWN_FEATURES.contains(&"passkey_bond_payout"));
    assert!(!FeatureSchedule::default().active(f, u64::MAX));
    assert!(!feature_active(f, A) && !passkey_bond_payout_active(A));
    assert!(feature_active(f, P) && passkey_bond_payout_active(P));
}

#[test]
fn pre_activation_a_passkey_only_payout_is_refused_exactly_as_today() {
    let mut s = state();
    let p = passkey_payload(10, 1, 0, NodeBondAction::Deposit, 1);
    assert!(!verify_at(&p, A));
    let before = root(&mut s);
    assert!(apply(&mut s, p, A).is_err());
    assert_eq!(root(&mut s), before, "refusal leaves state untouched");
}

#[test]
fn pre_activation_the_member_is_ignored_and_state_is_byte_identical() {
    // An ordinary-key Deposit that also carries a (valid, or garbage) member:
    // a binary without this feature ignores the unknown member and accepts it.
    let plain = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
    let mut with_member = plain.clone();
    with_member.payout_passkey = Some(approve(&plain, &authenticator()));
    let mut s1 = state();
    let mut s2 = state();
    apply(&mut s1, plain.clone(), A).unwrap();
    apply(&mut s2, with_member, A).unwrap();
    assert_eq!(root(&mut s1), root(&mut s2));
    assert_eq!(s1.node_bond_registry, s2.node_bond_registry);

    // A malformed member reads as absent (lenient), so it parses the same.
    let mut json: serde_json::Value = serde_json::to_value(&plain).unwrap();
    json["payout_passkey"] = serde_json::json!({"public_key_p256": 5});
    let parsed: NodeBondLifecycle = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, plain);
    // Absent members are never serialized: existing JSON is byte-identical.
    assert!(!serde_json::to_string(&plain).unwrap().contains("payout_passkey"));
}

#[test]
fn active_a_passkey_payout_deposit_is_accepted_and_binds_the_passkey_account() {
    let mut s = state();
    advance(&mut s, P);
    let p = passkey_payload(10, 1, 0, NodeBondAction::Deposit, 1);
    assert!(verify_at(&p, P));
    apply(&mut s, p, P).unwrap();
    let r = &s.node_bond_registry.as_ref().unwrap().registrations[&[1; 32]];
    assert_eq!(r.binding.payout_account, authenticator().account());
    // The stored binding is the plain authorization: no passkey material in state.
    assert_eq!(r.binding, passkey_payload(10, 1, 0, NodeBondAction::Deposit, 1).signed.authorization);
}

#[test]
fn active_the_ordinary_key_path_is_unchanged() {
    let mut s = state();
    advance(&mut s, P);
    let p = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
    assert!(verify_at(&p, P));
    apply(&mut s, p, P).unwrap();
}

#[test]
fn active_exactly_one_payout_approval_is_accepted() {
    let auth = authenticator();
    // Both an ed25519 key and a passkey assertion.
    let mut both = passkey_payload(10, 1, 0, NodeBondAction::Deposit, 1);
    both.payout_key = Some(pk(10));
    assert!(!verify_at(&both, P));
    // A stray ed25519 signature next to the passkey.
    let mut stray = passkey_payload(10, 1, 0, NodeBondAction::Deposit, 1);
    stray.payout_signature = vec![1; 64];
    assert!(!verify_at(&stray, P));
    // Neither.
    let mut neither = passkey_payload(10, 1, 0, NodeBondAction::Deposit, 1);
    neither.payout_passkey = None;
    assert!(!verify_at(&neither, P));
    // An ordinary-key payload also carrying a valid passkey member.
    let mut extra = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
    extra.payout_passkey = Some(approve(&extra, &auth));
    assert!(!verify_at(&extra, P));
    assert!(verify_at(&extra, A), "but ignored before activation");
}

#[test]
fn active_assertions_bound_elsewhere_are_refused() {
    let auth = authenticator();
    let good = passkey_payload(10, 1, 0, NodeBondAction::Deposit, 1);
    assert!(verify_at(&good, P));

    // Another passkey's assertion: credential does not derive payout_account.
    let other = SoftwareAuthenticator::new(&OTHER_SECRET, "wrfcoin.com");
    let mut p = good.clone();
    p.payout_passkey = Some(approve(&p, &other));
    assert!(!verify_at(&p, P));

    // Same key under another RP ID: a different account.
    let phish = SoftwareAuthenticator::new(&SECRET, "wrfcoin.com.evil.example");
    let mut p = good.clone();
    p.payout_passkey = Some(approve(&p, &phish));
    assert!(!verify_at(&p, P));

    // An assertion over a different authorization (next nonce) of the same bond.
    let mut p = good.clone();
    let mut later = good.clone();
    later.signed.authorization.expected_nonce += 1;
    p.payout_passkey = Some(approve(&later, &auth));
    assert!(!verify_at(&p, P));

    // An assertion over a different chain identity.
    let mut p = good.clone();
    let challenge =
        node_bond_payout_passkey_challenge(&p.signed.authorization, &ChainIdentity::new("888", "testnet"));
    p.payout_passkey = Some(auth.authorization_for(&auth.assert(&challenge)));
    assert!(!verify_at(&p, P));

    // A passkey *transfer* challenge is never a bond approval.
    let mut p = good.clone();
    p.payout_passkey = Some(auth.authorize_transfer(&address(10).to_hex(), 1, 0));
    assert!(!verify_at(&p, P));
}

#[test]
fn active_high_s_and_missing_user_verification_are_refused() {
    let auth = authenticator();
    let mut p = passkey_payload(10, 1, 0, NodeBondAction::Deposit, 1);
    let challenge = node_bond_payout_passkey_challenge(&p.signed.authorization, &identity());
    let mut assertion = auth.assert(&challenge);
    let high = P256Signature::from_der(&assertion.signature).unwrap().malleable_twin().unwrap();
    assertion.signature = high.to_der_bytes();
    p.payout_passkey = Some(auth.authorization_for(&assertion));
    assert!(!verify_at(&p, P));

    let mut presence_only = authenticator();
    presence_only.flags = FLAG_USER_PRESENT;
    let mut p = passkey_payload(10, 1, 0, NodeBondAction::Deposit, 1);
    p.payout_passkey = Some(approve(&p, &presence_only));
    assert!(!verify_at(&p, P));
}

#[test]
fn exit_and_withdraw_refuse_a_passkey_member_only_once_active() {
    for action in [NodeBondAction::Exit, NodeBondAction::Withdraw] {
        let mut p = fixture::auth(10, 1, 1, action, 2);
        assert!(verify_at(&p, P));
        p.payout_passkey = Some(approve(&p, &authenticator()));
        assert!(verify_at(&p, A), "{action:?}: ignored before activation");
        assert!(!verify_at(&p, P), "{action:?}: surplus approval refused once active");
    }
}

#[test]
fn an_ordinary_payout_bond_switches_to_a_passkey_payout_via_update() {
    let mut s = state();
    apply(&mut s, fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    let e1 = A + node_epoch_blocks();
    advance(&mut s, e1);
    // Update to the passkey account, approved by the passkey (governance,
    // owner and both online keys re-sign; the old payout key does not).
    let mut update = passkey_payload(10, 1, 1, NodeBondAction::Update, 2);
    assert!(update.payout_key.is_none());
    apply(&mut s, update.clone(), e1).unwrap();
    let r = &s.node_bond_registry.as_ref().unwrap().registrations[&[1; 32]];
    assert_eq!(r.binding.payout_account, address(10));
    assert_eq!(r.pending_binding.as_ref().unwrap().payout_account, authenticator().account());
    advance(&mut s, e1 + node_epoch_blocks());
    let r = &s.node_bond_registry.as_ref().unwrap().registrations[&[1; 32]];
    assert_eq!(r.binding.payout_account, authenticator().account());
    assert!(r.pending_binding.is_none());

    // The same Update without governance is refused.
    update.governance_signature.clear();
    assert!(!verify_at(&update, e1));
}

#[test]
fn passkey_payout_pricing_is_gated_by_activation() {
    let _ = state();
    let p = passkey_payload(10, 1, 0, NodeBondAction::Deposit, 1);
    let tx = fixture::transaction(&p, 1_000_000);
    let base = tx.verify_work();
    assert_eq!(tx.verify_work_at_height(A), base, "pre-activation price unchanged");
    let active = tx.verify_work_at_height(P);
    assert_eq!(active.ecdsa, base.ecdsa + 1);
    assert_eq!(active.ed25519, base.ed25519);
    // No member: never charged.
    let plain = fixture::transaction(&fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1), 1_000_000);
    assert_eq!(plain.verify_work_at_height(P), plain.verify_work());
}
