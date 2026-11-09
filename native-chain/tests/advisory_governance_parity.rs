//! Advisory governance (`advisory_governance`): activation boundary and
//! producer / applier / WAL-replay parity through the real block producer,
//! across a whole proposal lifecycle (propose, votes, finalize at the
//! deadline, deposit returned).
//!
//! Two child processes (the feature schedule is process-wide):
//! - **gate off**: an `AdvisoryGovernance` action is refused by admission and a
//!   block carrying one is invalid at every height; `State::advisory_proposals`
//!   stays empty, so the root has no advisory section.
//! - **gate on (H = 3)**: below H the same refusals hold. At H Tony proposes
//!   "Welcome to governance"; three wallets vote; at H + 1,120 the proposal is
//!   finalized as passed and the deposit returned. Producer
//!   (`create_next_block`), applier (`extract_transitions` →
//!   `apply_transitions_batch_at_height`) and WAL replay agree on `state_root`,
//!   on every balance and on the proposal map at every height.

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
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{PopLivenessPolicy, State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, GENESIS_CHAIN_ID_KEY, feature_active,
    refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::advisory::{
    ADVISORY_DEPOSIT, ADVISORY_VOTING_PERIOD_BLOCKS, AdvisoryAction, AdvisoryStatus,
    SignedAdvisoryAction, proposal_id,
};

const H: u64 = 3;
const FEATURE: &str = "advisory_governance";
const CHILD_ENV: &str = "WRFCOIN_ADVISORY_GOVERNANCE_CHILD";
const TEST_NAME: &str = "advisory_governance_producer_applier_wal";
const FUNDING: u128 = 5 * 100_000_000;

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn address_of(k: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(k))
}

fn advisory(k: &PrivateKey, action: AdvisoryAction, nonce: u64, tag: &str) -> Transaction {
    let identity = ChainIdentity::active();
    let signed = SignedAdvisoryAction::sign(&identity.chain_id, &identity.network_id, k, action, nonce);
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("advisory-{tag}")),
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
    tx.tx_type = TransactionType::AdvisoryGovernance;
    tx.sender_address = signed.account;
    tx.nonce = nonce;
    tx.data = Some(serde_json::to_string(&signed).unwrap());
    tx.set_id();
    tx
}

fn welcome(k: &PrivateKey, tag: &str) -> Transaction {
    advisory(
        k,
        AdvisoryAction::Propose {
            title: "Welcome to governance".into(),
            summary: "The first advisory proposal on chain 888. It changes nothing.".into(),
        },
        0,
        tag,
    )
}

/// The #1762 recipe: a hand-built block carrying `tx`, accepted by every gate
/// except the one under test.
async fn verify_block_with(tx: Transaction, height: u64) -> bool {
    let txs = vec![
        Transaction::new_coinbase("advisory-miner", adjusted_block_reward(height), height),
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

fn balance(state: &State, address: &Address) -> (u128, u64) {
    state.get_account(address).map_or((0, 0), |a| (a.balance.into_inner(), a.nonce))
}

#[test]
fn advisory_governance_producer_applier_wal() {
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
    assert_eq!(feature_active(ConsensusFeature::AdvisoryGovernance, H), feature_on);
    assert!(!feature_active(ConsensusFeature::AdvisoryGovernance, H - 1));
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(feature_on));
}

async fn run(feature_on: bool) {
    let tony = key(0xA1);
    let me = address_of(&tony);
    let voters: Vec<PrivateKey> = [0xB1, 0xB2, 0xB3].into_iter().map(key).collect();
    let id = proposal_id(&me, 0);

    // Block classifier: below H a carried action invalidates the block on
    // every node; from H a well-signed one is accepted, a forged one is not.
    assert!(!verify_block_with(welcome(&tony, "probe-below"), H - 1).await, "below H: block invalid");
    assert_eq!(verify_block_with(welcome(&tony, "probe-at"), H).await, feature_on, "at H: valid only when on");
    let mut forged = welcome(&tony, "probe-forged");
    let mut signed = SignedAdvisoryAction::parse(&forged).unwrap();
    signed.action = AdvisoryAction::Propose { title: "Changed".into(), summary: "Changed".into() };
    forged.data = Some(serde_json::to_string(&signed).unwrap());
    forged.set_id();
    assert!(!verify_block_with(forged, H).await, "a forged action invalidates the block");

    let mut genesis = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    genesis.metadata.insert(GENESIS_CHAIN_ID_KEY.into(), "777".into());
    let mut genesis_transitions = extract_transitions(&State::new(), &genesis, 1, 0);
    for who in std::iter::once(me).chain(voters.iter().map(address_of)) {
        genesis_transitions.push(StateTransition::ApplyReward {
            address: who,
            amount: Balance::new(FUNDING),
            reason: "advisory fixture funding".to_string(),
        });
    }
    let mut applier = State::new();
    applier.apply_transitions_batch_at_height(1, genesis_transitions.clone()).unwrap();
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    {
        let mut c = chain.write().await;
        c.height = 1;
        c.state = applier.clone();
    }
    let db = std::env::temp_dir().join(format!(
        "advisory-governance-{}-{}.redb",
        std::process::id(),
        feature_on
    ));
    let _ = std::fs::remove_file(&db);
    // No checkpoint inside the run, so the WAL keeps every block for the
    // replay below (a checkpoint prunes the rows under it).
    let checkpoint_interval = (ADVISORY_VOTING_PERIOD_BLOCKS + 100).max(DEFAULT_CHECKPOINT_INTERVAL);
    let mut storage = Storage::open(&db, checkpoint_interval).unwrap();
    storage.persist_block(genesis, genesis_transitions, None).await.unwrap();
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let miner = Address([0x41; 20]).to_string();

    let deadline = H + ADVISORY_VOTING_PERIOD_BLOCKS;
    let last = deadline + 1;
    let tony_funded = balance(&applier, &me).0;
    for height in 2..=last {
        let txs: Vec<(&str, Transaction)> = match height {
            2 => vec![("propose-below-h", welcome(&tony, "propose-below-h"))],
            3 => vec![("propose-at-h", welcome(&tony, "propose-at-h"))],
            4 => vec![
                ("vote-1", advisory(&voters[0], AdvisoryAction::Vote { proposal_id: id.clone(), approve: true }, 0, "vote-1")),
                ("vote-2", advisory(&voters[1], AdvisoryAction::Vote { proposal_id: id.clone(), approve: true }, 0, "vote-2")),
            ],
            5 => vec![("vote-3", advisory(&voters[2], AdvisoryAction::Vote { proposal_id: id.clone(), approve: false }, 0, "vote-3"))],
            _ => Vec::new(),
        };
        for (tag, tx) in txs {
            let admitted = mempool
                .write()
                .await
                .add_transaction_with_signing_keys_at_height(tx, None, Some(height - 1));
            match admitted {
                Ok(_) => assert!(
                    feature_active(ConsensusFeature::AdvisoryGovernance, height),
                    "{tag} admitted while the feature is inactive"
                ),
                Err(e) => assert!(
                    !feature_active(ConsensusFeature::AdvisoryGovernance, height),
                    "{tag} refused while the feature is active: {e}"
                ),
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
        assert_eq!(applier.advisory_proposals, producer.advisory_proposals, "proposals at {height}");
        for who in std::iter::once(me).chain(voters.iter().map(address_of)) {
            assert_eq!(balance(&applier, &who), balance(&producer, &who), "{who} at {height}");
        }
        if height < H || !feature_on {
            assert!(producer.advisory_proposals.is_empty(), "no advisory state at {height}");
        }
        if feature_on && height == H {
            assert_eq!(balance(&producer, &me).0, tony_funded - u128::from(ADVISORY_DEPOSIT), "deposit debited at H");
        }
        mempool.write().await.clear_all();
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
    assert_eq!(wal.advisory_proposals, producer.advisory_proposals, "WAL replay proposals");

    if feature_on {
        let p = &producer.advisory_proposals[&id];
        assert_eq!(p.status, AdvisoryStatus::Passed);
        assert_eq!(p.finalized_at_block, Some(deadline));
        assert_eq!((p.yes_votes, p.no_votes), (2, 1));
        assert_eq!(p.deposit, Balance::new(0));
        assert_eq!(balance(&producer, &me), (tony_funded, 1), "deposit returned; one nonce used");
        assert!(producer.governance.proposals.is_empty(), "the legacy map is untouched");
    } else {
        assert!(producer.advisory_proposals.is_empty());
        assert_eq!(balance(&producer, &me), (tony_funded, 0));
    }

    // A peer must refuse a block whose governance metadata injects an
    // unsigned-for-this-sender action or a premature finalize.
    if feature_on {
        let mut forced = applier.clone();
        let mut stolen = welcome(&voters[0], "forced");
        stolen.sender_address = me;
        stolen.set_id();
        assert!(forced
            .apply_transitions_batch_at_height(last + 1, vec![StateTransition::ProcessGovernanceTransaction { tx: stolen, current_block: last + 1 }])
            .is_err());
        assert!(forced
            .apply_transitions_batch_at_height(last + 1, vec![StateTransition::FinalizeAdvisoryProposal { proposal_id: id.clone(), current_block: last + 1 }])
            .is_err(), "a finalized proposal cannot be finalized again");
    }

    drop(storage);
    std::fs::remove_file(db).unwrap();
}
