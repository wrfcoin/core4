//! Chain-888 shape: `passkey_p256_auth` is activated by an APPLIED governance
//! schedule extension (h3400), not by genesis. Mempool admission must treat
//! the feature as scheduled from the same source `feature_active` reads
//! (base ∪ applied extensions), while every height-aware consensus surface —
//! the producer self-screen, block validation and state apply — keeps
//! enforcing the exact activation height, byte-for-byte as before.
//!
//! Below the height the documented behaviour is "admitted, then screened":
//! admission is height-free, so a passkey transfer submitted before h3400 is
//! admitted once the extension is applied, and the producer self-screen
//! (`classify_block_transaction`, the same gate as `verify_block`) keeps it
//! out of every block below h3400.
//!
//! ONE test: the base schedule is a first-install-wins `OnceLock` and the
//! overlay is process-global, so this file owns its process.

use std::sync::Arc;

use rand::{SeedableRng, rngs::StdRng};
use tokio::sync::RwLock;
use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
use wrfcoin_shared_protocol::accounts::passkey::{passkey_auth_active, passkey_auth_scheduled};
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType, TransparentTransferPayload,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, FeatureSchedule, feature_active, feature_scheduled, install_feature_schedule,
    refresh_feature_schedule_extensions, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::SCHEDULE_EXTENSION_FEATURE;
use wrfcoin_shared_protocol::mempool::pool::Mempool;

const PASSKEY_HEIGHT: u64 = 3_400;
const EXTENSION_GATE: u64 = 100;
const SECRET: [u8; 32] = [
    0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f, 0x80,
    0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f, 0x90,
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

/// Produce a block at `next_height` through the real `create_next_block`
/// (producer self-screen = `classify_block_transaction`, the same per-tx gate
/// as `verify_block`) and return the packed tx ids.
async fn packed_tx_ids(
    next_height: u64,
    funded: &[Address],
    txs: &[&Transaction],
) -> Vec<Option<wrfcoin_shared_protocol::crypto::hash::Hash>> {
    let mut chain = Blockchain::default();
    chain.height = next_height - 1;
    for who in funded {
        chain.state.create_account(*who, Balance::new(FUNDED));
    }
    let chain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    {
        let mut pool = mempool.write().await;
        for tx in txs {
            let _ = pool.add_transaction((*tx).clone());
        }
    }
    let mut pop = ProofOfPublishing::new(1, 60, 0, 64);
    pop.declare_no_genesis_oracles();
    let block = pop
        .create_next_block(chain, mempool, "wrf-3400-self-screen-miner")
        .await
        .expect("the producer must seal a block");
    assert_eq!(block.height, next_height);
    block.transactions.iter().map(|t| t.id.clone()).collect()
}

#[tokio::test]
async fn extension_scheduled_passkey_is_admitted_and_still_height_gated_downstream() {
    // ── Base schedule: the extension gate only; passkey NOT in genesis ──────
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![(SCHEDULE_EXTENSION_FEATURE.to_string(), EXTENSION_GATE)])
            .unwrap(),
    );
    assert_eq!(
        try_feature_schedule()
            .unwrap()
            .activation_height(ConsensusFeature::PasskeyP256Auth.name()),
        None,
        "this file owns its process: passkey is not in the base schedule"
    );
    refresh_feature_schedule_extensions(Vec::new());

    // ── Fixtures ────────────────────────────────────────────────────────────
    let auth = SoftwareAuthenticator::new(&SECRET, "wallet.wrfcoin.test");
    let passkey_from = auth.account();
    let to = Address([0x77; 20]);
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
    };
    let passkey_tx = transfer_tx(&passkey_payload, passkey_from, "passkey-3400");

    let ed_sk = PrivateKey::generate(&mut StdRng::seed_from_u64(3400));
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
    };
    ed_payload.sign_authorization(&ed_sk, 0);
    let ed_tx = transfer_tx(&ed_payload, ed_from, "ed25519-3400");
    assert!(passkey_payload.verify_passkey_authorization(0), "the crypto is valid");

    // ── Phase 1: extension NOT yet applied — exactly today's behaviour ──────
    assert!(!passkey_auth_scheduled());
    assert!(!feature_scheduled(ConsensusFeature::PasskeyP256Auth));
    assert!(!passkey_payload.verify_authorization_for_admission(0));
    assert!(ed_payload.verify_authorization_for_admission(0));
    let mut mempool = Mempool::new(&Config::default());
    assert!(mempool.add_transaction(passkey_tx.clone()).is_err());
    mempool.add_transaction(ed_tx.clone()).expect("ed25519 admitted as before");

    // ── Phase 2: the governance extension #0 is applied (passkey @ h3400) ───
    refresh_feature_schedule_extensions(vec![(
        ConsensusFeature::PasskeyP256Auth.name().to_string(),
        PASSKEY_HEIGHT,
    )]);

    // The height-aware consensus view is exactly the extension's (unchanged
    // code path: `feature_active` already read the overlay).
    for h in [0, EXTENSION_GATE, PASSKEY_HEIGHT - 1] {
        assert!(!feature_active(ConsensusFeature::PasskeyP256Auth, h), "height {h}");
        assert!(!passkey_auth_active(h), "height {h}");
        assert!(!passkey_payload.verify_authorization_at_height(0, h), "height {h}");
        assert!(ed_payload.verify_authorization_at_height(0, h), "height {h}");
    }
    for h in [PASSKEY_HEIGHT, PASSKEY_HEIGHT + 1, 1_000_000] {
        assert!(feature_active(ConsensusFeature::PasskeyP256Auth, h), "height {h}");
        assert!(passkey_payload.verify_authorization_at_height(0, h), "height {h}");
        assert!(ed_payload.verify_authorization_at_height(0, h), "height {h}");
    }

    // Admission: scheduled via the extension ⇒ admitted. RED before the fix
    // (`passkey_auth_scheduled` read the base schedule only).
    assert!(
        passkey_auth_scheduled(),
        "an applied extension schedules passkey_p256_auth for admission"
    );
    assert!(passkey_payload.verify_authorization_for_admission(0));
    assert!(ed_payload.verify_authorization_for_admission(0), "ed25519 unchanged");
    assert!(!passkey_payload.verify_authorization(0), "still not an ed25519 authorization");
    let mut mempool = Mempool::new(&Config::default());
    mempool
        .add_transaction(passkey_tx.clone())
        .expect("extension-scheduled: the mempool admits a valid passkey transfer");
    mempool.add_transaction(ed_tx.clone()).expect("ed25519 unchanged");

    // Producer self-screen (block-validation gate): admitted-then-screened
    // below h3400, packed at h3400; ed25519 packed at both.
    let funded = [passkey_from, ed_from];
    let below = packed_tx_ids(PASSKEY_HEIGHT - 1, &funded, &[&passkey_tx, &ed_tx]).await;
    assert!(below.contains(&ed_tx.id));
    assert!(
        !below.contains(&passkey_tx.id),
        "below h3400 the self-screen drops the admitted passkey transfer"
    );
    let at = packed_tx_ids(PASSKEY_HEIGHT, &funded, &[&passkey_tx, &ed_tx]).await;
    assert!(at.contains(&ed_tx.id));
    assert!(at.contains(&passkey_tx.id), "at h3400 the self-screen packs it");

    // State apply: refused at h3399, applied at h3400.
    let mut state = State::new();
    state.create_account(passkey_from, Balance::new(FUNDED));
    state.create_account(ed_from, Balance::new(FUNDED));
    let err = apply_at(&mut state, &passkey_tx, PASSKEY_HEIGHT - 1).unwrap_err();
    assert!(err.contains("SP_PRIVACY_TRANSPARENT_TRANSFER_UNAUTHORIZED"), "{err}");
    assert_eq!(balance(&state, &passkey_from), Balance::new(FUNDED));
    apply_at(&mut state, &passkey_tx, PASSKEY_HEIGHT).expect("applied at activation");
    assert_eq!(balance(&state, &passkey_from), Balance::new(FUNDED - AMOUNT as u128));
    assert_eq!(balance(&state, &to), Balance::new(AMOUNT as u128));
    apply_at(&mut state, &ed_tx, PASSKEY_HEIGHT - 1).expect("ed25519 unaffected below");

    // An overlay entry for an UNRELATED feature schedules nothing for passkey.
    refresh_feature_schedule_extensions(vec![("session_keys".to_string(), PASSKEY_HEIGHT)]);
    assert!(!passkey_auth_scheduled());
    assert!(!passkey_payload.verify_authorization_for_admission(0));
    refresh_feature_schedule_extensions(Vec::new());
}
