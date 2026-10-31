//! Security-key wallets (`security_key_spend`): producer / applier / WAL parity.
//!
//! Two child processes (the feature schedule is process-wide):
//! - **gate off**: a `SecurityKeyAction` is refused at mempool admission and
//!   never reaches a block; a phrase-only transfer applies as on main.
//! - **gate on** (`account_authorization_dispatch` at 2, `security_key_spend`
//!   at H = 4): an action submitted below H never lands; at H the real
//!   producer (`create_next_block`) includes an `enable`; a phrase-only
//!   transfer from the protected account is dropped at assembly while a
//!   phrase + key transfer applies; a phrase-only `begin_escape` and a
//!   key-signed `cancel_escape` apply.
//!
//! At every height the P2P applier (`extract_transitions` →
//! `apply_transitions_batch_at_height`) and the producer agree on
//! `state_root`, balances and the account's declaration, and WAL replay
//! reproduces the producer's final root.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;

mod helpers {
    pub(crate) mod test_env {
        include!("../src/bin/launch_api/test_env.rs");
    }
}

use std::sync::Arc;

use tokio::sync::RwLock;
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
use wrfcoin_shared_protocol::accounts::security_key::{
    ESCAPE_DELAY_MIN_BLOCKS, SECURITY_KEY_AUTH_MEMBER, SecurityKeyActionBody,
    SecurityKeyActionPayload, SecurityKeyAssertionWire, SecurityKeyWire, action_challenge,
    action_message, spend_challenge, stored_policy,
};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType, TransparentTransferPayload,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, GENESIS_CHAIN_ID_KEY, feature_active,
    refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const H: u64 = 4;
const CHILD_ENV: &str = "WRFCOIN_SECURITY_KEY_SPEND_CHILD";
const TEST_NAME: &str = "security_key_spend_producer_applier_wal";
const TO: Address = Address([0xB7; 20]);

fn owner_key() -> PrivateKey {
    PrivateKey::from_bytes(&[0x5C; 32]).expect("32-byte ed25519 seed")
}

fn owner() -> Address {
    Address::from_public_key(&PublicKey::from_private_key(&owner_key()))
}

fn yubikey() -> SoftwareAuthenticator {
    SoftwareAuthenticator::new(&[0x6D; 32], "wrfcoin.com")
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

fn envelope(tag: &str, tx_type: TransactionType, data: String, nonce: u64) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("security-key-input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: owner().to_string(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = tx_type;
    tx.nonce = nonce;
    tx.sender_address = owner();
    tx.data = Some(data);
    tx.set_id();
    tx
}

fn action(tag: &str, nonce: u64, body: SecurityKeyActionBody, with_key: bool) -> Transaction {
    let identity = ChainIdentity::active();
    let key = owner_key();
    let message = action_message(&identity.chain_id, &identity.network_id, &owner(), nonce, &body).unwrap();
    let challenge = action_challenge(&message);
    let payload = SecurityKeyActionPayload {
        account: owner().to_string(),
        nonce,
        action: body,
        owner_public_key: PublicKey::from_private_key(&key).to_hex(),
        owner_signature: key.sign(&message).to_hex(),
        key_assertions: if with_key { vec![assertion(&yubikey(), &challenge)] } else { vec![] },
    };
    envelope(tag, TransactionType::SecurityKeyAction, serde_json::to_string(&payload).unwrap(), 0)
}

fn enable(tag: &str) -> Transaction {
    let yk = yubikey();
    let body = SecurityKeyActionBody::Enable {
        key: SecurityKeyWire {
            public_key_p256: hex::encode(yk.key.public_key().to_raw_xy()),
            rp_id_hash: hex::encode(yk.rp_id_hash),
            credential_id: "c0de".into(),
        },
        escape_delay_blocks: ESCAPE_DELAY_MIN_BLOCKS,
    };
    action(tag, 0, body, true)
}

fn transfer(tag: &str, amount: u64, nonce: u64, with_key: bool) -> Transaction {
    let key = owner_key();
    let mut payload = TransparentTransferPayload {
        from: owner().to_string(),
        to: TO.to_string(),
        amount,
        transparent_opt_out: true,
        opt_out_audit_tag: None,
        auth_pubkey: None,
        auth_signature: None,
        authority: None,
        authority_signature: None,
        passkey_auth: None,
    };
    payload.sign_authorization(&key, nonce);
    let mut value = serde_json::to_value(&payload).unwrap();
    if with_key {
        let challenge = spend_challenge(&owner(), &payload.authorization_message(nonce));
        value[SECURITY_KEY_AUTH_MEMBER] = serde_json::to_value(assertion(&yubikey(), &challenge)).unwrap();
    }
    envelope(tag, TransactionType::Transfer, value.to_string(), nonce)
}

fn balance(state: &State, address: &Address) -> u128 {
    state.get_account(address).map(|a| a.balance.into_inner()).unwrap_or(0)
}

#[test]
fn security_key_spend_producer_applier_wal() {
    let Some(mode) = std::env::var_os(CHILD_ENV) else {
        for mode in ["off", "on"] {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(CHILD_ENV, mode)
                .env("WRFCOIN_CHAIN_ID", "777")
                .env("WRFCOIN_NETWORK_ID", "testnet")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success() && stdout.contains("1 passed"),
                "child ({mode}) failed:\n{stdout}\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        return;
    };
    let on = mode == "on";
    if on {
        refresh_feature_schedule_extensions(vec![
            (ConsensusFeature::AccountAuthorizationDispatch.name().to_string(), 2),
            (ConsensusFeature::SecurityKeySpend.name().to_string(), H),
        ]);
    }
    assert_eq!(feature_active(ConsensusFeature::SecurityKeySpend, H), on);
    assert!(!feature_active(ConsensusFeature::SecurityKeySpend, H - 1));
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(on));
}

async fn run(on: bool) {
    let mut genesis = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    genesis.metadata.insert(GENESIS_CHAIN_ID_KEY.into(), "777".into());
    let mut genesis_transitions = extract_transitions(&State::new(), &genesis, 1, 0);
    // Fund the owner through a WAL-recorded transition so replay sees it too.
    genesis_transitions.push(StateTransition::ApplyReward {
        address: owner(),
        amount: Balance::new(1_000),
        reason: "security-key fixture".to_string(),
    });
    let mut applier = State::new();
    applier.apply_transitions_batch_at_height(1, genesis_transitions.clone()).unwrap();
    assert_eq!(balance(&applier, &owner()), 1_000);
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    {
        let mut c = chain.write().await;
        c.height = 1;
        c.state = applier.clone();
    }
    let db = std::env::temp_dir().join(format!("security-key-spend-{}-{}.redb", std::process::id(), on));
    let _ = std::fs::remove_file(&db);
    let mut storage = Storage::open(&db, DEFAULT_CHECKPOINT_INTERVAL).unwrap();
    storage.persist_block(genesis, genesis_transitions, None).await.unwrap();
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let miner = Address([0x41; 20]).to_string();

    let plan: Vec<(u64, Vec<(&str, Transaction)>)> = vec![
        (2, vec![("enable-below-h", enable("enable-below-h"))]),
        (3, vec![]),
        (4, vec![("enable-at-h", enable("enable-at-h"))]),
        (5, vec![("phrase-only", transfer("phrase-only", 10, 0, false))]),
        (6, vec![("phrase-and-key", transfer("phrase-and-key", 10, 0, true))]),
        (7, vec![("begin-escape", action("begin-escape", 1, SecurityKeyActionBody::BeginEscape, false))]),
        (8, vec![("cancel-escape", action("cancel-escape", 2, SecurityKeyActionBody::CancelEscape, true))]),
    ];
    let last = 8;
    let mut admitted = Vec::new();
    for (height, txs) in plan {
        for (tag, tx) in txs {
            let keys = chain.read().await.state.signing_keys.clone();
            match mempool
                .write()
                .await
                .add_transaction_with_signing_keys_at_height(tx, Some(&keys), Some(height - 1))
            {
                Ok(()) => admitted.push(tag),
                Err(e) => eprintln!("admission refused {tag} at {height}: {e}"),
            }
        }
        let before = applier.clone();
        let block = pop
            .create_next_block(chain.clone(), mempool.clone(), &miner)
            .await
            .expect("the real producer builds the block");
        assert_eq!(block.height, height);
        let has_action = block.transactions.iter().any(|t| t.tx_type == TransactionType::SecurityKeyAction);
        assert!(!(has_action && height < H), "no SecurityKeyAction may land below H (height {height})");
        let producer = chain.read().await.state.clone();
        let transitions = extract_transitions(&before, &block, block.height, block.timestamp.timestamp());
        applier.apply_transitions_batch_at_height(block.height, transitions.clone()).unwrap();
        storage.persist_block(block, transitions, None).await.unwrap();
        assert_eq!(applier.state_root, producer.state_root, "applier root at {height}");
        assert_eq!(applier.authorization_declarations, producer.authorization_declarations, "declarations at {height}");
        assert_eq!(balance(&applier, &owner()), balance(&producer, &owner()), "balance at {height}");

        let declared = producer.authorization_declarations.get(&owner()).cloned();
        match (on, height) {
            (false, _) => assert!(producer.authorization_declarations.is_empty()),
            (true, h) if h < H => assert!(declared.is_none(), "nothing stored below H"),
            (true, 4) => {
                let policy = stored_policy(declared.as_ref().unwrap()).unwrap().clone();
                assert_eq!(policy.keys.len(), 1);
            },
            (true, 5) => assert_eq!(balance(&producer, &owner()), 1_000, "phrase-only send dropped"),
            (true, 6) => assert_eq!(balance(&producer, &owner()), 990, "phrase + key send applied"),
            (true, 7) => {
                let policy = stored_policy(declared.as_ref().unwrap()).unwrap().clone();
                assert_eq!(policy.pending_escape.map(|e| e.executable_at_height), Some(7 + ESCAPE_DELAY_MIN_BLOCKS));
            },
            (true, 8) => {
                let policy = stored_policy(declared.as_ref().unwrap()).unwrap().clone();
                assert!(policy.pending_escape.is_none(), "key-signed cancel applied");
            },
            _ => {},
        }
    }
    if on {
        assert!(admitted.contains(&"enable-at-h"));
    } else {
        assert!(!admitted.iter().any(|t| t.starts_with("enable") || t.ends_with("escape")), "gate off: actions refused at admission");
        let producer = chain.read().await.state.clone();
        assert_eq!(balance(&producer, &owner()), 990, "gate off: the phrase-only send applies as on main");
    }
    let producer = chain.read().await.state.clone();

    let rows = storage.load_wal_transitions_range(1, last).unwrap();
    let mut wal = State::new();
    for height in 1..=last {
        wal.apply_transitions_batch_at_height(
            height,
            rows.iter().filter(|(h, _)| *h == height).map(|(_, t)| t.clone()),
        )
        .unwrap();
    }
    assert_eq!(wal.state_root, producer.state_root, "WAL replay root");
    assert_eq!(wal.authorization_declarations, producer.authorization_declarations);

    drop(storage);
    std::fs::remove_file(db).unwrap();
}
