//! Chain-enforced send allowlist: every value-moving apply seam, the
//! activation boundary, time-locks through the real protocol sink
//! (`ProcessGovernanceTransaction`), state-root byte-identity while unused,
//! and the snapshot round trip.
//!
//! One test function: the feature schedule is process-wide.

use std::collections::BTreeMap;

use wrfcoin_shared_protocol::accounts::paymaster::PaymasterAction;
use wrfcoin_shared_protocol::accounts::send_allowlist::{
    AccountSendAllowlist, MIN_ADD_DELAY_BLOCKS, SendAllowlistAction, SignedSendAllowlistAction,
};
use wrfcoin_shared_protocol::accounts::session_keys::{MethodId, SessionKeyAction};
use wrfcoin_shared_protocol::blockchain::state::{
    GovernanceParameterUpdate, State, StateError, StateTransition,
};
use wrfcoin_shared_protocol::blockchain::state_snapshot::{export_state_entries, import_state_entries};
use wrfcoin_shared_protocol::blockchain::transaction::{
    ShieldedDepositPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
    TransparentTransferPayload,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, feature_active, refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const H: u64 = 10;
const A: Address = Address([0xAA; 20]);
const B: Address = Address([0xBB; 20]);

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}
fn address_of(k: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(k))
}

fn action_tx(owner: &PrivateKey, action: SendAllowlistAction, nonce: u64) -> Transaction {
    let signed = SignedSendAllowlistAction::sign(&ChainIdentity::active().chain_id, owner, action, nonce);
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("sa-action-{nonce}")),
            vout: 0,
            signature: None,
            public_key: Some(signed.public_key.clone()),
            coinbase_data: None,
        }],
        vec![TransactionOutput { amount: 0, address: signed.account.to_string(), script: "OP_TRUE".into(), nft_data: None }],
    );
    tx.tx_type = TransactionType::SendAllowlist;
    tx.sender_address = signed.account;
    tx.data = Some(serde_json::to_string(&signed).unwrap());
    tx.set_id();
    tx
}

fn transfer_tx(from: &PrivateKey, to: Address, amount: u64, nonce: u64) -> Transaction {
    let mut payload = TransparentTransferPayload {
        from: address_of(from).to_hex(),
        to: to.to_hex(),
        amount,
        transparent_opt_out: true,
        opt_out_audit_tag: None,
        auth_pubkey: None,
        auth_signature: None,
        authority: None,
        authority_signature: None,
        passkey_auth: None,
        sponsorship: None,
    };
    payload.sign_authorization(from, nonce);
    let mut tx = Transaction::new(
        vec![TransactionInput { txid: hash_string(&format!("sa-transfer-{nonce}-{to}")), vout: 0, signature: None, public_key: None, coinbase_data: None }],
        vec![TransactionOutput { amount: 0, address: address_of(from).to_hex(), script: "OP_TRUE".into(), nft_data: None }],
    );
    tx.tx_type = TransactionType::Transfer;
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    tx.nonce = nonce;
    tx.set_id();
    tx
}

fn process(state: &mut State, tx: Transaction, height: u64) -> Result<(), StateError> {
    state.apply_transition(StateTransition::ProcessGovernanceTransaction { tx, current_block: height })
}

fn refused(result: Result<(), StateError>, code: &str) {
    let err = format!("{:?}", result.expect_err("must be refused"));
    assert!(err.contains(code), "expected {code}, got {err}");
}

#[test]
fn send_allowlist_guards_time_locks_and_layout() {
    refresh_feature_schedule_extensions(vec![
        ("paymaster".to_string(), 0),
        ("send_allowlist".to_string(), H),
        ("session_keys".to_string(), 0),
    ]);
    assert!(!feature_active(ConsensusFeature::SendAllowlist, H - 1));
    assert!(feature_active(ConsensusFeature::SendAllowlist, H));

    let owner = key(0x11);
    let me = address_of(&owner);
    let mut state = State::new();
    state.create_account(me, Balance::new(1_000_000));
    state.recalculate_state_root();

    // --- Activation boundary: below H the action is refused and nothing,
    // not even the root, changes.
    let root_before = state.state_root;
    let enable = SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: vec![A] };
    assert!(matches!(process(&mut state, action_tx(&owner, enable.clone(), 0), H - 1), Err(StateError::InvalidTransition)));
    assert!(state.send_allowlists.is_empty());
    assert_eq!(state.state_root, root_before);
    assert!(
        !state.state_root_material().windows(26).any(|w| w == b"wrfcoin_send_allowlists_v1"),
        "no allowlist section while empty"
    );
    // A transfer anywhere is unaffected while no account has a policy.
    process(&mut state, transfer_tx(&owner, B, 1, 0), H - 1).unwrap();

    // --- At H: on, immediately.
    process(&mut state, action_tx(&owner, enable, 0), H).unwrap();
    assert!(state.state_root_material().windows(26).any(|w| w == b"wrfcoin_send_allowlists_v1"));
    process(&mut state, transfer_tx(&owner, A, 10, 1), H).unwrap();
    refused(process(&mut state, transfer_tx(&owner, B, 10, 2), H), "SP_SEND_ALLOWLIST_DESTINATION_REFUSED");
    let nonce_after_refusal = state.get_account(&me).unwrap().nonce;
    assert_eq!(nonce_after_refusal, 2, "a refused transfer consumes no nonce");
    process(&mut state, transfer_tx(&owner, me, 10, 2), H).unwrap();
    // Replay of the enable is refused (stale nonce).
    refused(
        process(&mut state, action_tx(&owner, SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: vec![A] }, 0), H + 1),
        "SP_SEND_ALLOWLIST_ACTION_REFUSED",
    );

    // --- Every other value seam.
    refused(
        state.apply_transition(StateTransition::ApplyShieldedDeposit {
            from: me,
            payload: ShieldedDepositPayload { shielded_commitment: "00".repeat(32), range_proof: "00".repeat(64), transparent_amount: 5 },
            current_block: H,
        }),
        "SP_SEND_ALLOWLIST_MOVEMENT_REFUSED",
    );
    refused(
        state.apply_transition(StateTransition::CreateGovernanceProposal {
            proposer: me,
            proposal_id: "sa-proposal".into(),
            update: GovernanceParameterUpdate::CommitWindow(10),
            voting_deadline: H + 100,
            deposit: Balance::new(u128::MAX / 2),
            created_at_block: H,
            current_timestamp: 0,
        }),
        "SP_SEND_ALLOWLIST_MOVEMENT_REFUSED",
    );
    refused(
        state.apply_transition(StateTransition::ApplyPaymasterAction {
            action: PaymasterAction::Stake { paymaster: me, amount: 100, unstake_delay_blocks: 100 },
            block_height: H,
        }),
        "SP_SEND_ALLOWLIST_MOVEMENT_REFUSED",
    );
    refused(
        state.apply_transition(StateTransition::ApplySessionKeyAction {
            action: SessionKeyAction::Use {
                owner: me,
                session_key: Address([0x55; 20]),
                method: MethodId("transfer".into()),
                target: B,
                value: 1,
                session_signature: String::new(),
            },
            block_height: H,
        }),
        "SP_SEND_ALLOWLIST_DESTINATION_REFUSED",
    );
    // Another account with no policy is untouched by all of this.
    let stranger = key(0x22);
    state.create_account(address_of(&stranger), Balance::new(100));
    process(&mut state, transfer_tx(&stranger, B, 1, 0), H).unwrap();

    // --- Addition is time-locked; the wallet view shows it pending.
    let t = H + 5;
    process(&mut state, action_tx(&owner, SendAllowlistAction::RequestAdd { address: B }, 1), t).unwrap();
    refused(process(&mut state, transfer_tx(&owner, B, 10, 3), t + MIN_ADD_DELAY_BLOCKS - 1), "SP_SEND_ALLOWLIST_DESTINATION_REFUSED");
    process(&mut state, transfer_tx(&owner, B, 10, 3), t + MIN_ADD_DELAY_BLOCKS).unwrap();

    // --- Remove is immediate.
    let t = t + MIN_ADD_DELAY_BLOCKS + 1;
    process(&mut state, action_tx(&owner, SendAllowlistAction::Remove { address: A }, 2), t).unwrap();
    refused(process(&mut state, transfer_tx(&owner, A, 1, 4), t), "SP_SEND_ALLOWLIST_DESTINATION_REFUSED");

    // --- Snapshot carries the map and restores the same root.
    let entries = export_state_entries(&state).unwrap();
    assert!(entries.iter().any(|(k, _)| String::from_utf8_lossy(k).contains("send_allowlists")));
    let restored = import_state_entries(&State::new(), &entries).unwrap();
    assert_eq!(restored.send_allowlists, state.send_allowlists);
    assert_eq!(restored.state_root, state.state_root);

    // --- Disable is time-locked, then lifts the policy; the nonce survives.
    process(&mut state, action_tx(&owner, SendAllowlistAction::RequestDisable, 3), t).unwrap();
    refused(process(&mut state, transfer_tx(&owner, A, 1, 4), t + MIN_ADD_DELAY_BLOCKS - 1), "SP_SEND_ALLOWLIST_DESTINATION_REFUSED");
    process(&mut state, transfer_tx(&owner, A, 1, 4), t + MIN_ADD_DELAY_BLOCKS).unwrap();
    assert_eq!(state.send_allowlists.get(&me).map(|e| e.action_nonce), Some(4));

    // --- An empty map is not exported, so a pre-activation snapshot is
    // byte-identical to one from a binary without the field.
    let mut empty = state.clone();
    empty.send_allowlists = BTreeMap::<Address, AccountSendAllowlist>::new();
    let entries = export_state_entries(&empty).unwrap();
    assert!(!entries.iter().any(|(k, _)| String::from_utf8_lossy(k).contains("send_allowlists")));
    let restored = import_state_entries(&State::new(), &entries).unwrap();
    assert!(restored.send_allowlists.is_empty());
}
