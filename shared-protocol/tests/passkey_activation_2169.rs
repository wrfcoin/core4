//! #2169 activation: `ConsensusFeature::PasskeyP256Auth` scheduled at H = 5
//! flips the passkey authorization rule at exactly 5 on every surface that
//! carries a height (payload, state apply), admits at the mempool only once
//! scheduled, and leaves the ed25519 rule untouched at every height.
//!
//! ONE test: the feature schedule is installed process-wide, so this file
//! owns its process (the same discipline as `feature_schedule_parity`).

use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
use wrfcoin_shared_protocol::accounts::passkey::{passkey_auth_active, passkey_auth_scheduled};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType, TransparentTransferPayload,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion, feature_active,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;

use std::sync::Arc;
use tokio::sync::RwLock;

use rand::{SeedableRng, rngs::StdRng};

const ACTIVATION_HEIGHT: u64 = 5;
const SECRET: [u8; 32] = [
    0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f, 0x60,
    0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, 0x70,
];
const AMOUNT: u64 = 1_000;
const FUNDED: u128 = 1_000_000;

fn transfer_tx(payload: &TransparentTransferPayload, sender: Address, tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(tag),
            vout: 0,
            signature: None,
            public_key: Some(payload.from.clone()),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: payload.from.clone(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::Transfer;
    tx.sender_address = sender;
    tx.nonce = 0;
    tx.fee = 10_000;
    tx.data = Some(serde_json::to_string(payload).unwrap());
    tx.set_id();
    tx
}

fn balance(state: &State, who: &Address) -> Balance {
    state
        .get_account(who)
        .map(|a| a.balance)
        .unwrap_or(Balance::new(0))
}

fn apply_at(state: &mut State, tx: &Transaction, current_block: u64) -> Result<(), String> {
    state
        .apply_transition(StateTransition::ProcessGovernanceTransaction {
            tx: tx.clone(),
            current_block,
        })
        .map_err(|e| e.to_string())
}

#[tokio::test]
async fn passkey_auth_scheduled_at_five_flips_at_five_and_leaves_ed25519_alone() {
    // ── Fixtures: one passkey account, one ed25519 account, same recipient ──
    let auth = SoftwareAuthenticator::new(&SECRET, "wallet.wrfcoin.test");
    let passkey_from = auth.account();
    let to = Address([0x66; 20]);
    let passkey_payload = TransparentTransferPayload {
        from: passkey_from.to_hex(),
        to: to.to_hex(),
        amount: AMOUNT,
        transparent_opt_out: true,
        opt_out_audit_tag: None,
        auth_pubkey: None,
        auth_signature: None,
        authority: None,
        authority_signature: None,
        passkey_auth: Some(auth.authorize_transfer(&to.to_hex(), AMOUNT, 0)),
        sponsorship: None,
        session_key_auth: None,
    };
    let passkey_tx = transfer_tx(&passkey_payload, passkey_from, "passkey-2169");

    let mut rng = StdRng::seed_from_u64(2169);
    let ed_sk = PrivateKey::generate(&mut rng);
    let ed_from = Address::from_public_key(&PublicKey::from_private_key(&ed_sk));
    let mut ed_payload = TransparentTransferPayload {
        from: ed_from.to_hex(),
        to: to.to_hex(),
        amount: AMOUNT,
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
    ed_payload.sign_authorization(&ed_sk, 0);
    let ed_tx = transfer_tx(&ed_payload, ed_from, "ed25519-2169");

    // ── Phase 1: no schedule installed — exactly today's behaviour ──────────
    assert!(try_feature_schedule().is_none());
    assert!(!passkey_auth_scheduled());
    assert!(
        passkey_payload.verify_passkey_authorization(0),
        "the crypto is valid"
    );
    assert!(
        !passkey_payload.verify_authorization(0),
        "but it is not an ed25519 authorization"
    );
    for h in [0, ACTIVATION_HEIGHT - 1, ACTIVATION_HEIGHT, 1_000_000] {
        assert!(!passkey_auth_active(h), "height {h}");
        assert!(
            !passkey_payload.verify_authorization_at_height(0, h),
            "height {h}"
        );
        assert!(
            ed_payload.verify_authorization_at_height(0, h),
            "height {h}"
        );
    }
    assert!(!passkey_payload.verify_authorization_for_admission(0));
    assert!(ed_payload.verify_authorization_for_admission(0));

    let mut mempool = Mempool::new(&Config::default());
    assert!(
        mempool.add_transaction(passkey_tx.clone()).is_err(),
        "unscheduled: the mempool refuses a passkey-only transfer"
    );
    mempool
        .add_transaction(ed_tx.clone())
        .expect("unscheduled: the mempool admits the ed25519 transfer as before");

    let mut state = State::new();
    state.create_account(passkey_from, Balance::new(FUNDED));
    state.create_account(ed_from, Balance::new(FUNDED));
    state.recalculate_state_root();
    let root_untouched = state.state_root;
    for h in [0, ACTIVATION_HEIGHT, 1_000_000] {
        let err = apply_at(&mut state, &passkey_tx, h).unwrap_err();
        assert!(
            err.contains("SP_PRIVACY_TRANSPARENT_TRANSFER_UNAUTHORIZED"),
            "height {h}: {err}"
        );
    }
    assert_eq!(balance(&state, &passkey_from), Balance::new(FUNDED));
    assert_eq!(balance(&state, &to), Balance::new(0));
    state.recalculate_state_root();
    assert_eq!(
        state.state_root, root_untouched,
        "a refused transfer moves no root"
    );

    // ── Phase 2: schedule the feature at H, as the boot gate would ──────────
    let raw = format!(
        "0:v1,{ACTIVATION_HEIGHT}:{}",
        ConsensusFeature::PasskeyP256Auth.name()
    );
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(passkey_auth_scheduled());

    // The flip is at H on the one view every surface consults.
    for h in 0..ACTIVATION_HEIGHT {
        assert!(
            !feature_active(ConsensusFeature::PasskeyP256Auth, h),
            "height {h}"
        );
        assert!(
            !passkey_payload.verify_authorization_at_height(0, h),
            "height {h}"
        );
    }
    for h in ACTIVATION_HEIGHT..ACTIVATION_HEIGHT + 5 {
        assert!(
            feature_active(ConsensusFeature::PasskeyP256Auth, h),
            "height {h}"
        );
        assert!(
            passkey_payload.verify_authorization_at_height(0, h),
            "height {h}"
        );
    }
    // Neither the signing version nor the ed25519 rule moved.
    for h in 0..ACTIVATION_HEIGHT + 5 {
        assert!(!feature_active(ConsensusFeature::SigningV2, h));
        assert!(!feature_active(
            ConsensusFeature::DomainSeparatedPreimages,
            h
        ));
        assert!(ed_payload.verify_authorization_at_height(0, h));
        assert!(!passkey_payload.verify_authorization(0));
    }

    // Mempool: scheduled ⇒ admitted (the height gate is downstream).
    assert!(passkey_payload.verify_authorization_for_admission(0));
    let mut mempool = Mempool::new(&Config::default());
    mempool
        .add_transaction(passkey_tx.clone())
        .expect("scheduled: the mempool admits a valid passkey transfer");

    // State apply: refused at H-1, applied at H, and the debit lands on the
    // passkey account.
    let err = apply_at(&mut state, &passkey_tx, ACTIVATION_HEIGHT - 1).unwrap_err();
    assert!(
        err.contains("SP_PRIVACY_TRANSPARENT_TRANSFER_UNAUTHORIZED"),
        "{err}"
    );
    assert_eq!(balance(&state, &passkey_from), Balance::new(FUNDED));

    apply_at(&mut state, &passkey_tx, ACTIVATION_HEIGHT).expect("applied at activation");
    assert_eq!(
        balance(&state, &passkey_from),
        Balance::new(FUNDED - AMOUNT as u128)
    );
    assert_eq!(balance(&state, &to), Balance::new(AMOUNT as u128));

    // Nonce replay: the same authorization cannot be applied twice.
    assert!(apply_at(&mut state, &passkey_tx, ACTIVATION_HEIGHT + 1).is_err());
    assert_eq!(
        balance(&state, &passkey_from),
        Balance::new(FUNDED - AMOUNT as u128)
    );

    // The ed25519 account transacts identically after activation.
    apply_at(&mut state, &ed_tx, ACTIVATION_HEIGHT + 1).expect("ed25519 unaffected");
    assert_eq!(
        balance(&state, &ed_from),
        Balance::new(FUNDED - AMOUNT as u128)
    );
    assert_eq!(balance(&state, &to), Balance::new(2 * AMOUNT as u128));

    // ── Phase 3: the BLOCK-VALIDATION surface (producer self-screen) ────────
    // The payload / mempool / state-apply surfaces above are all height-gated,
    // but the producer self-screen is its own consensus surface: block assembly
    // runs `classify_block_transaction` (the SAME per-tx gate as `verify_block`,
    // #1730) at `next_height` and drops any transaction it refuses, rather than
    // sealing an invalid block. Nothing above pins that path, so it could stay
    // ed25519-only after activation and no test would notice.
    //
    // Pin it end-to-end through the real `create_next_block`: a passkey-only
    // transfer is screened OUT of a block produced below H and packed IN at H,
    // while the ed25519 control is packed at both. RED if `classify` drops the
    // height-aware passkey check (ed25519-only -> passkey screened at H too) or
    // if the height gate is removed (passkey packed below H).
    async fn packed_tx_ids(
        next_height: u64,
        passkey_from: Address,
        ed_from: Address,
        passkey_tx: &Transaction,
        ed_tx: &Transaction,
    ) -> Vec<Option<wrfcoin_shared_protocol::crypto::hash::Hash>> {
        let mut chain = Blockchain::default();
        chain.height = next_height - 1;
        // Fund both senders so a transfer is dropped only by the authorization
        // gate under test, never for insufficient balance.
        chain.state.create_account(passkey_from, Balance::new(FUNDED));
        chain.state.create_account(ed_from, Balance::new(FUNDED));
        let chain = Arc::new(RwLock::new(chain));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        {
            let mut pool = mempool.write().await;
            // The mempool admits both once the feature is scheduled (the exact
            // height gate is the self-screen's job, exercised below).
            let _ = pool.add_transaction(passkey_tx.clone());
            let _ = pool.add_transaction(ed_tx.clone());
        }
        let mut pop = ProofOfPublishing::new(1, 60, 0, 64);
        pop.declare_no_genesis_oracles();
        let block = pop
            .create_next_block(chain, mempool, "wrf-2169-self-screen-miner")
            .await
            .expect("the producer must seal a block");
        assert_eq!(block.height, next_height, "produced at the requested height");
        block.transactions.iter().map(|t| t.id.clone()).collect()
    }

    // Below activation: the ed25519 control is packed, the passkey transfer is
    // self-screened out.
    let below = packed_tx_ids(
        ACTIVATION_HEIGHT - 1,
        passkey_from,
        ed_from,
        &passkey_tx,
        &ed_tx,
    )
    .await;
    assert!(
        below.contains(&ed_tx.id),
        "below activation the producer packs the ed25519 transfer"
    );
    assert!(
        !below.contains(&passkey_tx.id),
        "below activation the producer self-screen drops the passkey-only transfer"
    );

    // At activation: both are packed — the self-screen now admits the passkey
    // transfer through the height-aware `classify` gate.
    let at = packed_tx_ids(
        ACTIVATION_HEIGHT,
        passkey_from,
        ed_from,
        &passkey_tx,
        &ed_tx,
    )
    .await;
    assert!(
        at.contains(&ed_tx.id),
        "at activation the producer packs the ed25519 transfer"
    );
    assert!(
        at.contains(&passkey_tx.id),
        "at activation the producer self-screen packs the valid passkey transfer"
    );
}
