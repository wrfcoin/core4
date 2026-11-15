//! Security-key wallets (`security_key_spend`) x burned transfer fee
//! (`transfer_fee_burn`, core4#101) x session keys (core4#97), all active from
//! H in one process:
//!
//! - a protected account's phrase + key transfer pays and burns the fee like
//!   any other transfer (the key adds a requirement, never an exemption);
//! - a refused phrase-only transfer moves nothing and burns nothing;
//! - a session key cannot route around the key: registering one needs the
//!   key, so a phrase-only thief's session send never pays out.

use rand::{SeedableRng, rngs::StdRng};
use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
use wrfcoin_shared_protocol::accounts::security_key::{
    ESCAPE_DELAY_MIN_BLOCKS, SECURITY_KEY_AUTH_MEMBER, SecurityKeyActionBody,
    SecurityKeyActionPayload, SecurityKeyAssertionWire, SecurityKeyWire, action_challenge,
    action_message, spend_challenge,
};
use wrfcoin_shared_protocol::accounts::session_keys::{
    MethodId, SessionKeyAction, SessionKeyGrant, SessionScope, session_key_register_message,
};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionType, TransparentTransferPayload,
    transparent_transfer_authorization_message,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::consensus::transfer_fee::transfer_fee_at;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FeatureSchedule, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const H: u64 = 100;
const TO: Address = Address([0xC4; 20]);
const FUNDS: u128 = 1_000_000;

fn install() {
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            (ConsensusFeature::AccountAuthorizationDispatch.name().to_string(), 10),
            (ConsensusFeature::SecurityKeySpend.name().to_string(), H),
            (ConsensusFeature::TransferFeeBurn.name().to_string(), H),
            (ConsensusFeature::SessionKeys.name().to_string(), H),
        ])
        .expect("schedule"),
    );
}

fn key(seed: u64) -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(seed))
}

fn address(k: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(k))
}

fn yubikey() -> SoftwareAuthenticator {
    SoftwareAuthenticator::new(&[0x7E; 32], "wrfcoin.com")
}

fn assertion(a: &SoftwareAuthenticator, challenge: &[u8; 32]) -> SecurityKeyAssertionWire {
    let raw = a.assert(challenge);
    SecurityKeyAssertionWire {
        public_key_p256: hex::encode(a.key.public_key().to_raw_xy()),
        authenticator_data: hex::encode(raw.authenticator_data),
        client_data_json: hex::encode(raw.client_data_json),
        signature: hex::encode(raw.signature),
    }
}

fn apply(state: &mut State, tx: Transaction, height: u64) -> bool {
    state
        .apply_transition(StateTransition::ProcessGovernanceTransaction { tx, current_block: height })
        .is_ok()
}

fn protected(owner: &PrivateKey) -> State {
    let account = address(owner);
    let mut state = State::new();
    state.create_account(account, Balance::new(FUNDS));
    state.create_account(TO, Balance::new(0));
    let yk = yubikey();
    let id = ChainIdentity::active();
    let body = SecurityKeyActionBody::Enable {
        key: SecurityKeyWire {
            public_key_p256: hex::encode(yk.key.public_key().to_raw_xy()),
            rp_id_hash: hex::encode(yk.rp_id_hash),
            credential_id: "c0de".into(),
        },
        escape_delay_blocks: ESCAPE_DELAY_MIN_BLOCKS,
    };
    let message = action_message(&id.chain_id, &id.network_id, &account, 0, &body).unwrap();
    let payload = SecurityKeyActionPayload {
        account: account.to_string(),
        nonce: 0,
        action: body,
        owner_public_key: PublicKey::from_private_key(owner).to_hex(),
        owner_signature: owner.sign(&message).to_hex(),
        key_assertions: vec![assertion(&yk, &action_challenge(&message))],
    };
    let mut tx = Transaction::new(vec![], vec![]);
    tx.tx_type = TransactionType::SecurityKeyAction;
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    tx.set_id();
    assert!(apply(&mut state, tx, H));
    state
}

fn blank(from: &Address, amount: u64) -> TransparentTransferPayload {
    TransparentTransferPayload {
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
    }
}

fn wrap(payload: &TransparentTransferPayload, nonce: u64, fee: u64, proof: Option<SecurityKeyAssertionWire>) -> Transaction {
    let mut value = serde_json::to_value(payload).unwrap();
    if let Some(proof) = proof {
        value[SECURITY_KEY_AUTH_MEMBER] = serde_json::to_value(proof).unwrap();
    }
    let mut tx = Transaction::new(vec![], vec![]);
    tx.tx_type = TransactionType::Transfer;
    tx.nonce = nonce;
    tx.fee = fee;
    tx.data = Some(value.to_string());
    tx.set_id();
    tx
}

fn balance(state: &State, a: &Address) -> u128 {
    state.get_account(a).map_or(0, |x| x.balance.into_inner())
}

#[test]
fn security_key_transfers_pay_and_burn_the_fee_and_session_keys_cannot_bypass_the_key() {
    install();
    let fee = transfer_fee_at(H + 1).expect("fee in force from H");
    assert_eq!(transfer_fee_at(H - 1), None);

    let owner = key(1);
    let account = address(&owner);
    let mut state = protected(&owner);
    let burned_before = state.supply_authorized_burning;

    // Phrase only, fee declared: refused; nothing moves, nothing burns.
    let mut payload = blank(&account, 100);
    payload.sign_authorization(&owner, 0);
    assert!(!apply(&mut state, wrap(&payload, 0, fee, None), H + 1));
    assert_eq!(balance(&state, &account), FUNDS);
    assert_eq!(state.supply_authorized_burning, burned_before);

    // Phrase + key, fee declared: the transfer applies and the fee is burned.
    let message = transparent_transfer_authorization_message(&account.to_string(), &TO.to_string(), 100, 0);
    let proof = assertion(&yubikey(), &spend_challenge(&account, &message));
    assert!(apply(&mut state, wrap(&payload, 0, fee, Some(proof.clone())), H + 1));
    assert_eq!(balance(&state, &TO), 100);
    assert_eq!(balance(&state, &account), FUNDS - 100 - fee as u128, "the protected sender pays the fee");
    assert_eq!(state.supply_authorized_burning, burned_before + fee as u128, "and it is burned");

    // Phrase + key but fee below the protocol fee: refused like any transfer.
    let mut low = blank(&account, 100);
    low.sign_authorization(&owner, 1);
    let message = transparent_transfer_authorization_message(&account.to_string(), &TO.to_string(), 100, 1);
    let proof = assertion(&yubikey(), &spend_challenge(&account, &message));
    assert!(!apply(&mut state, wrap(&low, 1, fee - 1, Some(proof)), H + 2));
    assert_eq!(balance(&state, &TO), 100);

    // Session-key bypass: a phrase-only thief registers a session key...
    let session = key(2);
    let grant = SessionKeyGrant {
        public_key: PublicKey::from_private_key(&session),
        scope: SessionScope {
            allowed_methods: vec![MethodId("transfer".into())],
            allowed_targets: vec![TO],
            max_value_per_use: 10_000,
            max_total_value: 100_000,
            window: None,
        },
        valid_from: H,
        valid_until: H + 10_000,
        max_uses: None,
    };
    let reg_message = session_key_register_message(&ChainIdentity::active().chain_id, &account, 0, &grant);
    let register = SessionKeyAction::Register {
        owner: account,
        registration_nonce: 0,
        grant,
        owner_public_key: PublicKey::from_private_key(&owner).to_hex(),
        owner_signature: owner.sign(&reg_message).to_hex(),
        session_signature: session.sign(&reg_message).to_hex(),
    };
    let refused = state.apply_transition(StateTransition::ApplySessionKeyAction {
        action: register,
        block_height: H + 3,
        security_key_auth: None,
    });
    assert!(refused.is_err(), "registration without the security key is refused");
    // ...so the session send it wanted never pays out, and burns nothing.
    let mut stolen = blank(&account, 5_000);
    stolen.sign_session_key_authorization(&session, 1);
    let burned = state.supply_authorized_burning;
    assert!(!apply(&mut state, wrap(&stolen, 1, fee, None), H + 4));
    assert_eq!(balance(&state, &TO), 100);
    assert_eq!(state.supply_authorized_burning, burned);
}
