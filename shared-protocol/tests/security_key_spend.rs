//! Security-key wallets (`security_key_spend`): a passphrase account that also
//! requires a registered FIDO2 / WebAuthn P-256 key to spend.
//!
//! One process, one installed schedule: `account_authorization_dispatch` at
//! [`DISPATCH`] and `security_key_spend` at [`H`]. Pinned here:
//!
//! - the activation boundary (H-1 refuses every action and stores nothing; H
//!   accepts) through the real state machine (`ProcessGovernanceTransaction`);
//! - the spend rule: phrase-only refused, phrase + key accepted, the key
//!   assertion bound to amount / recipient / nonce / account / domain;
//! - the time-locked escape (phrase alone starts it, the rule lapses at exactly
//!   `executable_at_height`, a key-signed cancel stops it, a phrase-only cancel
//!   cannot) and key add / remove / disable;
//! - replay and chain binding of actions;
//! - the pre-block safety argument: the State view is strictly stronger than
//!   the `EmptyAuthorizationView` used by verify-block and the mempool;
//! - state-root material is byte-absent until a rule is stored, and pricing is
//!   unchanged below H.

use proptest::prelude::*;
use wrfcoin_shared_protocol::accounts::authorization::{
    AccountAuthorization, AccountAuthorizationDeclaration, AuthorizationError, AuthorizationRule,
    AuthorizationView, DeclareAuthorization, EmptyAuthorizationView, SpendAuthorization,
    apply_declaration, authorization_for, authorize_debit_with_security_key,
    declare_authorization_message,
};
use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
use wrfcoin_shared_protocol::accounts::passkey::{
    FLAG_USER_PRESENT, passkey_transfer_authorization_message, passkey_transfer_challenge,
};
use wrfcoin_shared_protocol::accounts::security_key::{
    ESCAPE_DELAY_DEFAULT_BLOCKS, ESCAPE_DELAY_MAX_BLOCKS, ESCAPE_DELAY_MIN_BLOCKS,
    SECURITY_KEY_AUTH_MEMBER, SecurityKeyActionBody, SecurityKeyActionPayload,
    SecurityKeyAssertionWire, SecurityKeyError, SecurityKeyWire, action_challenge,
    action_message, apply_action, security_key_active, spend_challenge, stored_policy,
};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    StakeAuthPayload, Transaction, TransactionType, TransparentTransferPayload,
    stake_authorization_message, transparent_transfer_authorization_message,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance, Timestamp};
use wrfcoin_shared_protocol::consensus::verify_cost::VerifyCost;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FeatureSchedule, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

use rand::{SeedableRng, rngs::StdRng};
use std::collections::BTreeMap;

const DISPATCH: u64 = 10;
const H: u64 = 100;
const RP_ID: &str = "wrfcoin.com";

fn install() {
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            (ConsensusFeature::AccountAuthorizationDispatch.name().to_string(), DISPATCH),
            (ConsensusFeature::SecurityKeySpend.name().to_string(), H),
            (ConsensusFeature::SessionKeys.name().to_string(), H),
        ])
        .expect("schedule"),
    );
}

fn ids() -> (String, String) {
    let identity = ChainIdentity::active();
    (identity.chain_id.clone(), identity.network_id.clone())
}

fn owner(seed: u64) -> (PrivateKey, PublicKey, Address) {
    let mut rng = StdRng::seed_from_u64(seed);
    let key = PrivateKey::generate(&mut rng);
    let public = PublicKey::from_private_key(&key);
    let address = Address::from_public_key(&public);
    (key, public, address)
}

fn yubikey(byte: u8) -> SoftwareAuthenticator {
    SoftwareAuthenticator::new(&[byte; 32], RP_ID)
}

fn wire(a: &SoftwareAuthenticator) -> SecurityKeyWire {
    SecurityKeyWire {
        public_key_p256: hex::encode(a.key.public_key().to_raw_xy()),
        rp_id_hash: hex::encode(a.rp_id_hash),
        credential_id: hex::encode([0xC0, 0xDE, 0x01]),
    }
}

fn pk_hex(a: &SoftwareAuthenticator) -> String {
    hex::encode(a.key.public_key().to_raw_xy())
}

fn assertion(a: &SoftwareAuthenticator, challenge: &[u8; 32]) -> SecurityKeyAssertionWire {
    let raw = a.assert(challenge);
    SecurityKeyAssertionWire {
        public_key_p256: pk_hex(a),
        authenticator_data: hex::encode(raw.authenticator_data),
        client_data_json: hex::encode(raw.client_data_json),
        signature: hex::encode(raw.signature),
    }
}

fn signed_action_on(
    chain_id: &str,
    network_id: &str,
    key: &PrivateKey,
    nonce: u64,
    body: SecurityKeyActionBody,
    signers: &[&SoftwareAuthenticator],
) -> SecurityKeyActionPayload {
    let public = PublicKey::from_private_key(key);
    let account = Address::from_public_key(&public);
    let message = action_message(chain_id, network_id, &account, nonce, &body).unwrap();
    let challenge = action_challenge(&message);
    SecurityKeyActionPayload {
        account: account.to_string(),
        nonce,
        action: body,
        owner_public_key: public.to_hex(),
        owner_signature: key.sign(&message).to_hex(),
        key_assertions: signers.iter().map(|s| assertion(s, &challenge)).collect(),
    }
}

fn signed_action(
    key: &PrivateKey,
    nonce: u64,
    body: SecurityKeyActionBody,
    signers: &[&SoftwareAuthenticator],
) -> SecurityKeyActionPayload {
    let (chain, network) = ids();
    signed_action_on(&chain, &network, key, nonce, body, signers)
}

fn enable(a: &SoftwareAuthenticator) -> SecurityKeyActionBody {
    SecurityKeyActionBody::Enable { key: wire(a), escape_delay_blocks: ESCAPE_DELAY_MIN_BLOCKS }
}

fn action_tx(payload: &SecurityKeyActionPayload) -> Transaction {
    let mut tx = Transaction::new(vec![], vec![]);
    tx.tx_type = TransactionType::SecurityKeyAction;
    tx.data = Some(serde_json::to_string(payload).unwrap());
    tx.set_id();
    tx
}

fn apply_via_state(state: &mut State, payload: &SecurityKeyActionPayload, height: u64) -> bool {
    state
        .apply_transition(StateTransition::ProcessGovernanceTransaction {
            tx: action_tx(payload),
            current_block: height,
        })
        .is_ok()
}

const TO: Address = Address([0xB2; 20]);

fn transfer_tx(
    key: &PrivateKey,
    amount: u64,
    nonce: u64,
    proof: Option<SecurityKeyAssertionWire>,
) -> Transaction {
    let from = Address::from_public_key(&PublicKey::from_private_key(key));
    let mut payload = TransparentTransferPayload {
        from: from.to_string(),
        to: TO.to_string(),
        amount,
        transparent_opt_out: true,
        opt_out_audit_tag: None,
        auth_pubkey: None,
        auth_signature: None,
        authority: None,
        authority_signature: None,
        passkey_auth: None,
        sponsorship: None,
        session_key_auth: None,
    };
    payload.sign_authorization(key, nonce);
    let mut value = serde_json::to_value(&payload).unwrap();
    if let Some(proof) = proof {
        value[SECURITY_KEY_AUTH_MEMBER] = serde_json::to_value(proof).unwrap();
    }
    let mut tx = Transaction::new(vec![], vec![]);
    tx.tx_type = TransactionType::Transfer;
    tx.nonce = nonce;
    tx.data = Some(value.to_string());
    tx.set_id();
    tx
}

fn transfer_proof(
    a: &SoftwareAuthenticator,
    from: &Address,
    amount: u64,
    nonce: u64,
) -> SecurityKeyAssertionWire {
    let message =
        transparent_transfer_authorization_message(&from.to_string(), &TO.to_string(), amount, nonce);
    assertion(a, &spend_challenge(from, &message))
}

fn send(state: &mut State, tx: Transaction, height: u64) -> bool {
    state
        .apply_transition(StateTransition::ProcessGovernanceTransaction { tx, current_block: height })
        .is_ok()
}

fn funded(address: Address) -> State {
    let mut state = State::new();
    state.create_account(address, Balance::new(1_000));
    state.create_account(TO, Balance::new(0));
    state
}

fn balance(state: &State, address: &Address) -> u128 {
    state.get_account(address).unwrap().balance.into_inner()
}

// ---------------------------------------------------------------------------
// Activation boundary
// ---------------------------------------------------------------------------

#[test]
fn activation_boundary_h_minus_one_refuses_and_stores_nothing_h_accepts() {
    install();
    assert!(!security_key_active(H - 1));
    assert!(security_key_active(H));
    let (key, _, account) = owner(1);
    let yk = yubikey(0x21);
    let payload = signed_action(&key, 0, enable(&yk), &[&yk]);

    let mut state = funded(account);
    let before = state.state_root_material();
    assert!(!apply_via_state(&mut state, &payload, H - 1), "an action below H must be refused");
    assert!(state.authorization_declarations.is_empty());
    assert_eq!(state.state_root_material(), before, "a refused action must not move root material");

    assert!(apply_via_state(&mut state, &payload, H), "the same action at H applies");
    let declaration = state.authorization_declarations.get(&account).unwrap();
    assert_eq!(declaration.next_nonce, 1);
    assert_eq!(declaration.generation, 1);
    let policy = stored_policy(declaration).unwrap();
    assert_eq!(policy.keys.len(), 1);
    assert_eq!(policy.escape_delay_blocks, ESCAPE_DELAY_MIN_BLOCKS);
    assert_ne!(state.state_root_material(), before, "a stored rule enters root material");
}

#[test]
fn root_material_is_byte_absent_until_a_rule_is_stored() {
    install();
    let (key, _, account) = owner(2);
    let mut state = funded(account);
    let empty = State::new().state_root_material();
    let mut baseline = State::new();
    baseline.create_account(account, Balance::new(1_000));
    baseline.create_account(TO, Balance::new(0));
    assert_eq!(state.state_root_material(), baseline.state_root_material());
    // A phrase-only send from an unprotected account at and after H is the
    // legacy path, byte for byte.
    assert!(send(&mut state, transfer_tx(&key, 10, 0, None), H + 5));
    assert!(send(&mut baseline, transfer_tx(&key, 10, 0, None), H + 5));
    assert_eq!(state.state_root_material(), baseline.state_root_material());
    assert!(!state.state_root_material().windows(37).any(|w| w == b"wrfcoin_authorization_declarations_v1"));
    assert!(!empty.windows(37).any(|w| w == b"wrfcoin_authorization_declarations_v1"));
}

// ---------------------------------------------------------------------------
// Spending
// ---------------------------------------------------------------------------

fn protected(seed: u64, yk: &SoftwareAuthenticator) -> (PrivateKey, Address, State) {
    let (key, _, account) = owner(seed);
    let mut state = funded(account);
    assert!(apply_via_state(&mut state, &signed_action(&key, 0, enable(yk), &[yk]), H));
    (key, account, state)
}

#[test]
fn a_protected_account_needs_phrase_and_key_to_send() {
    install();
    let yk = yubikey(0x31);
    let (key, account, mut state) = protected(3, &yk);

    // Phrase only: refused, nothing moves.
    assert!(!send(&mut state, transfer_tx(&key, 10, 0, None), H + 1));
    assert_eq!(balance(&state, &account), 1_000);

    // Phrase + key: accepted.
    let proof = transfer_proof(&yk, &account, 10, 0);
    assert!(send(&mut state, transfer_tx(&key, 10, 0, Some(proof)), H + 1));
    assert_eq!(balance(&state, &account), 990);
    assert_eq!(balance(&state, &TO), 10);
}

#[test]
fn the_key_assertion_is_bound_to_amount_recipient_nonce_and_domain() {
    install();
    let yk = yubikey(0x41);
    let (key, account, mut state) = protected(4, &yk);

    // Assertion for 10 cannot authorize 500.
    let for_ten = transfer_proof(&yk, &account, 10, 0);
    assert!(!send(&mut state, transfer_tx(&key, 500, 0, Some(for_ten.clone())), H + 1));
    // Assertion for nonce 0 cannot authorize nonce 1.
    assert!(!send(&mut state, transfer_tx(&key, 10, 1, Some(for_ten)), H + 1));
    // A passkey-domain challenge over the same transfer is not a security-key
    // co-signature.
    let passkey_message = passkey_transfer_authorization_message(&account.to_string(), &TO.to_string(), 10, 0);
    let passkey_domain = assertion(&yk, &passkey_transfer_challenge(&passkey_message));
    assert!(!send(&mut state, transfer_tx(&key, 10, 0, Some(passkey_domain)), H + 1));
    // An action-domain challenge is not a spend either.
    let action_domain = assertion(&yk, &action_challenge(b"anything"));
    assert!(!send(&mut state, transfer_tx(&key, 10, 0, Some(action_domain)), H + 1));
    // An assertion over another account's identical transfer does not transfer.
    let (_, _, other) = owner(40);
    let other_message = transparent_transfer_authorization_message(&account.to_string(), &TO.to_string(), 10, 0);
    let other_account = assertion(&yk, &spend_challenge(&other, &other_message));
    assert!(!send(&mut state, transfer_tx(&key, 10, 0, Some(other_account)), H + 1));
    // An unregistered key, even with a perfect assertion, is refused.
    let stranger = yubikey(0x42);
    let stranger_proof = transfer_proof(&stranger, &account, 10, 0);
    assert!(!send(&mut state, transfer_tx(&key, 10, 0, Some(stranger_proof)), H + 1));
    // Touch without PIN (user present, not verified) is refused.
    let mut no_pin = yubikey(0x41);
    no_pin.flags = FLAG_USER_PRESENT;
    let no_pin_proof = transfer_proof(&no_pin, &account, 10, 0);
    assert!(!send(&mut state, transfer_tx(&key, 10, 0, Some(no_pin_proof)), H + 1));
    assert_eq!(balance(&state, &account), 1_000);

    // The right one still works.
    assert!(send(&mut state, transfer_tx(&key, 10, 0, Some(transfer_proof(&yk, &account, 10, 0))), H + 1));
}

#[test]
fn a_key_without_the_phrase_cannot_spend() {
    install();
    let yk = yubikey(0x51);
    let (_, account, mut state) = protected(5, &yk);
    let (thief, _, _) = owner(55);
    // The thief signs the debit with their own phrase key over the victim's
    // `from`: the signer-is-sender half refuses it regardless of the key.
    let mut tx = transfer_tx(&thief, 10, 0, Some(transfer_proof(&yk, &account, 10, 0)));
    let mut value: serde_json::Value = serde_json::from_str(tx.data.as_deref().unwrap()).unwrap();
    value["from"] = serde_json::Value::String(account.to_string());
    tx.data = Some(value.to_string());
    tx.set_id();
    assert!(!send(&mut state, tx, H + 1));
    assert_eq!(balance(&state, &account), 1_000);
}

#[test]
fn stake_and_unstake_need_the_key_too() {
    install();
    let yk = yubikey(0x61);
    let (key, account, state) = protected(6, &yk);
    let stake = |proof: Option<SecurityKeyAssertionWire>| {
        let mut payload = StakeAuthPayload {
            account: account.to_string(),
            amount: 100,
            auth_pubkey: None,
            auth_signature: None,
        };
        payload.sign_authorization(&key, false, 0);
        let mut value = serde_json::to_value(&payload).unwrap();
        if let Some(proof) = proof {
            value[SECURITY_KEY_AUTH_MEMBER] = serde_json::to_value(proof).unwrap();
        }
        let mut tx = Transaction::new(vec![], vec![]);
        tx.tx_type = TransactionType::Stake;
        tx.nonce = 0;
        tx.data = Some(value.to_string());
        tx.set_id();
        tx
    };
    let message = stake_authorization_message(&account.to_string(), 100, false, 0);
    let proof = assertion(&yk, &spend_challenge(&account, &message));
    assert!(!stake(None).verify_stake_authorization_with_view(&state, H + 1, Timestamp::new(0)));
    assert!(stake(Some(proof.clone())).verify_stake_authorization_with_view(&state, H + 1, Timestamp::new(0)));
    // The pre-block (EmptyView) check accepts both: the State view is stricter.
    assert!(stake(None).verify_stake_authorization());
    assert!(stake(Some(proof)).verify_stake_authorization());
}

// ---------------------------------------------------------------------------
// Pre-block safety: the State view never accepts what the EmptyView refuses
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]

    /// For any transfer (right or wrong signer, right or wrong amount, with or
    /// without a proof, valid or tampered), State-backed acceptance implies
    /// EmptyView acceptance. That is what makes it safe for verify-block and
    /// the mempool to keep the EmptyView: they accept a superset, and the
    /// governance sink drops the rest deterministically.
    #[test]
    fn security_key_spend_state_view_is_stricter(
        seed in 0u64..1_000,
        amount in 1u64..1_000,
        sign_amount in 1u64..1_000,
        nonce in 0u64..4,
        proof_nonce in 0u64..4,
        with_proof in any::<bool>(),
        wrong_signer in any::<bool>(),
        height in H..H + 3 * ESCAPE_DELAY_MIN_BLOCKS,
    ) {
        install();
        let yk = yubikey(0x71);
        let (key, account, mut state) = protected(1_000 + seed, &yk);
        if seed % 3 == 0 {
            // Some cases with a pending escape.
            let begin = signed_action(&key, 1, SecurityKeyActionBody::BeginEscape, &[]);
            prop_assert!(apply_via_state(&mut state, &begin, H));
        }
        let signer = if wrong_signer { owner(seed + 5_000).0 } else { key.clone() };
        let message = transparent_transfer_authorization_message(&account.to_string(), &TO.to_string(), sign_amount, nonce);
        let signature = signer.sign(&message).to_hex();
        let public = PublicKey::from_private_key(&signer).to_hex();
        let spend_message = transparent_transfer_authorization_message(&account.to_string(), &TO.to_string(), amount, proof_nonce);
        let proof = with_proof.then(|| assertion(&yk, &spend_challenge(&account, &spend_message)));
        let auth = SpendAuthorization { signer_public_key: &public, signature: &signature };
        let state_backed = authorize_debit_with_security_key(&state, &account, &message, auth, proof.as_ref(), height, Timestamp::new(0));
        let pre_block = authorize_debit_with_security_key(&EmptyAuthorizationView, &account, &message, auth, proof.as_ref(), height, Timestamp::new(0));
        if state_backed.is_ok() {
            prop_assert!(pre_block.is_ok());
        }
        // And the protected rule really does refuse a phrase-only debit while enforced.
        let enforced = !matches!(authorization_for(&state.declared(&account), height, Timestamp::new(0)), AuthorizationRule::SignerIsSender);
        // (The ed25519 half is checked first: a wrong signer fails there.)
        if enforced && proof.is_none() && !wrong_signer {
            prop_assert_eq!(state_backed, Err(AuthorizationError::SecurityKeyRequired));
        }
    }

    /// Any single-byte tamper of a valid spend assertion is refused.
    #[test]
    fn tampered_spend_assertions_are_refused(field in 0usize..3, index in 0usize..64, flip in 1u8..=255) {
        install();
        let yk = yubikey(0x72);
        let (key, account, state) = protected(2_000, &yk);
        let message = transparent_transfer_authorization_message(&account.to_string(), &TO.to_string(), 10, 0);
        let mut proof = assertion(&yk, &spend_challenge(&account, &message));
        let target = match field { 0 => &mut proof.authenticator_data, 1 => &mut proof.client_data_json, _ => &mut proof.signature };
        let mut bytes = hex::decode(&*target).unwrap();
        let at = index % bytes.len();
        bytes[at] ^= flip;
        *target = hex::encode(bytes);
        let public = PublicKey::from_private_key(&key).to_hex();
        let signature = key.sign(&message).to_hex();
        let result = authorize_debit_with_security_key(
            &state, &account, &message,
            SpendAuthorization { signer_public_key: &public, signature: &signature },
            Some(&proof), H + 1, Timestamp::new(0),
        );
        prop_assert!(result.is_err());
    }
}

// ---------------------------------------------------------------------------
// Escape and cancel
// ---------------------------------------------------------------------------

#[test]
fn phrase_only_escape_lapses_the_rule_at_exactly_the_executable_height() {
    install();
    let yk = yubikey(0x81);
    let (key, account, mut state) = protected(8, &yk);
    let start = H + 10;
    // The phrase alone starts it.
    assert!(apply_via_state(&mut state, &signed_action(&key, 1, SecurityKeyActionBody::BeginEscape, &[]), start));
    let policy = stored_policy(state.authorization_declarations.get(&account).unwrap()).unwrap().clone();
    let pending = policy.pending_escape.unwrap();
    assert_eq!(pending.initiated_at_height, start);
    assert_eq!(pending.executable_at_height, start + ESCAPE_DELAY_MIN_BLOCKS);
    // A second begin is refused (no stacking, no reset of the clock).
    assert!(!apply_via_state(&mut state, &signed_action(&key, 2, SecurityKeyActionBody::BeginEscape, &[]), start + 1));

    // One block before maturity the key is still required.
    let last_locked = pending.executable_at_height - 1;
    assert!(!send(&mut state, transfer_tx(&key, 10, 0, None), last_locked));
    // At maturity the phrase alone spends.
    assert!(send(&mut state, transfer_tx(&key, 10, 0, None), pending.executable_at_height));
    assert_eq!(balance(&state, &account), 990);

    // After maturity the rule is off: a new key can be enabled with the phrase
    // and that new key alone (lost-key replacement).
    let replacement = yubikey(0x82);
    assert!(apply_via_state(&mut state, &signed_action(&key, 2, enable(&replacement), &[&replacement]), pending.executable_at_height + 1));
    assert!(!send(&mut state, transfer_tx(&key, 10, 1, None), pending.executable_at_height + 2));
    let proof = transfer_proof(&replacement, &account, 10, 1);
    assert!(send(&mut state, transfer_tx(&key, 10, 1, Some(proof)), pending.executable_at_height + 2));
}

#[test]
fn the_owner_cancels_an_escape_with_the_key_and_a_thief_cannot() {
    install();
    let yk = yubikey(0x91);
    let (key, account, mut state) = protected(9, &yk);
    assert!(apply_via_state(&mut state, &signed_action(&key, 1, SecurityKeyActionBody::BeginEscape, &[]), H + 1));
    // Phrase-only cancel (what a thief holding the phrase could send): refused.
    assert!(!apply_via_state(&mut state, &signed_action(&key, 2, SecurityKeyActionBody::CancelEscape, &[]), H + 2));
    // Cancel signed with an unregistered key: refused.
    let stranger = yubikey(0x92);
    assert!(!apply_via_state(&mut state, &signed_action(&key, 2, SecurityKeyActionBody::CancelEscape, &[&stranger]), H + 2));
    // Key-signed cancel: accepted.
    assert!(apply_via_state(&mut state, &signed_action(&key, 2, SecurityKeyActionBody::CancelEscape, &[&yk]), H + 2));
    let policy = stored_policy(state.authorization_declarations.get(&account).unwrap()).unwrap();
    assert!(policy.pending_escape.is_none());
    // Long past the old maturity the key is still required.
    assert!(!send(&mut state, transfer_tx(&key, 10, 0, None), H + 10 * ESCAPE_DELAY_MIN_BLOCKS));
    // Nothing pending: a cancel is refused.
    assert!(!apply_via_state(&mut state, &signed_action(&key, 3, SecurityKeyActionBody::CancelEscape, &[&yk]), H + 3));
}

#[test]
fn escape_delay_bounds_are_enforced() {
    install();
    let yk = yubikey(0xA1);
    let (key, _, account) = owner(10);
    let mut map = BTreeMap::new();
    let (chain, network) = ids();
    for (delay, ok) in [
        (ESCAPE_DELAY_MIN_BLOCKS - 1, false),
        (ESCAPE_DELAY_MIN_BLOCKS, true),
        (ESCAPE_DELAY_DEFAULT_BLOCKS, true),
        (ESCAPE_DELAY_MAX_BLOCKS, true),
        (ESCAPE_DELAY_MAX_BLOCKS + 1, false),
    ] {
        let mut fresh = map.clone();
        let body = SecurityKeyActionBody::Enable { key: wire(&yk), escape_delay_blocks: delay };
        let payload = signed_action(&key, 0, body, &[&yk]);
        let result = apply_action(&mut fresh, &payload, H, &chain, &network);
        assert_eq!(result.is_ok(), ok, "delay {delay}: {result:?}");
        if !ok {
            assert!(matches!(result, Err(SecurityKeyError::InvalidEscapeDelay { .. })));
        }
    }
    assert!(map.is_empty());
    let _ = account;
    map.clear();
}

// ---------------------------------------------------------------------------
// Key management
// ---------------------------------------------------------------------------

#[test]
fn enable_needs_proof_of_possession_of_the_new_key() {
    install();
    let yk = yubikey(0xB1);
    let other = yubikey(0xB2);
    let (key, _, account) = owner(11);
    let (chain, network) = ids();
    let mut map = BTreeMap::new();
    let none = signed_action(&key, 0, enable(&yk), &[]);
    assert_eq!(apply_action(&mut map, &none, H, &chain, &network), Err(SecurityKeyError::MissingAssertion("the new key")));
    let wrong = signed_action(&key, 0, enable(&yk), &[&other]);
    assert_eq!(apply_action(&mut map, &wrong, H, &chain, &network), Err(SecurityKeyError::MissingAssertion("the new key")));
    let extra = signed_action(&key, 0, enable(&yk), &[&yk, &other]);
    assert_eq!(apply_action(&mut map, &extra, H, &chain, &network), Err(SecurityKeyError::UnexpectedAssertion));
    let twice = signed_action(&key, 0, enable(&yk), &[&yk, &yk]);
    assert_eq!(apply_action(&mut map, &twice, H, &chain, &network), Err(SecurityKeyError::DuplicateAssertion));
    assert!(map.is_empty(), "every refusal leaves state untouched");
    let good = signed_action(&key, 0, enable(&yk), &[&yk]);
    assert_eq!(apply_action(&mut map, &good, H, &chain, &network), Ok(()));
    assert!(map.contains_key(&account));
    // Enabling again while enabled is refused.
    let again = signed_action(&key, 1, enable(&other), &[&other]);
    assert_eq!(apply_action(&mut map, &again, H, &chain, &network), Err(SecurityKeyError::AlreadyEnabled));
}

#[test]
fn add_needs_an_existing_key_and_the_new_key_and_caps_at_two() {
    install();
    let yk = yubikey(0xC1);
    let spare = yubikey(0xC2);
    let third = yubikey(0xC3);
    let (key, account, mut state) = protected(12, &yk);
    // Phrase + new key only (a thief adding their own key): refused.
    let thief_add = signed_action(&key, 1, SecurityKeyActionBody::AddKey { key: wire(&spare) }, &[&spare]);
    assert!(!apply_via_state(&mut state, &thief_add, H + 1));
    // Existing + new: accepted.
    let add = signed_action(&key, 1, SecurityKeyActionBody::AddKey { key: wire(&spare) }, &[&yk, &spare]);
    assert!(apply_via_state(&mut state, &add, H + 1));
    assert_eq!(stored_policy(state.authorization_declarations.get(&account).unwrap()).unwrap().keys.len(), 2);
    // Either key now co-signs a send.
    assert!(send(&mut state, transfer_tx(&key, 10, 0, Some(transfer_proof(&spare, &account, 10, 0))), H + 2));
    assert!(send(&mut state, transfer_tx(&key, 10, 1, Some(transfer_proof(&yk, &account, 10, 1))), H + 2));
    // A third key is refused.
    let add_third = signed_action(&key, 2, SecurityKeyActionBody::AddKey { key: wire(&third) }, &[&yk, &third]);
    assert!(!apply_via_state(&mut state, &add_third, H + 3));
    // Re-adding a registered key is refused.
    let dup = signed_action(&key, 2, SecurityKeyActionBody::AddKey { key: wire(&spare) }, &[&yk, &spare]);
    assert!(!apply_via_state(&mut state, &dup, H + 3));
}

#[test]
fn lost_key_is_replaced_immediately_with_the_spare() {
    install();
    let yk = yubikey(0xD1);
    let spare = yubikey(0xD2);
    let new = yubikey(0xD3);
    let (key, account, mut state) = protected(13, &yk);
    assert!(apply_via_state(&mut state, &signed_action(&key, 1, SecurityKeyActionBody::AddKey { key: wire(&spare) }, &[&yk, &spare]), H + 1));
    // `yk` is lost: remove it with the spare, add a new one with the spare.
    assert!(apply_via_state(&mut state, &signed_action(&key, 2, SecurityKeyActionBody::RemoveKey { public_key_p256: pk_hex(&yk) }, &[&spare]), H + 2));
    assert!(apply_via_state(&mut state, &signed_action(&key, 3, SecurityKeyActionBody::AddKey { key: wire(&new) }, &[&spare, &new]), H + 2));
    // The lost key no longer spends.
    assert!(!send(&mut state, transfer_tx(&key, 10, 0, Some(transfer_proof(&yk, &account, 10, 0))), H + 3));
    assert!(send(&mut state, transfer_tx(&key, 10, 0, Some(transfer_proof(&new, &account, 10, 0))), H + 3));
}

#[test]
fn removing_the_last_key_or_disable_turns_the_rule_off_and_needs_the_key() {
    install();
    let yk = yubikey(0xE1);
    let (key, account, mut state) = protected(14, &yk);
    // Phrase-only disable: refused (that is what the escape is for).
    assert!(!apply_via_state(&mut state, &signed_action(&key, 1, SecurityKeyActionBody::Disable, &[]), H + 1));
    assert!(apply_via_state(&mut state, &signed_action(&key, 1, SecurityKeyActionBody::Disable, &[&yk]), H + 1));
    let declaration = state.authorization_declarations.get(&account).unwrap();
    assert!(declaration.current.is_none());
    assert_eq!(declaration.next_nonce, 2, "the nonce never resets");
    assert!(send(&mut state, transfer_tx(&key, 10, 0, None), H + 2));

    let yk2 = yubikey(0xE2);
    assert!(apply_via_state(&mut state, &signed_action(&key, 2, enable(&yk2), &[&yk2]), H + 3));
    assert!(apply_via_state(&mut state, &signed_action(&key, 3, SecurityKeyActionBody::RemoveKey { public_key_p256: pk_hex(&yk2) }, &[&yk2]), H + 3));
    assert!(state.authorization_declarations.get(&account).unwrap().current.is_none());
}

// ---------------------------------------------------------------------------
// Replay and chain binding
// ---------------------------------------------------------------------------

#[test]
fn actions_are_single_use_and_chain_bound() {
    install();
    let yk = yubikey(0xF1);
    let (key, _, account) = owner(15);
    let (chain, network) = ids();
    let mut map = BTreeMap::new();
    let payload = signed_action(&key, 0, enable(&yk), &[&yk]);
    assert_eq!(apply_action(&mut map, &payload, H, &chain, &network), Ok(()));
    // Replaying the same signed action: stale nonce.
    assert_eq!(
        apply_action(&mut map, &payload, H + 1, &chain, &network),
        Err(SecurityKeyError::StaleNonce { expected: 1, got: 0 })
    );
    // Signed for another chain or network: the owner signature does not verify here.
    let mut fresh = BTreeMap::new();
    let foreign = signed_action_on("777", "testnet", &key, 0, enable(&yk), &[&yk]);
    if chain != "777" {
        assert_eq!(apply_action(&mut fresh, &foreign, H, &chain, &network), Err(SecurityKeyError::BadOwnerSignature));
    }
    let other_net = signed_action_on(&chain, "other-network", &key, 0, enable(&yk), &[&yk]);
    assert_eq!(apply_action(&mut fresh, &other_net, H, &chain, &network), Err(SecurityKeyError::BadOwnerSignature));
    // Owner key that does not derive the account.
    let (thief, _, _) = owner(16);
    let mut forged = signed_action(&thief, 0, enable(&yk), &[&yk]);
    forged.account = account.to_string();
    assert_eq!(apply_action(&mut fresh, &forged, H, &chain, &network), Err(SecurityKeyError::OwnerKeyMismatch));
    // Key assertion lifted from one action onto another (same nonce).
    let disable = signed_action(&key, 1, SecurityKeyActionBody::Disable, &[&yk]);
    let mut lifted = signed_action(&key, 1, SecurityKeyActionBody::CancelEscape, &[]);
    lifted.key_assertions = disable.key_assertions.clone();
    lifted.action = SecurityKeyActionBody::Disable;
    // Owner signature now covers CancelEscape bytes, not Disable: refused.
    assert_eq!(apply_action(&mut map, &lifted, H + 1, &chain, &network), Err(SecurityKeyError::BadOwnerSignature));
    assert!(fresh.is_empty());
}

#[test]
fn a_generic_declaration_cannot_overwrite_or_create_a_security_key_rule() {
    install();
    let yk = yubikey(0xF3);
    let (key, public, account) = owner(17);
    let (chain, _) = ids();
    let mut map = BTreeMap::new();
    let (c, n) = ids();
    apply_action(&mut map, &signed_action(&key, 0, enable(&yk), &[&yk]), H, &c, &n).unwrap();
    let declared = map.get(&account).unwrap().clone();
    // The phrase key alone tries to revoke through DeclareAuthorization.
    let message = declare_authorization_message(&chain, &account, &None, declared.next_nonce, declared.generation);
    let revoke = DeclareAuthorization {
        account,
        authorization: None,
        nonce: declared.next_nonce,
        generation: declared.generation,
        signer_public_key: public.to_hex(),
        signature: key.sign(&message).to_hex(),
    };
    let result = apply_declaration(&mut map, revoke, H + 1, Timestamp::new(0), &chain);
    assert!(result.is_err(), "{result:?}");
    assert_eq!(map.get(&account), Some(&declared));

    // Nor can a generic declaration create one.
    let (key2, public2, account2) = owner(18);
    let rule = AccountAuthorization::declaring(AuthorizationRule::SignerPlusSecurityKey {
        policy: stored_policy(&declared).unwrap().clone(),
    });
    let message = declare_authorization_message(&chain, &account2, &Some(rule.clone()), 0, 0);
    let create = DeclareAuthorization {
        account: account2,
        authorization: Some(rule),
        nonce: 0,
        generation: 0,
        signer_public_key: public2.to_hex(),
        signature: key2.sign(&message).to_hex(),
    };
    assert!(apply_declaration(&mut map, create, H + 1, Timestamp::new(0), &chain).is_err());
    assert!(!map.contains_key(&account2));
    let _: Option<&AccountAuthorizationDeclaration> = None;
}

// ---------------------------------------------------------------------------
// Pricing
// ---------------------------------------------------------------------------

#[test]
fn spend_pricing_is_unchanged_below_h_and_adds_one_p256_from_h() {
    install();
    let yk = yubikey(0xF5);
    let (key, _, account) = owner(19);
    let with = transfer_tx(&key, 10, 0, Some(transfer_proof(&yk, &account, 10, 0)));
    let without = transfer_tx(&key, 10, 0, None);
    for height in [0, DISPATCH, H - 1] {
        assert_eq!(with.verify_work_at_height(height), with.verify_work(), "height {height}");
        assert_eq!(without.verify_work_at_height(height), without.verify_work());
    }
    assert_eq!(with.verify_work_at_height(H).ecdsa, with.verify_work().ecdsa + 1);
    assert_eq!(without.verify_work_at_height(H), without.verify_work());
    // A malformed member is not a proof and is not charged.
    let mut junk = without.clone();
    let mut value: serde_json::Value = serde_json::from_str(junk.data.as_deref().unwrap()).unwrap();
    value[SECURITY_KEY_AUTH_MEMBER] = serde_json::json!({"public_key_p256": 7});
    junk.data = Some(value.to_string());
    assert_eq!(junk.verify_work_at_height(H), junk.verify_work());
    // An action is always charged the maximum.
    let action = action_tx(&signed_action(&key, 0, enable(&yk), &[&yk]));
    assert_eq!(action.verify_work().ecdsa, 2);
}

// ---------------------------------------------------------------------------
// Session keys (core4#97) cannot route around the security key
// ---------------------------------------------------------------------------

mod session_keys_vs_security_key {
    use super::*;
    use wrfcoin_shared_protocol::accounts::session_keys::{
        MethodId, SessionKeyAction, SessionKeyGrant, SessionScope, session_key_register_message,
    };

    fn session_grant(session: &PrivateKey) -> SessionKeyGrant {
        SessionKeyGrant {
            public_key: PublicKey::from_private_key(session),
            scope: SessionScope {
                allowed_methods: vec![MethodId("transfer".into())],
                allowed_targets: vec![TO],
                max_value_per_use: 500,
                max_total_value: 1_000,
                window: None,
            },
            valid_from: H,
            valid_until: H + 10_000,
            max_uses: None,
        }
    }

    fn register(owner_key: &PrivateKey, session: &PrivateKey, nonce: u64) -> (SessionKeyAction, Vec<u8>) {
        let owner = Address::from_public_key(&PublicKey::from_private_key(owner_key));
        let grant = session_grant(session);
        let message = session_key_register_message(&ids().0, &owner, nonce, &grant);
        let action = SessionKeyAction::Register {
            owner,
            registration_nonce: nonce,
            grant,
            owner_public_key: PublicKey::from_private_key(owner_key).to_hex(),
            owner_signature: owner_key.sign(&message).to_hex(),
            session_signature: session.sign(&message).to_hex(),
        };
        (action, message)
    }

    fn apply_register(
        state: &mut State,
        action: SessionKeyAction,
        proof: Option<SecurityKeyAssertionWire>,
        height: u64,
    ) -> bool {
        state
            .apply_transition(StateTransition::ApplySessionKeyAction {
                action,
                block_height: height,
                security_key_auth: proof,
            })
            .is_ok()
    }

    fn session_send(owner: &Address, session: &PrivateKey, amount: u64, nonce: u64) -> Transaction {
        let mut payload = TransparentTransferPayload {
            from: owner.to_string(),
            to: TO.to_string(),
            amount,
            transparent_opt_out: true,
            opt_out_audit_tag: None,
            auth_pubkey: None,
            auth_signature: None,
            authority: None,
            authority_signature: None,
            passkey_auth: None,
            sponsorship: None,
            session_key_auth: None,
        };
        payload.sign_session_key_authorization(session, nonce);
        let mut tx = Transaction::new(vec![], vec![]);
        tx.tx_type = TransactionType::Transfer;
        tx.nonce = nonce;
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();
        tx
    }

    /// The core4#97 x #103 gap: a phrase-only thief registers a session key on
    /// a protected account and spends with it. Registration needs the key.
    #[test]
    fn a_phrase_only_thief_cannot_register_a_session_key_on_a_protected_account() {
        install();
        let yk = yubikey(0x1A);
        let (key, account, mut state) = protected(30, &yk);
        let thief_session = owner(31).0;
        let (action, message) = register(&key, &thief_session, 0);
        // Phrase only (owner + session signatures, no security key): refused.
        assert!(!apply_register(&mut state, action.clone(), None, H + 1));
        assert!(state.session_keys.get(&account).is_none_or(|o| o.records.is_empty()));
        // A stranger's key: refused.
        let stranger = yubikey(0x1B);
        let forged = assertion(&stranger, &spend_challenge(&account, &message));
        assert!(!apply_register(&mut state, action.clone(), Some(forged), H + 1));
        // An assertion over a different message (a spend challenge): refused.
        let wrong = assertion(&yk, &spend_challenge(&account, b"not the registration"));
        assert!(!apply_register(&mut state, action.clone(), Some(wrong), H + 1));
        // So the session send that would have bypassed the key fails.
        assert!(!send(&mut state, session_send(&account, &thief_session, 100, 0), H + 2));
        assert_eq!(balance(&state, &account), 1_000);

        // The owner WITH the key can delegate on purpose.
        let good = assertion(&yk, &spend_challenge(&account, &message));
        assert!(apply_register(&mut state, action, Some(good), H + 1));
        assert!(send(&mut state, session_send(&account, &thief_session, 100, 0), H + 2));
        assert_eq!(balance(&state, &account), 900);
    }

    /// Session keys registered while the rule was off (a thief's, planted
    /// before the owner enabled the key) are revoked by `enable`.
    #[test]
    fn enabling_the_key_revokes_session_keys_registered_without_it() {
        install();
        let (key, _, account) = owner(32);
        let mut state = funded(account);
        let planted = owner(33).0;
        let (action, _) = register(&key, &planted, 0);
        assert!(apply_register(&mut state, action, None, H), "rule off: phrase-only registration is the #97 behaviour");
        // Before enable the planted key spends.
        assert!(send(&mut state, session_send(&account, &planted, 50, 0), H + 1));
        let yk = yubikey(0x1C);
        assert!(apply_via_state(&mut state, &signed_action(&key, 0, enable(&yk), &[&yk]), H + 2));
        let records = &state.session_keys.get(&account).unwrap().records;
        assert!(!records.is_empty() && records.values().all(|r| r.revoked), "enable revokes every existing session key");
        assert!(!send(&mut state, session_send(&account, &planted, 50, 1), H + 3));
        assert_eq!(balance(&state, &account), 950);
    }

    /// The gate is inert for an unprotected account and after a matured escape.
    #[test]
    fn the_registration_gate_only_binds_while_the_rule_is_enforced() {
        install();
        let yk = yubikey(0x1D);
        let (key, account, mut state) = protected(34, &yk);
        assert!(apply_via_state(&mut state, &signed_action(&key, 1, SecurityKeyActionBody::BeginEscape, &[]), H + 1));
        let matured = H + 1 + ESCAPE_DELAY_MIN_BLOCKS;
        let session = owner(35).0;
        let (action, _) = register(&key, &session, 0);
        assert!(!apply_register(&mut state, action.clone(), None, matured - 1));
        assert!(apply_register(&mut state, action, None, matured), "escape matured: rule off, #97 behaviour");
        let _ = account;
    }
}
