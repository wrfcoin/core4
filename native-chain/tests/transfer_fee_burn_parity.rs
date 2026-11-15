//! Burned transfer fee (`transfer_fee_burn`) with sponsored first sends
//! (`paymaster`), scheduled together at H = 3: producer / applier / WAL parity
//! on mined blocks that burn fees and carry a sponsorship.
//!
//! Two child processes (the feature schedule is process-wide):
//! - **gate off**: every transfer applies as on main: no fee, the sponsorship
//!   is ignored, nothing is burned.
//! - **gate on (H = 3)**: below H as on main; from H the sender of an
//!   unsponsored transfer pays the fee, a sponsored newcomer pays none (the
//!   sponsor's signed fee is burned), and an under-fee transfer is kept out of
//!   the block.
//!
//! In both, the real producer (`create_next_block`), the P2P applier
//! (`extract_transitions` → `apply_transitions_batch_at_height`) and WAL replay
//! (`load_wal_transitions_range`) reach the same `state_root`, balances and
//! burn journal at every height.

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

use tokio::sync::RwLock;
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::accounts::paymaster::SponsorAuthorization;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType, TransparentTransferPayload,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::consensus::transfer_fee::{
    TRANSFER_FEE_V1_BASE_UNITS, transfer_fee_at,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_CHAIN_ID_KEY, feature_active, refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const H: u64 = 3;
const FEE: u64 = TRANSFER_FEE_V1_BASE_UNITS;
const SPONSOR_FEE: u64 = 6_000;
const AMOUNT: u64 = 1_000;
const FUNDS: u128 = 1_000_000;
const CHILD_ENV: &str = "WRFCOIN_TRANSFER_FEE_BURN_PARITY_CHILD";
const TEST_NAME: &str = "transfer_fee_burn_producer_applier_wal_parity";

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn addr(k: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(k))
}

fn transfer(
    tag: &str,
    from: &PrivateKey,
    to: Address,
    nonce: u64,
    declared_fee: u64,
    sponsor: Option<(&PrivateKey, u64)>,
) -> Transaction {
    let from_addr = addr(from);
    let sponsorship = sponsor.map(|(sk, fee)| {
        let mut auth = SponsorAuthorization {
            sponsor: addr(sk).to_hex(),
            fee,
            expires_at_height: H + 20,
            balance_floor: 0,
            first_send_only: true,
            sponsor_public_key: String::new(),
            signature: String::new(),
        };
        auth.sign(sk, &from_addr.to_hex(), &to.to_hex(), AMOUNT, nonce);
        auth
    });
    let mut payload = TransparentTransferPayload {
        from: from_addr.to_hex(),
        to: to.to_hex(),
        amount: AMOUNT,
        transparent_opt_out: true,
        opt_out_audit_tag: None,
        auth_pubkey: None,
        auth_signature: None,
        authority: None,
        authority_signature: None,
        passkey_auth: None,
        session_key_auth: None,
        sponsorship,
    };
    payload.sign_authorization(from, nonce);
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("fee-burn-input-{tag}")),
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
    tx.sender_address = from_addr;
    tx.nonce = nonce;
    tx.fee = declared_fee;
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    tx.set_id();
    tx
}

fn bal(state: &State, who: &Address) -> u128 {
    state.get_account(who).map(|a| a.balance.0).unwrap_or(0)
}

#[test]
fn transfer_fee_burn_producer_applier_wal_parity() {
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
        refresh_feature_schedule_extensions(vec![
            (ConsensusFeature::Paymaster.name().to_string(), H),
            (ConsensusFeature::TransferFeeBurn.name().to_string(), H),
        ]);
    }
    assert_eq!(
        feature_active(ConsensusFeature::TransferFeeBurn, H),
        feature_on
    );
    assert_eq!(feature_active(ConsensusFeature::Paymaster, H), feature_on);
    assert_eq!(transfer_fee_at(H - 1), None);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(feature_on));
}

async fn run(feature_on: bool) {
    let (payer, newcomer, late, sponsor) = (key(0xB1), key(0xB2), key(0xB3), key(0xB4));
    let to = Address([0x5A; 20]);

    let mut genesis = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    genesis
        .metadata
        .insert(GENESIS_CHAIN_ID_KEY.into(), "777".into());
    let genesis_transitions = extract_transitions(&State::new(), &genesis, 1, 0);
    let mut applier = State::new();
    applier
        .apply_transitions_batch_at_height(1, genesis_transitions.clone())
        .unwrap();
    // Test fixture: funded accounts, adopted into the supply baseline so
    // INV-SUPPLY-1 audits every later burn. WAL replay starts from this state.
    applier.create_account(addr(&payer), Balance::new(FUNDS));
    applier.create_account(addr(&late), Balance::new(FUNDS));
    applier.create_account(addr(&sponsor), Balance::new(FUNDS));
    // The newcomer holds exactly the amount: only a sponsored send works.
    applier.create_account(addr(&newcomer), Balance::new(u128::from(AMOUNT)));
    applier.supply_baseline_initialized = false;
    applier.initialize_supply_baseline().unwrap();
    applier.recalculate_state_root();
    let funded = applier.clone();

    let chain = Arc::new(RwLock::new(Blockchain::default()));
    {
        let mut c = chain.write().await;
        c.height = 1;
        c.state = applier.clone();
    }
    let db = std::env::temp_dir().join(format!(
        "transfer-fee-burn-{}-{}.redb",
        std::process::id(),
        feature_on
    ));
    let _ = std::fs::remove_file(&db);
    let mut storage = Storage::open(&db, DEFAULT_CHECKPOINT_INTERVAL).unwrap();
    storage
        .persist_block(genesis, genesis_transitions, None)
        .await
        .unwrap();
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let miner = Address([0x41; 20]).to_string();

    let plan: Vec<(u64, Vec<(&str, Transaction)>)> = vec![
        // Below H: no fee in either mode, whatever is declared.
        (
            2,
            vec![("below-h", transfer("below-h", &payer, to, 0, 0, None))],
        ),
        (
            3,
            vec![
                (
                    "payer-at-h",
                    transfer("payer-at-h", &payer, to, 1, FEE, None),
                ),
                (
                    "newcomer-sponsored",
                    transfer(
                        "newcomer-sponsored",
                        &newcomer,
                        to,
                        0,
                        FEE,
                        Some((&sponsor, SPONSOR_FEE)),
                    ),
                ),
                // Declares no fee: kept out of the block once the fee is in force.
                (
                    "late-under-fee",
                    transfer("late-under-fee", &late, to, 0, 0, None),
                ),
            ],
        ),
        (
            4,
            vec![(
                "payer-after-h",
                transfer("payer-after-h", &payer, to, 2, 2 * FEE, None),
            )],
        ),
    ];

    for (height, txs) in plan {
        for (tag, tx) in txs {
            mempool
                .write()
                .await
                .add_transaction_with_signing_keys_at_height(tx, None, Some(height - 1))
                .unwrap_or_else(|e| {
                    // Admission refusing what consensus refuses is expected.
                    eprintln!("admission refused {tag} at {height}: {e}");
                });
        }
        let before = applier.clone();
        let block = pop
            .create_next_block(chain.clone(), mempool.clone(), &miner)
            .await
            .expect("the real producer builds the block");
        assert_eq!(block.height, height);
        let producer = chain.read().await.state.clone();
        let transitions =
            extract_transitions(&before, &block, block.height, block.timestamp.timestamp());
        applier
            .apply_transitions_batch_at_height(block.height, transitions.clone())
            .unwrap();
        storage
            .persist_block(block, transitions, None)
            .await
            .unwrap();
        assert_eq!(
            applier.state_root, producer.state_root,
            "applier root at {height}"
        );
        assert_eq!(
            applier.supply_authorized_burning, producer.supply_authorized_burning,
            "burn journal at {height}"
        );
        for who in [&payer, &newcomer, &late, &sponsor] {
            assert_eq!(
                bal(&applier, &addr(who)),
                bal(&producer, &addr(who)),
                "balance at {height}"
            );
        }
    }
    let mut producer = chain.read().await.state.clone();

    let rows = storage.load_wal_transitions_range(2, 4).unwrap();
    let mut wal = funded;
    for height in 2..=4 {
        wal.apply_transitions_batch_at_height(
            height,
            rows.iter()
                .filter(|(h, _)| *h == height)
                .map(|(_, t)| t.clone()),
        )
        .unwrap();
    }
    assert_eq!(wal.state_root, producer.state_root, "WAL replay root");
    assert_eq!(
        wal.supply_authorized_burning,
        producer.supply_authorized_burning
    );
    producer
        .check_supply_conservation()
        .expect("INV-SUPPLY-1 after fee burns");

    let amount = u128::from(AMOUNT);
    let (fee, sponsor_fee) = (u128::from(FEE), u128::from(SPONSOR_FEE));
    if feature_on {
        // payer: three sends, fee charged at H and H+1 only (protocol fee, not
        // the declared 2*FEE).
        assert_eq!(bal(&producer, &addr(&payer)), FUNDS - 3 * amount - 2 * fee);
        assert_eq!(
            bal(&producer, &addr(&newcomer)),
            0,
            "sponsored newcomer paid no fee"
        );
        assert_eq!(bal(&producer, &addr(&sponsor)), FUNDS - sponsor_fee);
        assert_eq!(
            bal(&producer, &addr(&late)),
            FUNDS,
            "under-fee transfer never mined"
        );
        assert_eq!(bal(&producer, &to), 4 * amount);
        assert_eq!(producer.supply_authorized_burning, 2 * fee + sponsor_fee);
        assert!(
            !producer.paymasters.is_empty(),
            "the sponsorship is recorded"
        );
    } else {
        assert_eq!(bal(&producer, &addr(&payer)), FUNDS - 3 * amount);
        assert_eq!(bal(&producer, &addr(&newcomer)), 0);
        assert_eq!(
            bal(&producer, &addr(&sponsor)),
            FUNDS,
            "sponsorship ignored"
        );
        assert_eq!(bal(&producer, &addr(&late)), FUNDS - amount);
        assert_eq!(bal(&producer, &to), 5 * amount);
        assert_eq!(producer.supply_authorized_burning, 0);
        assert!(producer.paymasters.is_empty());
    }

    drop(storage);
    std::fs::remove_file(db).unwrap();
}
