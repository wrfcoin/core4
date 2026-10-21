//! #3361 F3: admission of the faucet sink binding on the chain its
//! `LEGACY_FAUCET_BINDINGS` row names.
//!
//! This process is a node whose durable block 1 IS the row's block-1 hash
//! (the live 777/testnet instance). There, and only there, the
//! `faucet_sink_binding` feature is schedulable: the mempool admits an
//! extension naming it and the producer includes it. A `faucet_sink_binding*`
//! name with no row (`_r2` in this binary) stays refused, and — T8 — a block
//! that carries it anyway is still valid: names are admission policy, never
//! consensus.
//!
//! The mismatched-chain half of the matrix (the same names refused on a
//! chain whose block 1 is NOT the row's) runs in
//! `schedule_extension_tx_3277`, whose process is such a chain; the CLI half
//! runs in the WrfcoinCLI unit tests.
//!
//! Own process: the base schedule (the V1 gate) and the extension authority
//! are process-wide, first-install-wins values; every test installs identical
//! ones.
//!
//! Run: `cargo test -p wrfcoin-shared-protocol --test faucet_sink_binding_admission_3361`

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::{Mutex, MutexGuard, RwLock};

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{Transaction, TransactionType};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, LEGACY_FAUCET_BINDINGS,
    SigningProtocolVersion, install_activation_table, install_feature_schedule,
    resolve_activation_schedule, resolve_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, GovernanceSignature, MIN_RUNUP_BLOCKS, SCHEDULE_EXTENSION_FEATURE,
    ScheduleEntry, SignedFeatureScheduleExtension,
    install_schedule_extension_authority_for_tests,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

const CHAIN: &str = "777";
const NETWORK: &str = "testnet";
/// This node's block-1 hash: the one the committed 777 row names.
const LIVE_777_BLOCK1: &str = "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8"; // pragma: allowlist secret -- public genesis hash
/// The base schedule enables the extension transaction from this height (V1).
const H_ENABLE: u64 = 2;
const CARRY: u64 = 10;
const H_F: u64 = CARRY + MIN_RUNUP_BLOCKS;

static LOCK: Mutex<()> = Mutex::const_new(());

fn governance() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(3361))
}

async fn gate() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().await;
    let raw = format!("0:v1,{H_ENABLE}:{SCHEDULE_EXTENSION_FEATURE}");
    let pairs = vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
        .expect("activation table");
    let features = resolve_feature_schedule(pairs, &table).expect("feature schedule");
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    install_schedule_extension_authority_for_tests(CHAIN, NETWORK, LIVE_777_BLOCK1, vec![
            PublicKey::from_private_key(&governance())
                .to_hex()
                .to_ascii_lowercase(),
        ]);
    guard
}

/// E0f: one faucet-binding entry, alone in its extension (#3361 Q2).
fn e0f(feature: &str, sequence: u64, activation: u64) -> FeatureScheduleExtension {
    FeatureScheduleExtension {
        chain_id: CHAIN.into(),
        network_id: NETWORK.into(),
        genesis_hash: LIVE_777_BLOCK1.into(),
        sequence,
        entries: vec![ScheduleEntry::new(feature, activation)],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "f3-test-release".into(),
    }
}

/// The governance signature over the extension's preimage, produced from
/// the raw key WITHOUT any library signer: the library signers refuse a
/// faucet binding without a proof of possession (the CLI's job, tested
/// there), while a node judges only the governance signature.
fn signed(extension: FeatureScheduleExtension) -> SignedFeatureScheduleExtension {
    let key = governance();
    let signature = GovernanceSignature {
        signer_pubkey: PublicKey::from_private_key(&key).to_hex().to_ascii_lowercase(),
        signature: key.sign(&extension.preimage()).to_hex(),
    };
    SignedFeatureScheduleExtension {
        extension,
        signatures: vec![signature],
    }
}

fn tx(signed: &SignedFeatureScheduleExtension) -> Transaction {
    Transaction::new_feature_schedule_extension(signed)
}

fn verdict(parent: &State, height: u64, txs: Vec<Transaction>) -> Result<(), String> {
    let mut chain = Blockchain::default();
    chain.state = parent.clone();
    ProofOfPublishing::verify_launch_state_invariants(
        &chain,
        &Block::new(3, Hash::default(), txs, 1, height),
    )
    .map_err(|e| e.to_string())
}

#[tokio::test]
async fn the_committed_row_names_this_nodes_block_1() {
    let _g = gate().await;
    assert!(
        LEGACY_FAUCET_BINDINGS.iter().any(|row| row.feature == ConsensusFeature::FaucetSinkBinding
            && row.genesis_hash == LIVE_777_BLOCK1
            && row.genesis_chain_id == CHAIN),
        "this process stands in for the node the 777 row binds"
    );
}

/// RED on main (`07c9ff78c`): the blanket S4 hold refused the name on every
/// chain, so the extension that the owner ruled (E0f) could never enter a
/// mempool.
#[tokio::test]
async fn the_mempool_admits_the_faucet_binding_on_the_rows_own_chain() {
    let _g = gate().await;
    let mut mempool = Mempool::new(&Config::default());
    let extension = signed(e0f(ConsensusFeature::FaucetSinkBinding.name(), 0, H_F));
    mempool
        .add_transaction(tx(&extension))
        .expect("the bound chain admits its own faucet binding");
    assert_eq!(mempool.size(), 1);
}

/// A `faucet_sink_binding*` name this binary has no row for stays refused on
/// every chain, the row's own included — and a block carrying it anyway is
/// valid and applies (T8: names are policy, never consensus).
#[tokio::test]
async fn a_binding_name_without_a_row_is_refused_yet_valid_in_a_block() {
    let _g = gate().await;
    let mut mempool = Mempool::new(&Config::default());
    let genesis = State::new();
    for feature in ["faucet_sink_binding_r2", "faucet_sink_binding_r9"] {
        let extension = signed(e0f(feature, 0, H_F));
        let err = mempool
            .add_transaction(tx(&extension))
            .expect_err("no row, no admission")
            .to_string();
        assert!(err.contains("admission policy"), "{feature}: {err}");
        assert!(
            verdict(&genesis, CARRY, vec![tx(&extension)]).is_ok(),
            "{feature}: a block carrying it must be valid"
        );
        let mut applied = genesis.clone();
        applied
            .apply_transitions_batch_at_height(
                CARRY,
                [StateTransition::ApplyFeatureScheduleExtension {
                    extension: extension.clone(),
                    block_height: CARRY,
                }],
            )
            .expect("applies");
        assert_eq!(applied.feature_schedule_extension_entries(), vec![(feature.to_string(), H_F)]);
    }
    assert_eq!(mempool.size(), 0);
}

/// RED on main: the producer dropped (and evicted) the admissible E0f.
#[tokio::test]
async fn the_producer_includes_the_faucet_binding_on_the_rows_own_chain() {
    let _g = gate().await;
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    pop.declare_no_genesis_oracles();
    let miner = hex::encode([0x42u8; 20]);

    let block_1 = pop
        .create_next_block(chain.clone(), mempool.clone(), &miner)
        .await
        .expect("block 1");
    assert_eq!(block_1.height, 1);

    let activation = H_ENABLE + MIN_RUNUP_BLOCKS;
    let extension = signed(e0f(ConsensusFeature::FaucetSinkBinding.name(), 0, activation));
    mempool
        .write()
        .await
        .add_transaction(tx(&extension))
        .expect("admitted");
    let block_2 = pop
        .create_next_block(chain.clone(), mempool.clone(), &miner)
        .await
        .expect("block 2");
    assert_eq!(block_2.height, H_ENABLE);
    assert!(
        block_2
            .transactions
            .iter()
            .any(|t| t.tx_type == TransactionType::FeatureScheduleExtension),
        "the producer includes the bound chain's faucet binding"
    );
    assert_eq!(
        chain.read().await.state.feature_schedule_extension_entries(),
        vec![("faucet_sink_binding".to_string(), activation)]
    );
    assert_eq!(mempool.read().await.size(), 0);
}
