//! Session keys made usable: `TransactionType::SessionKeyAction` registers /
//! revokes a scoped device key, and a transparent transfer may be authorized
//! by that key (`session_key_auth`) within its scope and limits.
//!
//! Gate: `ConsensusFeature::SessionKeys`, scheduled here at H = 5.
//! - Unscheduled (phase 1): every surface behaves exactly as before — the
//!   new tx type is refused at admission, the producer self-screen and apply;
//!   a session-only transfer is refused; an ed25519 transfer that ALSO
//!   carries a `session_key_auth` member applies to the same root as one that
//!   does not; `State::session_keys` stays empty.
//! - Scheduled (phase 2): refused below H, live from H; limits (per-use,
//!   window, lifetime, targets, revocation) hold; a failed transfer charges
//!   nothing; an owner-signed transfer never consumes a session use.
//!
//! ONE test: the feature schedule is installed process-wide, so this file
//! owns its process (the same discipline as `passkey_activation_2169`).

use std::sync::Arc;

use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::accounts::session_keys::{
    ANY_TARGET, MethodId, SessionKeyAction, SessionKeyActionPayload, SessionKeyGrant,
    SessionScope, SpendWindow, session_key_register_message, session_key_revoke_message,
    session_keys_active, session_keys_scheduled,
};
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType, TransparentTransferPayload,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

const H: u64 = 5;
const FUNDED: u128 = 1_000_000;
const PER_USE: u128 = 300;
const WINDOW_BLOCKS: u64 = 10;
const WINDOW_MAX: u128 = 500;
const LIFETIME: u128 = 900;

struct Keys {
    owner_sk: PrivateKey,
    owner: Address,
    session_sk: PrivateKey,
    session_key: Address,
}

fn keys() -> Keys {
    let mut rng = StdRng::seed_from_u64(0x5e55_1011);
    let owner_sk = PrivateKey::generate(&mut rng);
    let owner = Address::from_public_key(&PublicKey::from_private_key(&owner_sk));
    let session_sk = PrivateKey::generate(&mut rng);
    let session_key = Address::from_public_key(&PublicKey::from_private_key(&session_sk));
    Keys { owner_sk, owner, session_sk, session_key }
}

fn chain_id() -> String {
    ChainIdentity::active().chain_id.clone()
}

fn grant(k: &Keys, targets: Vec<Address>) -> SessionKeyGrant {
    SessionKeyGrant {
        public_key: PublicKey::from_private_key(&k.session_sk),
        scope: SessionScope {
            allowed_methods: vec![MethodId("transfer".into())],
            allowed_targets: targets,
            max_value_per_use: PER_USE,
            max_total_value: LIFETIME,
            window: Some(SpendWindow { blocks: WINDOW_BLOCKS, max_value: WINDOW_MAX }),
        },
        valid_from: H,
        valid_until: H + 1_000,
        max_uses: None,
    }
}

fn register_action(k: &Keys, nonce: u64, g: SessionKeyGrant) -> SessionKeyAction {
    let message = session_key_register_message(&chain_id(), &k.owner, nonce, &g);
    SessionKeyAction::Register {
        owner: k.owner,
        registration_nonce: nonce,
        grant: g,
        owner_public_key: PublicKey::from_private_key(&k.owner_sk).to_hex(),
        owner_signature: k.owner_sk.sign(&message).to_hex(),
        session_signature: k.session_sk.sign(&message).to_hex(),
    }
}

fn revoke_action(k: &Keys, registration_nonce: u64) -> SessionKeyAction {
    let message =
        session_key_revoke_message(&chain_id(), &k.owner, &k.session_key, registration_nonce);
    SessionKeyAction::Revoke {
        owner: k.owner,
        session_key: k.session_key,
        owner_public_key: PublicKey::from_private_key(&k.owner_sk).to_hex(),
        owner_signature: k.owner_sk.sign(&message).to_hex(),
    }
}

fn action_tx(action: SessionKeyAction, tag: &str) -> Transaction {
    let mut tx = base_tx(tag, "0x00");
    tx.tx_type = TransactionType::SessionKeyAction;
    tx.data = Some(serde_json::to_string(&SessionKeyActionPayload { action }).unwrap());
    tx.set_id();
    tx
}

fn base_tx(tag: &str, label: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(tag),
            vout: 0,
            signature: None,
            public_key: Some(label.to_string()),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: label.to_string(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.fee = 10_000;
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

fn transfer_tx(p: &TransparentTransferPayload, sender: Address, nonce: u64, tag: &str) -> Transaction {
    let mut tx = base_tx(tag, &p.from);
    tx.tx_type = TransactionType::Transfer;
    tx.sender_address = sender;
    tx.nonce = nonce;
    tx.data = Some(serde_json::to_string(p).unwrap());
    tx.set_id();
    tx
}

fn session_transfer(k: &Keys, to: Address, amount: u64, nonce: u64) -> Transaction {
    let mut p = payload(k.owner, to, amount);
    p.sign_session_key_authorization(&k.session_sk, nonce);
    transfer_tx(&p, k.owner, nonce, &format!("session-{to}-{amount}-{nonce}"))
}

fn apply_transfer(state: &mut State, tx: &Transaction, h: u64) -> Result<(), String> {
    state
        .apply_transition(StateTransition::ProcessGovernanceTransaction {
            tx: tx.clone(),
            current_block: h,
        })
        .map_err(|e| e.to_string())
}

fn apply_action(state: &mut State, action: SessionKeyAction, h: u64) -> Result<(), String> {
    state
        .apply_transition(StateTransition::ApplySessionKeyAction { action, block_height: h })
        .map_err(|e| e.to_string())
}

fn balance(state: &State, who: &Address) -> u128 {
    state.get_account(who).map_or(0, |a| a.balance.into_inner())
}

fn nonce_of(state: &State, who: &Address) -> u64 {
    state.get_account(who).map_or(0, |a| a.nonce)
}

async fn packed(next_height: u64, owner: Address, txs: &[Transaction]) -> Vec<Transaction> {
    let mut chain = Blockchain::default();
    chain.height = next_height - 1;
    chain.state.create_account(owner, Balance::new(FUNDED));
    let chain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    {
        let mut pool = mempool.write().await;
        for tx in txs {
            // Admission's own gate is asserted separately; here every tx is
            // forced into the pool so the self-screen is what decides.
            let _ = pool.add_transaction_with_signing_keys_at_height(
                tx.clone(),
                None,
                Some(next_height),
            );
            let _ = pool.add_transaction(tx.clone());
        }
    }
    let mut pop = ProofOfPublishing::new(1, 60, 0, 64);
    pop.declare_no_genesis_oracles();
    let block = pop
        .create_next_block(chain, mempool, "wrf-session-keys-miner")
        .await
        .expect("the producer must seal a block");
    assert_eq!(block.height, next_height);
    block.transactions
}

#[tokio::test]
async fn session_keys_are_inert_until_scheduled_then_spend_within_limits() {
    let k = keys();
    let alice = Address([0x0a; 20]);
    let bob = Address([0x0b; 20]);

    // ── Phase 1: unscheduled — byte-identical behaviour ─────────────────────
    assert!(try_feature_schedule().is_none());
    assert!(!session_keys_scheduled());
    for h in [0, H, 1_000_000] {
        assert!(!session_keys_active(h));
    }

    let reg_tx = action_tx(register_action(&k, 0, grant(&k, vec![ANY_TARGET])), "reg-0");
    let mut pool = Mempool::new(&Config::default());
    let err = pool
        .add_transaction_with_signing_keys_at_height(reg_tx.clone(), None, Some(H))
        .unwrap_err()
        .to_string();
    assert!(err.contains("SP_SESSION_KEYS_INACTIVE"), "{err}");

    let mut state = State::new();
    state.create_account(k.owner, Balance::new(FUNDED));
    state.recalculate_state_root();
    let root0 = state.state_root;
    let err = apply_action(&mut state, register_action(&k, 0, grant(&k, vec![ANY_TARGET])), H)
        .unwrap_err();
    assert!(err.contains("Invalid state transition"), "{err}");
    assert!(state.session_keys.is_empty());

    // A session-only transfer is refused everywhere, pre-schedule.
    let s_tx = session_transfer(&k, alice, 100, 0);
    let s_payload = s_tx.parse_transparent_transfer_payload().unwrap();
    assert!(s_payload.verify_session_key_signature(0), "the crypto is valid");
    assert!(!s_payload.verify_authorization_for_admission(0));
    for h in [0, H, 1_000_000] {
        assert!(!s_payload.verify_authorization_at_height(0, h));
        let err = apply_transfer(&mut state, &s_tx, h).unwrap_err();
        assert!(err.contains("SP_PRIVACY_TRANSPARENT_TRANSFER_UNAUTHORIZED"), "{err}");
    }
    state.recalculate_state_root();
    assert_eq!(state.state_root, root0, "refusals move no root");

    // An owner-signed transfer that also carries a session_key_auth member
    // lands on exactly the root of the same transfer without it.
    let mut plain = payload(k.owner, alice, 100);
    plain.sign_authorization(&k.owner_sk, 0);
    let mut with_member = plain.clone();
    with_member.sign_session_key_authorization(&k.session_sk, 0);
    let mut a = state.clone();
    let mut b = state.clone();
    apply_transfer(&mut a, &transfer_tx(&plain, k.owner, 0, "plain"), H).unwrap();
    apply_transfer(&mut b, &transfer_tx(&with_member, k.owner, 0, "member"), H).unwrap();
    assert_eq!(a.state_root, b.state_root);
    assert!(b.session_keys.is_empty());

    // A malformed member parses leniently (as absent), like passkey_auth.
    let mut raw: serde_json::Value = serde_json::to_value(&plain).unwrap();
    raw["session_key_auth"] = serde_json::json!(42);
    let mut odd = transfer_tx(&plain, k.owner, 0, "odd");
    odd.data = Some(raw.to_string());
    odd.set_id();
    let parsed = odd.parse_transparent_transfer_payload().expect("still parses");
    assert!(parsed.session_key_auth.is_none());
    assert!(parsed.verify_authorization(0));

    // Producer self-screen drops the new tx type and the session transfer.
    let block = packed(H, k.owner, &[reg_tx.clone(), s_tx.clone()]).await;
    assert!(!block.iter().any(|t| t.id == reg_tx.id));
    assert!(!block.iter().any(|t| t.id == s_tx.id));

    // ── Phase 2: schedule at H ───────────────────────────────────────────────
    let raw = format!("0:v1,{H}:{}", ConsensusFeature::SessionKeys.name());
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(session_keys_scheduled());
    assert!(!session_keys_active(H - 1));
    assert!(session_keys_active(H));

    // Admission: tip H-1 → next block H.
    Mempool::new(&Config::default())
        .add_transaction_with_signing_keys_at_height(reg_tx.clone(), None, Some(H - 1))
        .expect("admitted for the activation block");
    let forged = {
        let mut a = register_action(&k, 0, grant(&k, vec![ANY_TARGET]));
        if let SessionKeyAction::Register { owner_signature, .. } = &mut a {
            *owner_signature = k.session_sk.sign(b"not the preimage").to_hex();
        }
        action_tx(a, "forged")
    };
    let err = Mempool::new(&Config::default())
        .add_transaction_with_signing_keys_at_height(forged.clone(), None, Some(H - 1))
        .unwrap_err()
        .to_string();
    assert!(err.contains("SP_SESSION_KEYS_AUTHORIZATION"), "{err}");
    let use_tx = {
        let mut tx = base_tx("use", "0x00");
        tx.tx_type = TransactionType::SessionKeyAction;
        tx.data = Some(
            serde_json::json!({"action": {"Use": {
                "owner": k.owner, "session_key": k.session_key,
                "method": "transfer", "target": alice, "value": 1,
                "session_signature": "00"
            }}})
            .to_string(),
        );
        tx.set_id();
        tx
    };
    assert!(SessionKeyActionPayload::parse(&use_tx).is_none(), "a bare Use is never carried");

    // Producer: register packed at H, not below; forged never.
    let below = packed(H - 1, k.owner, &[reg_tx.clone()]).await;
    assert!(!below.iter().any(|t| t.id == reg_tx.id));
    let at = packed(H, k.owner, &[reg_tx.clone(), forged.clone()]).await;
    assert!(at.iter().any(|t| t.id == reg_tx.id), "register packed at H");
    assert!(!at.iter().any(|t| t.id == forged.id), "forged register screened out");

    // State: register below H refused; at H applied.
    assert!(apply_action(&mut state, register_action(&k, 0, grant(&k, vec![ANY_TARGET])), H - 1).is_err());
    apply_action(&mut state, register_action(&k, 0, grant(&k, vec![ANY_TARGET])), H).unwrap();
    assert_eq!(state.session_keys[&k.owner].next_nonce, 1);

    // Spend 300 (per-use max) to alice at H: ok, no phrase involved.
    let mut nonce = nonce_of(&state, &k.owner);
    apply_transfer(&mut state, &session_transfer(&k, alice, 300, nonce), H).unwrap();
    assert_eq!(balance(&state, &alice), 300);
    let rec = &state.session_keys[&k.owner].records[&k.session_key];
    assert_eq!((rec.uses_so_far, rec.cumulative_value_used, rec.window_value_used), (1, 300, 300));

    // Over per-use: refused, nothing moves.
    nonce += 1;
    let before = state.clone();
    let err = apply_transfer(&mut state, &session_transfer(&k, bob, 301, nonce), H).unwrap_err();
    assert!(err.contains("SP_SESSION_KEY_TRANSFER_REFUSED"), "{err}");
    assert_eq!(state.session_keys, before.session_keys);
    assert_eq!(balance(&state, &bob), 0);

    // Window: 300 used of 500, so 201 is refused and 200 fits.
    let err = apply_transfer(&mut state, &session_transfer(&k, bob, 201, nonce), H + 1).unwrap_err();
    assert!(err.contains("window"), "{err}");
    apply_transfer(&mut state, &session_transfer(&k, bob, 200, nonce), H + 1).unwrap();
    nonce += 1;

    // Next window (heights H+10..): the allowance resets.
    apply_transfer(&mut state, &session_transfer(&k, bob, 300, nonce), H + WINDOW_BLOCKS).unwrap();
    nonce += 1;
    let rec = &state.session_keys[&k.owner].records[&k.session_key];
    assert_eq!((rec.window_index, rec.window_value_used, rec.cumulative_value_used), (1, 300, 800));

    // Lifetime cap: 800 of 900 used, so 101 is refused even in a fresh window.
    let err = apply_transfer(&mut state, &session_transfer(&k, bob, 101, nonce), H + 2 * WINDOW_BLOCKS)
        .unwrap_err();
    assert!(err.contains("max_total_value"), "{err}");

    // A transfer that fails AFTER the session checks (insufficient balance)
    // charges the key nothing.
    let poor = keys();
    let mut poor_state = State::new();
    poor_state.create_account(poor.owner, Balance::new(50));
    apply_action(&mut poor_state, register_action(&poor, 0, grant(&poor, vec![ANY_TARGET])), H).unwrap();
    let before = poor_state.session_keys.clone();
    assert!(apply_transfer(&mut poor_state, &session_transfer(&poor, alice, 100, 0), H).is_err());
    assert_eq!(poor_state.session_keys, before, "a failed move spends no allowance");

    // An owner-signed transfer never consumes a session use.
    let uses = state.session_keys[&k.owner].records[&k.session_key].uses_so_far;
    let mut owner_paid = payload(k.owner, bob, 5_000);
    owner_paid.sign_authorization(&k.owner_sk, nonce);
    apply_transfer(&mut state, &transfer_tx(&owner_paid, k.owner, nonce, "owner"), H + 2).unwrap();
    nonce += 1;
    assert_eq!(state.session_keys[&k.owner].records[&k.session_key].uses_so_far, uses);

    // Revoke (owner key) → the session key can no longer spend.
    apply_action(&mut state, revoke_action(&k, 0), H + 3).unwrap();
    let err = apply_transfer(&mut state, &session_transfer(&k, bob, 1, nonce), H + 3).unwrap_err();
    assert!(err.contains("Revoked") || err.contains("revoked"), "{err}");

    // Allowlist: a key scoped to alice only cannot pay bob.
    let listed = keys();
    let mut listed_state = State::new();
    listed_state.create_account(listed.owner, Balance::new(FUNDED));
    apply_action(&mut listed_state, register_action(&listed, 0, grant(&listed, vec![alice])), H).unwrap();
    let err = apply_transfer(&mut listed_state, &session_transfer(&listed, bob, 1, 0), H).unwrap_err();
    assert!(err.contains("TargetNotAllowed") || err.contains("allowed_targets"), "{err}");
    apply_transfer(&mut listed_state, &session_transfer(&listed, alice, 1, 0), H).unwrap();

    // A different (unregistered) key's signature is refused at apply even
    // though it verifies cryptographically.
    let stranger_sk = PrivateKey::generate(&mut StdRng::seed_from_u64(99));
    let mut p = payload(listed.owner, alice, 1);
    p.sign_session_key_authorization(&stranger_sk, 1);
    let err = apply_transfer(&mut listed_state, &transfer_tx(&p, listed.owner, 1, "stranger"), H)
        .unwrap_err();
    assert!(err.contains("NotFound") || err.contains("no session key"), "{err}");
}
