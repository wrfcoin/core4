//! Send allowlist with the other spend paths (#97 session keys, #101 burned
//! transfer fee): a session key cannot route around an owner's list, a
//! refused session-key send spends none of the key's allowance, and a
//! permitted send from a protected account still pays (burns) the protocol
//! fee.
//!
//! One test function: the feature schedule is process-wide.

use rand::{SeedableRng, rngs::StdRng};
use wrfcoin_shared_protocol::accounts::send_allowlist::{
    MIN_ADD_DELAY_BLOCKS, SendAllowlistAction, SignedSendAllowlistAction,
};
use wrfcoin_shared_protocol::accounts::session_keys::{
    ANY_TARGET, MethodId, SessionKeyAction, SessionKeyGrant, SessionScope,
    session_key_register_message,
};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType, TransparentTransferPayload,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::consensus::transfer_fee::transfer_fee_at;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const H: u64 = 5;
const A: Address = Address([0xAA; 20]);
const B: Address = Address([0xBB; 20]);

fn tx_of(tx_type: TransactionType, sender: Address, nonce: u64, fee: u64, data: String, tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput { txid: hash_string(tag), vout: 0, signature: None, public_key: None, coinbase_data: None }],
        vec![TransactionOutput { amount: 0, address: sender.to_hex(), script: "OP_TRUE".into(), nft_data: None }],
    );
    tx.tx_type = tx_type;
    tx.sender_address = sender;
    tx.nonce = nonce;
    tx.fee = fee;
    tx.data = Some(data);
    tx.set_id();
    tx
}

fn payload(from: Address, to: Address, amount: u64) -> TransparentTransferPayload {
    TransparentTransferPayload {
        from: from.to_hex(),
        to: to.to_hex(),
        amount,
        transparent_opt_out: true,
        opt_out_audit_tag: None,
        auth_pubkey: None,
        auth_signature: None,
        authority: None,
        authority_signature: None,
        passkey_auth: None,
        session_key_auth: None,
        sponsorship: None,
    }
}

fn process(state: &mut State, tx: Transaction, h: u64) -> Result<(), String> {
    state
        .apply_transition(StateTransition::ProcessGovernanceTransaction { tx, current_block: h })
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn session_keys_and_fee_burn_obey_the_send_allowlist() {
    refresh_feature_schedule_extensions(vec![
        ("send_allowlist".to_string(), H),
        ("session_keys".to_string(), H),
        ("transfer_fee_burn".to_string(), H),
    ]);
    let fee = transfer_fee_at(H).expect("transfer fee in force at H");

    let mut rng = StdRng::seed_from_u64(0x5a11_0f);
    let owner_sk = PrivateKey::generate(&mut rng);
    let owner = Address::from_public_key(&PublicKey::from_private_key(&owner_sk));
    let session_sk = PrivateKey::generate(&mut rng);
    let chain_id = ChainIdentity::active().chain_id.clone();

    let mut state = State::new();
    state.create_account(owner, Balance::new(1_000_000));
    state.recalculate_state_root();

    // A session key allowed to pay anyone.
    let grant = SessionKeyGrant {
        public_key: PublicKey::from_private_key(&session_sk),
        scope: SessionScope {
            allowed_methods: vec![MethodId("transfer".into())],
            allowed_targets: vec![ANY_TARGET],
            max_value_per_use: 1_000,
            max_total_value: 10_000,
            window: None,
        },
        valid_from: H,
        valid_until: H + 10_000,
        max_uses: None,
    };
    let message = session_key_register_message(&chain_id, &owner, 0, &grant);
    state
        .apply_transition(StateTransition::ApplySessionKeyAction {
            action: SessionKeyAction::Register {
                owner,
                registration_nonce: 0,
                grant,
                owner_public_key: PublicKey::from_private_key(&owner_sk).to_hex(),
                owner_signature: owner_sk.sign(&message).to_hex(),
                session_signature: session_sk.sign(&message).to_hex(),
            },
            block_height: H,
            security_key_auth: None,
        })
        .expect("session key registers");

    // Owner turns the allowlist on with A only.
    let signed = SignedSendAllowlistAction::sign(
        &chain_id,
        &owner_sk,
        SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: vec![A] },
        0,
    );
    process(
        &mut state,
        tx_of(TransactionType::SendAllowlist, owner, 0, 0, serde_json::to_string(&signed).unwrap(), "enable"),
        H,
    )
    .expect("enable");

    // Session-key send to B: refused, and the key's allowance is untouched.
    let grants_before = state.session_keys.clone();
    let balance_before = state.get_account(&owner).unwrap().balance;
    let mut to_b = payload(owner, B, 100);
    to_b.sign_session_key_authorization(&session_sk, 0);
    let err = process(&mut state, tx_of(TransactionType::Transfer, owner, 0, fee, serde_json::to_string(&to_b).unwrap(), "sk-b"), H + 1)
        .expect_err("a session key cannot route around the list");
    assert!(err.contains("SP_SEND_ALLOWLIST_DESTINATION_REFUSED"), "{err}");
    assert_eq!(state.session_keys, grants_before, "a refused send spends no session allowance");
    assert_eq!(state.get_account(&owner).unwrap().balance, balance_before, "nothing debited, not even the fee");

    // Session-key send to A: applies, and the protocol fee is still burned.
    let burned_before = state.supply_authorized_burning;
    let mut to_a = payload(owner, A, 100);
    to_a.sign_session_key_authorization(&session_sk, 0);
    process(&mut state, tx_of(TransactionType::Transfer, owner, 0, fee, serde_json::to_string(&to_a).unwrap(), "sk-a"), H + 1)
        .expect("a listed destination is paid");
    assert_eq!(state.get_account(&A).unwrap().balance, Balance::new(100));
    assert_eq!(
        state.get_account(&owner).unwrap().balance,
        balance_before.checked_sub(Balance::new(100 + u128::from(fee))).unwrap(),
        "amount plus the protocol fee"
    );
    assert_eq!(state.supply_authorized_burning, burned_before + u128::from(fee), "the fee is burned");

    // Owner-signed send to B: refused the same way.
    let mut owner_to_b = payload(owner, B, 10);
    owner_to_b.sign_authorization(&owner_sk, 1);
    let err = process(&mut state, tx_of(TransactionType::Transfer, owner, 1, fee, serde_json::to_string(&owner_to_b).unwrap(), "own-b"), H + 2)
        .expect_err("refused");
    assert!(err.contains("SP_SEND_ALLOWLIST_DESTINATION_REFUSED"), "{err}");
}
