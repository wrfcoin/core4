//! Chain-enforced send allowlist (`send_allowlist`): activation boundary and
//! producer / applier / WAL-replay parity through the real block producer.
//!
//! Two child processes (the feature schedule is process-wide):
//! - **gate off**: a `SendAllowlist` action is refused by admission and a block
//!   carrying one is invalid at every height; transfers apply as on main and
//!   `State::send_allowlists` stays empty, so the root has no allowlist section.
//! - **gate on (H = 3)**: below H the same refusals hold. At H the owner turns
//!   the policy on with one allowed address A. From then a transfer to A
//!   applies, a transfer to B is dropped by the producer and would fail a
//!   peer's apply, and a requested addition of B is pending (not yet usable).
//!   Producer (`create_next_block`), applier (`extract_transitions` →
//!   `apply_transitions_batch_at_height`) and WAL replay agree on `state_root`
//!   and on every balance at every height.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;

mod helpers {
    pub(crate) mod test_env {
        // Mount the canonical test-only lock and fixture pins (#3521).
        include!("../src/bin/launch_api/test_env.rs");
    }
}

use std::sync::Arc;

use rustc_hash::FxHashSet;
use tokio::sync::RwLock;
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::consensus::proof_of_publishing::{ProofOfPublishing, adjusted_block_reward};
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::accounts::send_allowlist::{
    MIN_ADD_DELAY_BLOCKS, SendAllowlistAction, SignedSendAllowlistAction, view,
};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{PopLivenessPolicy, State, StateTransition};
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

const H: u64 = 3;
const FEATURE: &str = "send_allowlist";
const CHILD_ENV: &str = "WRFCOIN_SEND_ALLOWLIST_CHILD";
const TEST_NAME: &str = "send_allowlist_producer_applier_wal";

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn address_of(k: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(k))
}
const A: Address = Address([0xAA; 20]);
const B: Address = Address([0xBB; 20]);

fn transfer(from: &PrivateKey, to: Address, amount: u64, nonce: u64, tag: &str) -> Transaction {
    let from_hex = address_of(from).to_hex();
    let mut payload = TransparentTransferPayload {
        from: from_hex.clone(),
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
        vec![TransactionInput {
            txid: hash_string(&format!("send-allowlist-transfer-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: from_hex,
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::Transfer;
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    tx.nonce = nonce;
    tx.set_id();
    tx
}

fn action(owner: &PrivateKey, action: SendAllowlistAction, nonce: u64, tag: &str) -> Transaction {
    let signed =
        SignedSendAllowlistAction::sign(&ChainIdentity::active().chain_id, owner, action, nonce);
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("send-allowlist-action-{tag}")),
            vout: 0,
            signature: None,
            public_key: Some(signed.public_key.clone()),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: signed.account.to_string(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::SendAllowlist;
    tx.sender_address = signed.account;
    tx.nonce = nonce;
    tx.data = Some(serde_json::to_string(&signed).unwrap());
    tx.set_id();
    tx
}

/// The #1762 recipe: a hand-built block carrying `tx`, accepted by every gate
/// except the one under test.
async fn verify_block_with(tx: Transaction, height: u64) -> bool {
    let txs = vec![
        Transaction::new_coinbase("send-allowlist-miner", adjusted_block_reward(height), height),
        tx,
    ];
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    pop.set_pop_liveness_policy(PopLivenessPolicy { useful_work_activation_height: 0 })
        .expect("liveness policy");
    let score = pop.calculate_block_publishing_score(&txs, height);
    let mut block = Block::new(3, Hash::default(), txs, 1, height);
    block.metadata.insert("publishing_score".to_string(), format!("{score}"));
    pop.verify_block(&block, &FxHashSet::default(), 2, &[])
        .await
        .expect("verify_block returns a verdict")
}

fn balance(state: &State, address: &Address) -> u128 {
    state.get_account(address).map_or(0, |a| a.balance.into_inner())
}

#[test]
fn send_allowlist_producer_applier_wal() {
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
    let feature_on = mode == "on";
    if feature_on {
        refresh_feature_schedule_extensions(vec![(FEATURE.to_string(), H)]);
    }
    assert_eq!(feature_active(ConsensusFeature::SendAllowlist, H), feature_on);
    assert!(!feature_active(ConsensusFeature::SendAllowlist, H - 1));
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(feature_on));
}

async fn run(feature_on: bool) {
    let owner = key(0xA1);
    let owner_address = address_of(&owner);

    // Block classifier: below H a carried action invalidates the block on
    // every node; from H a well-signed one is accepted, a forged one is not.
    let enable = |nonce, tag| {
        action(&owner, SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: vec![A] }, nonce, tag)
    };
    assert!(!verify_block_with(enable(0, "probe-below"), H - 1).await, "below H: block invalid");
    assert_eq!(verify_block_with(enable(0, "probe-at"), H).await, feature_on, "at H: valid only when on");
    let mut forged = enable(0, "probe-forged");
    let mut signed = SignedSendAllowlistAction::parse(&forged).unwrap();
    signed.action = SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: vec![B] };
    forged.data = Some(serde_json::to_string(&signed).unwrap());
    forged.set_id();
    assert!(!verify_block_with(forged, H).await, "a forged action invalidates the block");

    let mut genesis = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    genesis.metadata.insert(GENESIS_CHAIN_ID_KEY.into(), "777".into());
    let mut genesis_transitions = extract_transitions(&State::new(), &genesis, 1, 0);
    genesis_transitions.push(StateTransition::ApplyReward {
        address: owner_address,
        amount: Balance::new(1_000_000),
        reason: "send-allowlist fixture funding".to_string(),
    });
    let mut applier = State::new();
    applier.apply_transitions_batch_at_height(1, genesis_transitions.clone()).unwrap();
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    {
        let mut c = chain.write().await;
        c.height = 1;
        c.state = applier.clone();
    }
    let db = std::env::temp_dir().join(format!(
        "send-allowlist-{}-{}.redb",
        std::process::id(),
        feature_on
    ));
    let _ = std::fs::remove_file(&db);
    let mut storage = Storage::open(&db, DEFAULT_CHECKPOINT_INTERVAL).unwrap();
    storage.persist_block(genesis, genesis_transitions, None).await.unwrap();
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let miner = Address([0x41; 20]).to_string();

    // (height, transactions). Transfer nonces follow the account nonce; the
    // refused transfer to B never consumes one.
    let plan: Vec<(u64, Vec<(&str, Transaction)>)> = vec![
        (2, vec![
            ("enable-below-h", enable(0, "enable-below-h")),
            ("pay-b-below-h", transfer(&owner, B, 100, 0, "pay-b-below-h")),
        ]),
        (3, vec![("enable-at-h", enable(0, "enable-at-h"))]),
        // One transfer per block: mempool ordering within a block is not by
        // nonce, so two transfers from one sender would race.
        (4, vec![("pay-a", transfer(&owner, A, 10, 1, "pay-a"))]),
        (5, vec![
            ("pay-b", transfer(&owner, B, 20, 2, "pay-b")),
            ("request-add-b", action(&owner, SendAllowlistAction::RequestAdd { address: B }, 1, "request-add-b")),
        ]),
        // Gate off the h5 payment consumed nonce 2; gate on it was refused.
        (6, vec![("pay-b-pending", transfer(&owner, B, 30, if feature_on { 2 } else { 3 }, "pay-b-pending"))]),
    ];

    for (height, txs) in plan {
        for (tag, tx) in txs {
            let admitted = mempool
                .write()
                .await
                .add_transaction_with_signing_keys_at_height(tx, None, Some(height - 1));
            if let Err(e) = admitted {
                eprintln!("admission refused {tag} at {height}: {e}");
                if tag.contains("enable") || tag.contains("request") {
                    assert!(
                        !feature_active(ConsensusFeature::SendAllowlist, height),
                        "{tag} refused while the feature is active: {e}"
                    );
                }
            }
        }
        let before = applier.clone();
        let block = pop
            .create_next_block(chain.clone(), mempool.clone(), &miner)
            .await
            .expect("the real producer builds the block");
        assert_eq!(block.height, height);
        let producer = chain.read().await.state.clone();
        let transitions = extract_transitions(&before, &block, block.height, block.timestamp.timestamp());
        applier.apply_transitions_batch_at_height(block.height, transitions.clone()).unwrap();
        storage.persist_block(block, transitions, None).await.unwrap();
        assert_eq!(applier.state_root, producer.state_root, "applier root at {height}");
        for who in [owner_address, A, B] {
            assert_eq!(balance(&applier, &who), balance(&producer, &who), "{who} at {height}");
        }
        assert_eq!(applier.send_allowlists, producer.send_allowlists, "allowlists at {height}");
        if height < H || !feature_on {
            assert!(producer.send_allowlists.is_empty(), "no allowlist state at {height}");
        }
        // Drop what the producer refused, so the next block starts clean.
        mempool.write().await.clear_all();
    }
    let producer = chain.read().await.state.clone();

    let rows = storage.load_wal_transitions_range(1, 6).unwrap();
    let mut wal = State::new();
    for height in 1..=6 {
        wal.apply_transitions_batch_at_height(
            height,
            rows.iter().filter(|(h, _)| *h == height).map(|(_, t)| t.clone()),
        )
        .unwrap();
    }
    assert_eq!(wal.state_root, producer.state_root, "WAL replay root");
    assert_eq!(wal.send_allowlists, producer.send_allowlists, "WAL replay allowlists");

    assert_eq!(balance(&producer, &A), 10, "A is on the list (or no list): paid");
    if feature_on {
        assert_eq!(balance(&producer, &B), 100, "only the pre-activation payment to B landed");
        let v = view(&producer.send_allowlists, &owner_address, 7);
        assert!(v.in_force);
        assert_eq!(v.allowed, vec![A.to_string()]);
        assert_eq!(v.pending_additions.len(), 1, "B is pending, visible to the wallet");
        assert_eq!(v.pending_additions[0].effective_at_height, 5 + MIN_ADD_DELAY_BLOCKS);
        assert_eq!(v.next_action_nonce, 2);
    } else {
        assert_eq!(balance(&producer, &B), 100 + 20 + 30, "gate off: every transfer applies as on main");
        assert!(producer.send_allowlists.is_empty());
    }

    // A peer must refuse a block whose governance metadata carries the
    // refused transfer: the applier fails, it does not skip.
    if feature_on {
        let mut forced = applier.clone();
        let refused = forced.apply_transitions_batch_at_height(
            7,
            vec![StateTransition::ProcessGovernanceTransaction {
                tx: transfer(&owner, B, 5, 2, "forced"),
                current_block: 7,
            }],
        );
        assert!(refused.is_err(), "a metadata-injected transfer to B must fail apply");
        let err = format!("{:?}", refused.unwrap_err());
        assert!(err.contains("SP_SEND_ALLOWLIST_DESTINATION_REFUSED"), "{err}");
    }

    drop(storage);
    std::fs::remove_file(db).unwrap();
}
