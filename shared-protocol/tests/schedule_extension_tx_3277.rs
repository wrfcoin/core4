//! #3277 S4: the `FeatureScheduleExtension` TRANSACTION, end to end on the
//! shared-protocol side.
//!
//! - **T4, validation matrix V1–V9, through the block path.** Every rule is
//!   exercised as a transaction in a block, judged by
//!   `ProofOfPublishing::verify_launch_state_invariants` — the stateful half
//!   of block validation a syncing peer runs (`PopConsensusAdapter::
//!   validate_block`). Each rule has an accept case and a reject case; a
//!   field tampered after signing, a replayed transaction and a cross-domain
//!   signature (an `UpgradeNotice` signature presented as an extension's) are
//!   rejected.
//! - **`verify_block`** rejects an unparsable payload from the bytes alone and
//!   accepts a parsable one (its rules need state; see above).
//! - **T8, admission policy.** The mempool refuses an extension naming a
//!   feature this binary does not implement or marks not schedulable, while
//!   the SAME transaction arriving in a block is valid and applies — names are
//!   policy, never consensus.
//! - **Producer inclusion.** `create_next_block` includes an admitted
//!   extension once the gate is active, and prunes one staged before it.
//!
//! Own process: the base schedule (the V1 gate, standing in for S5's
//! bootstrap) and the extension authority (standing in for S5's installer)
//! are process-wide first-install-wins values; every test installs identical
//! ones.
//!
//! Run: `cargo test -p wrfcoin-shared-protocol --test schedule_extension_tx_3277`

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use rustc_hash::FxHashSet;
use tokio::sync::{Mutex, MutexGuard, RwLock};

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{Transaction, TransactionType};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, GovernanceSignature, MAX_ENTRIES_PER_EXTENSION, MAX_HORIZON_BLOCKS,
    MIN_RUNUP_BLOCKS, SCHEDULE_EXTENSION_FEATURE, ScheduleEntry, SignedFeatureScheduleExtension, VersionStep, install_schedule_extension_authority_for_tests,
    sign_feature_schedule_extension_with_key,
};
use wrfcoin_shared_protocol::governance::upgrade_notice::{
    UpgradeNotice, sign_upgrade_notice_with_key,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

const CHAIN: &str = "777";
const NETWORK: &str = "testnet";
const GENESIS: &str = "abababababababababababababababababababababababababababababababab";
const OTHER_GENESIS: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
/// The base schedule enables the extension transaction from this height (V1).
const H_ENABLE: u64 = 2;
/// The carrying height used by the matrix.
const CARRY: u64 = 10;
/// The earliest activation V7 allows for an extension carried at `CARRY`.
const H0: u64 = CARRY + MIN_RUNUP_BLOCKS;

static LOCK: Mutex<()> = Mutex::const_new(());

fn governance() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(3277))
}

fn outsider() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(0xbad))
}

/// The V1 gate and the authority, as a node installs them before applying
/// anything. First install wins; every test installs identical values.
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
    install_schedule_extension_authority_for_tests(CHAIN, NETWORK, GENESIS, vec![
            PublicKey::from_private_key(&governance())
                .to_hex()
                .to_ascii_lowercase(),
        ]);
    guard
}

/// The E0 shape: two known, schedulable features at the earliest allowed
/// height.
fn e0(sequence: u64) -> FeatureScheduleExtension {
    FeatureScheduleExtension {
        chain_id: CHAIN.into(),
        network_id: NETWORK.into(),
        genesis_hash: GENESIS.into(),
        sequence,
        entries: vec![
            ScheduleEntry::new(ConsensusFeature::ReferenceSeaLevelPressure.name(), H0),
            ScheduleEntry::new(ConsensusFeature::RewardGrossOutlierGuard.name(), H0),
        ],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "s4-test-release".into(),
    }
}

fn with_entries(entries: &[(&str, u64)]) -> FeatureScheduleExtension {
    FeatureScheduleExtension {
        entries: entries
            .iter()
            .map(|(name, height)| ScheduleEntry::new(*name, *height))
            .collect(),
        ..e0(0)
    }
}

fn signed_by(extension: FeatureScheduleExtension, keys: &[PrivateKey]) -> SignedFeatureScheduleExtension {
    let signatures = keys
        .iter()
        // Raw preimage signatures, not the library signer: T8 must also
        // build HELD names (#3361 F3), which every library signer refuses,
        // to show admission refuses them independently.
        .map(|key| GovernanceSignature {
            signer_pubkey: PublicKey::from_private_key(key).to_hex().to_ascii_lowercase(),
            signature: key.sign(&extension.preimage()).to_hex(),
        })
        .collect();
    SignedFeatureScheduleExtension {
        extension,
        signatures,
    }
}

fn signed(extension: FeatureScheduleExtension) -> SignedFeatureScheduleExtension {
    signed_by(extension, &[governance()])
}

fn tx(signed: &SignedFeatureScheduleExtension) -> Transaction {
    Transaction::new_feature_schedule_extension(signed)
}

fn block(height: u64, txs: Vec<Transaction>) -> Block {
    Block::new(3, Hash::default(), txs, 1, height)
}

fn chain_with(state: State) -> Blockchain {
    let mut chain = Blockchain::default();
    chain.state = state;
    chain
}

/// The stateful block verdict a syncing peer reaches for `txs` at `height`
/// over `parent`.
fn verdict(parent: &State, height: u64, txs: Vec<Transaction>) -> Result<(), String> {
    ProofOfPublishing::verify_launch_state_invariants(&chain_with(parent.clone()), &block(height, txs))
        .map_err(|e| e.to_string())
}

fn accepts(parent: &State, height: u64, signed: &SignedFeatureScheduleExtension) {
    if let Err(e) = verdict(parent, height, vec![tx(signed)]) {
        panic!("expected the block at {height} to be valid, got: {e}");
    }
}

fn rejects(parent: &State, height: u64, signed: &SignedFeatureScheduleExtension, needle: &str) {
    match verdict(parent, height, vec![tx(signed)]) {
        Ok(()) => panic!("expected the block at {height} to be invalid ({needle})"),
        Err(e) => assert!(e.contains(needle), "expected {needle:?} in: {e}"),
    }
}

/// A parent state that has applied `signed` at `height` through the same
/// transition the extractors emit.
fn after(parent: &State, height: u64, signed: &SignedFeatureScheduleExtension) -> State {
    let mut state = parent.clone();
    state
        .apply_transitions_batch_at_height(
            height,
            [StateTransition::ApplyFeatureScheduleExtension {
                extension: signed.clone(),
                block_height: height,
            }],
        )
        .expect("the fixture extension applies");
    state
}

// ---------------------------------------------------------------------------
// The transaction itself
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_transaction_carries_the_signed_extension_and_parses_back() {
    let _g = gate().await;
    let signed = signed(e0(0));
    let tx = tx(&signed);
    assert_eq!(tx.tx_type, TransactionType::FeatureScheduleExtension);
    assert_eq!(tx.fee, 0);
    assert!(tx.outputs.iter().all(|o| o.amount == 0), "the tx carries no funds");
    assert!(tx.validate_structure(), "admissible shape");
    assert_eq!(tx.parse_feature_schedule_extension(), Some(signed.clone()));

    // The txid is bound to every signed byte and to the signatures.
    let mut other_sequence = signed.clone();
    other_sequence.extension.sequence = 1;
    assert_ne!(Transaction::new_feature_schedule_extension(&other_sequence).id, tx.id);
    let mut cosigned = signed.clone();
    cosigned
        .signatures
        .push(sign_feature_schedule_extension_with_key(&signed.extension, &outsider())
            .expect("governance signs a non-held extension"));
    assert_ne!(Transaction::new_feature_schedule_extension(&cosigned).id, tx.id);

    // The parse is type-bound and payload-bound.
    let mut wrong_type = tx.clone();
    wrong_type.tx_type = TransactionType::OracleRegistration;
    assert_eq!(wrong_type.parse_feature_schedule_extension(), None);
    let mut garbage = tx.clone();
    garbage.data = Some("{\"not\":\"an extension\"}".into());
    assert_eq!(garbage.parse_feature_schedule_extension(), None);
    let mut missing = tx;
    missing.data = None;
    assert_eq!(missing.parse_feature_schedule_extension(), None);
}

/// The producer extractor emits exactly the transition the applier's does
/// (the applier half is pinned in `native-chain`), and refuses to silently
/// skip an unparsable payload.
#[tokio::test]
async fn the_producer_block_replay_applies_the_extension_and_refuses_an_unparsable_one() {
    let _g = gate().await;
    let signed = signed(e0(0));
    let mut replay = State::new();
    let block = block(CARRY, vec![tx(&signed)]);
    ProofOfPublishing::verify_launch_state_invariants(&chain_with(replay.clone()), &block)
        .expect("valid block");
    replay
        .apply_transitions_batch_at_height(
            CARRY,
            [StateTransition::ApplyFeatureScheduleExtension {
                extension: signed.clone(),
                block_height: CARRY,
            }],
        )
        .unwrap();
    assert_eq!(replay.feature_schedule_extensions.len(), 1);
    assert_eq!(replay.feature_schedule_extensions[0].carried_at_height, CARRY);

    let mut unparsable = tx(&signed);
    unparsable.data = Some("not json".into());
    unparsable.set_id();
    let err = verdict(&State::new(), CARRY, vec![unparsable]).expect_err("must be invalid");
    assert!(err.contains("FeatureScheduleExtension"), "{err}");
}

// ---------------------------------------------------------------------------
// T4: V1–V9 through the block path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn t4_v1_the_transaction_type_is_gated_by_the_base_schedule() {
    let _g = gate().await;
    let genesis = State::new();
    let early = FeatureScheduleExtension {
        entries: vec![ScheduleEntry::new(
            ConsensusFeature::RewardGrossOutlierGuard.name(),
            (H_ENABLE - 1) + MIN_RUNUP_BLOCKS,
        )],
        ..e0(0)
    };
    rejects(&genesis, H_ENABLE - 1, &signed(early), "Invalid state transition");
    let on_time = FeatureScheduleExtension {
        entries: vec![ScheduleEntry::new(
            ConsensusFeature::RewardGrossOutlierGuard.name(),
            H_ENABLE + MIN_RUNUP_BLOCKS,
        )],
        ..e0(0)
    };
    accepts(&genesis, H_ENABLE, &signed(on_time));
}

#[tokio::test]
async fn t4_v2_chain_network_and_genesis_binding() {
    let _g = gate().await;
    let genesis = State::new();
    accepts(&genesis, CARRY, &signed(e0(0)));
    rejects(
        &genesis,
        CARRY,
        &signed(FeatureScheduleExtension { chain_id: "778".into(), ..e0(0) }),
        "is for chain",
    );
    rejects(
        &genesis,
        CARRY,
        &signed(FeatureScheduleExtension { network_id: "mainnet".into(), ..e0(0) }),
        "is for chain",
    );
    rejects(
        &genesis,
        CARRY,
        &signed(FeatureScheduleExtension { genesis_hash: OTHER_GENESIS.into(), ..e0(0) }),
        "bound to genesis",
    );
}

#[tokio::test]
async fn t4_v3_sequence_and_replay() {
    let _g = gate().await;
    let genesis = State::new();
    rejects(&genesis, CARRY, &signed(e0(1)), "is not the next slot (0)");
    let first = signed(e0(0));
    accepts(&genesis, CARRY, &first);
    let applied = after(&genesis, CARRY, &first);

    // The identical transaction replayed in a later block.
    rejects(&applied, CARRY + 1, &first, "is not the next slot (1)");

    // The next slot, appended after the first.
    let second = signed(FeatureScheduleExtension {
        sequence: 1,
        entries: vec![ScheduleEntry::new(
            ConsensusFeature::VerifyUnitPricing.name(),
            H0 + 1,
        )],
        ..e0(1)
    });
    accepts(&applied, CARRY + 1, &second);

    // Both in one block apply in order; reversed, the block is invalid.
    let both = verdict(&genesis, CARRY, vec![tx(&first), tx(&second)]);
    assert!(both.is_ok(), "sequence 0 then 1 in one block: {both:?}");
    let reversed = verdict(&genesis, CARRY, vec![tx(&second), tx(&first)]);
    assert!(reversed.is_err(), "sequence 1 before 0 must invalidate the block");
}

#[tokio::test]
async fn t4_v4_entry_count_and_version_steps() {
    let _g = gate().await;
    let genesis = State::new();
    rejects(&genesis, CARRY, &signed(with_entries(&[])), "neither a feature entry");
    let names: Vec<String> = (0..=MAX_ENTRIES_PER_EXTENSION).map(|i| format!("feature_{i}")).collect();
    let too_many: Vec<(&str, u64)> = names.iter().map(|n| (n.as_str(), H0)).collect();
    rejects(&genesis, CARRY, &signed(with_entries(&too_many)), "the limit is 8");
    let eight: Vec<(&str, u64)> = too_many[..MAX_ENTRIES_PER_EXTENSION].to_vec();
    accepts(&genesis, CARRY, &signed(with_entries(&eight)));
    let step = FeatureScheduleExtension {
        entries: Vec::new(),
        version_steps: vec![VersionStep {
            activation_height: CARRY + 5_000,
            version: 2,
        }],
        ..e0(0)
    };
    rejects(&genesis, CARRY, &signed(step), "S11");
}

#[tokio::test]
async fn t4_v5_names() {
    let _g = gate().await;
    let genesis = State::new();
    rejects(&genesis, CARRY, &signed(with_entries(&[("Bad-Name", H0)])), "is not [a-z]");
    rejects(&genesis, CARRY, &signed(with_entries(&[("v3", H0)])), "signing-version token");
    rejects(&genesis, CARRY, &signed(with_entries(&[("signing_v2", H0)])), "version table");
    rejects(
        &genesis,
        CARRY,
        &signed(with_entries(&[("verify_unit_pricing", H0), ("verify_unit_pricing", H0 + 1)])),
        "appears twice",
    );
    accepts(&genesis, CARRY, &signed(with_entries(&[("a_well_formed_name_2", H0)])));
}

#[tokio::test]
async fn t4_v6_no_name_is_scheduled_twice() {
    let _g = gate().await;
    let genesis = State::new();
    // In the base schedule.
    rejects(
        &genesis,
        CARRY,
        &signed(with_entries(&[(SCHEDULE_EXTENSION_FEATURE, H0)])),
        "already scheduled",
    );
    // In a prior extension.
    let applied = after(&genesis, CARRY, &signed(e0(0)));
    let again = FeatureScheduleExtension {
        sequence: 1,
        entries: vec![ScheduleEntry::new(
            ConsensusFeature::RewardGrossOutlierGuard.name(),
            H0 + 10,
        )],
        ..e0(1)
    };
    rejects(&applied, CARRY + 1, &signed(again), "already scheduled");
}

#[tokio::test]
async fn t4_v7_run_up_and_horizon() {
    let _g = gate().await;
    let genesis = State::new();
    rejects(&genesis, CARRY, &signed(with_entries(&[("verify_unit_pricing", H0 - 1)])), "minimum run-up");
    accepts(&genesis, CARRY, &signed(with_entries(&[("verify_unit_pricing", H0)])));
    let horizon = CARRY + MAX_HORIZON_BLOCKS;
    accepts(&genesis, CARRY, &signed(with_entries(&[("verify_unit_pricing", horizon)])));
    rejects(
        &genesis,
        CARRY,
        &signed(with_entries(&[("verify_unit_pricing", horizon + 1)])),
        "maximum horizon",
    );
    // Measured from the CARRYING block: the same extension one block later is
    // one block short.
    rejects(
        &genesis,
        CARRY + 1,
        &signed(with_entries(&[("verify_unit_pricing", H0)])),
        "minimum run-up",
    );
}

#[tokio::test]
async fn t4_v8_sorted_and_strictly_appended() {
    let _g = gate().await;
    let genesis = State::new();
    rejects(
        &genesis,
        CARRY,
        &signed(with_entries(&[("verify_unit_pricing", H0 + 1), ("passkey_p256_auth", H0)])),
        "out of order",
    );
    let applied = after(
        &genesis,
        CARRY,
        &signed(with_entries(&[("verify_unit_pricing", H0 + 10)])),
    );
    let below = FeatureScheduleExtension {
        sequence: 1,
        entries: vec![ScheduleEntry::new("passkey_p256_auth", H0 + 9)],
        ..e0(1)
    };
    rejects(&applied, CARRY, &signed(below), "extensions append in activation order");
    let equal = FeatureScheduleExtension {
        sequence: 1,
        entries: vec![ScheduleEntry::new("passkey_p256_auth", H0 + 10)],
        ..e0(1)
    };
    accepts(&applied, CARRY, &signed(equal));
}

#[tokio::test]
async fn t4_v9_signatures_tampering_and_cross_domain() {
    let _g = gate().await;
    let genesis = State::new();
    rejects(&genesis, CARRY, &signed_by(e0(0), &[]), "no signatures");
    rejects(&genesis, CARRY, &signed_by(e0(0), &[outsider()]), "not in the governance allowlist");
    let mut twice = signed(e0(0));
    twice.signatures.push(twice.signatures[0].clone());
    rejects(&genesis, CARRY, &twice, "signer listed twice");

    // Every field is bound: change it after signing and the signature breaks.
    type Tamper = fn(&mut FeatureScheduleExtension);
    let tampers: [(&str, Tamper); 7] = [
        ("chain", |e| e.chain_id = "778".into()),
        ("genesis hash", |e| e.genesis_hash = OTHER_GENESIS.into()),
        ("sequence", |e| e.sequence = 1),
        ("name", |e| e.entries[0].feature = String::from("verify_unit_pricing")),
        ("height", |e| e.entries[1].activation_height = H0 + 1),
        ("min protocol version", |e| e.min_protocol_version = 4),
        ("release commit", |e| e.release_commit = "0000000".into()),
    ];
    for (field, tamper) in tampers {
        let mut forged = signed(e0(0));
        tamper(&mut forged.extension);
        let result = verdict(&genesis, CARRY, vec![tx(&forged)]);
        assert!(result.is_err(), "tampered {field} must invalidate the block");
    }
    // The signature-only case: fields the binding rules would pass.
    let mut forged = signed(e0(0));
    forged.extension.release_commit = "0000000".into();
    rejects(&genesis, CARRY, &forged, "does not verify");

    // Cross-domain: an UpgradeNotice signature by the same governance key
    // cannot stand in for an extension signature.
    let notice = UpgradeNotice {
        notice_id: format!("upgrade-{H0}-c4d0a11"),
        chain_id: CHAIN.into(),
        network_id: NETWORK.into(),
        activation_height: H0,
        min_protocol_version: 3,
        features: vec![SCHEDULE_EXTENSION_FEATURE.into()],
        release_commit: "s4-test-release".into(),
        image_digests: Default::default(),
        issued_at: 1_790_000_000,
        expires_at: None,
    };
    let notice_signature: GovernanceSignature = sign_upgrade_notice_with_key(&notice, &governance());
    let cross = SignedFeatureScheduleExtension {
        extension: e0(0),
        signatures: vec![notice_signature],
    };
    rejects(&genesis, CARRY, &cross, "does not verify");
}

// ---------------------------------------------------------------------------
// verify_block (state-free)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_block_rejects_an_unparsable_extension_and_leaves_rules_to_the_state() {
    let _g = gate().await;
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    let build = |pop: &ProofOfPublishing, extension_tx: Transaction| {
        let coinbase = Transaction::new_coinbase(
            "miner",
            wrfcoin_shared_protocol::consensus::proof_of_publishing::adjusted_block_reward(CARRY),
            CARRY,
        );
        let txs = vec![coinbase, extension_tx];
        let score = pop.calculate_block_publishing_score(&txs, CARRY);
        let mut block = Block::new(3, Hash::default(), txs, 1, CARRY);
        block
            .metadata
            .insert("publishing_score".to_string(), score.to_string());
        block
    };
    let good = build(&pop, tx(&signed(e0(0))));
    assert!(
        pop.verify_block(&good, &FxHashSet::default(), 1, &[]).await.unwrap(),
        "a parsable extension passes the state-free screen"
    );
    let mut garbage = tx(&signed(e0(0)));
    garbage.data = Some("not json".into());
    garbage.set_id();
    let bad = build(&pop, garbage);
    assert!(
        !pop.verify_block(&bad, &FxHashSet::default(), 1, &[]).await.unwrap(),
        "an unparsable extension payload makes the block invalid"
    );
}

// ---------------------------------------------------------------------------
// T8: admission policy is policy, not validity
// ---------------------------------------------------------------------------

#[tokio::test]
async fn t8_the_mempool_refuses_unknown_and_unschedulable_names_that_a_block_may_carry() {
    let _g = gate().await;
    let mut mempool = Mempool::new(&Config::default());
    let genesis = State::new();

    let refused = [
        ("a_feature_this_binary_lacks", "not implemented by this binary"),
        (ConsensusFeature::Paymaster.name(), "not schedulable"),
        (ConsensusFeature::SocialRecovery.name(), "not schedulable"),
        (ConsensusFeature::SessionKeys.name(), "not schedulable"),
        // #3361 F3: this process's block 1 is NOT the faucet row's, so the
        // bound name is refused here (it is admitted only on the row's own
        // chain, `faucet_sink_binding_admission_3361`); a suffix with no row
        // is refused on every chain.
        ("faucet_sink_binding", "not schedulable here (#3361 F3)"),
        ("faucet_sink_binding_r2", "has no faucet-binding row in this binary"),
    ];
    for (feature, reason) in refused {
        let extension = signed(with_entries(&[(feature, H0)]));
        let err = mempool
            .add_transaction(tx(&extension))
            .expect_err("the mempool must refuse it")
            .to_string();
        assert!(err.contains("admission policy") && err.contains(reason), "{feature}: {err}");
        // ...yet the same transaction in a block is VALID and applies: names
        // are not a consensus rule, so an older binary never rejects a block
        // for naming a feature it lacks (it halts cleanly at the activation).
        assert!(
            verdict(&genesis, CARRY, vec![tx(&extension)]).is_ok(),
            "{feature}: a block carrying it must be valid"
        );
        let applied = after(&genesis, CARRY, &extension);
        assert_eq!(applied.feature_schedule_extension_entries(), vec![(feature.to_string(), H0)]);
    }

    // Refused before any rule: an unparsable payload, a bad signature.
    let mut garbage = tx(&signed(e0(0)));
    garbage.data = Some("not json".into());
    garbage.set_id();
    assert!(mempool.add_transaction(garbage).is_err());
    let forged = signed_by(e0(0), &[outsider()]);
    let err = mempool.add_transaction(tx(&forged)).expect_err("unlisted signer").to_string();
    assert!(err.contains("not in the governance allowlist"), "{err}");

    // A schedulable, well-signed extension is admitted.
    mempool
        .add_transaction(tx(&signed(e0(0))))
        .expect("an admissible extension enters the mempool");
}

// ---------------------------------------------------------------------------
// Producer inclusion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_producer_includes_an_admitted_extension_only_once_the_gate_is_active() {
    let _g = gate().await;
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    pop.declare_no_genesis_oracles();
    let miner = hex::encode([0x42u8; 20]);

    // Staged while the next block is below H_ENABLE: V1 fails, so the
    // producer prunes (and evicts) it rather than building a block peers
    // would reject.
    let early = signed(with_entries(&[("verify_unit_pricing", 1 + MIN_RUNUP_BLOCKS)]));
    mempool.write().await.add_transaction(tx(&early)).unwrap();
    let block_1 = pop
        .create_next_block(chain.clone(), mempool.clone(), &miner)
        .await
        .expect("block 1");
    assert_eq!(block_1.height, 1);
    assert!(
        !block_1
            .transactions
            .iter()
            .any(|t| t.tx_type == TransactionType::FeatureScheduleExtension),
        "an extension below the gate is not included"
    );
    assert!(chain.read().await.state.feature_schedule_extensions.is_empty());

    // At H_ENABLE it is included and applied, in sequence order even if
    // staged out of order.
    let first = signed(FeatureScheduleExtension {
        entries: vec![ScheduleEntry::new("verify_unit_pricing", H_ENABLE + MIN_RUNUP_BLOCKS)],
        ..e0(0)
    });
    let second = signed(FeatureScheduleExtension {
        entries: vec![ScheduleEntry::new("passkey_p256_auth", H_ENABLE + MIN_RUNUP_BLOCKS + 1)],
        ..e0(1)
    });
    {
        let mut pool = mempool.write().await;
        pool.add_transaction(tx(&second)).unwrap();
        pool.add_transaction(tx(&first)).unwrap();
    }
    let block_2 = pop
        .create_next_block(chain.clone(), mempool.clone(), &miner)
        .await
        .expect("block 2");
    assert_eq!(block_2.height, H_ENABLE);
    let carried: Vec<u64> = block_2
        .transactions
        .iter()
        .filter_map(|t| t.parse_feature_schedule_extension())
        .map(|s| s.extension.sequence)
        .collect();
    assert_eq!(carried, vec![0, 1], "both included, in sequence order");
    let state = chain.read().await.state.clone();
    assert_eq!(
        state.feature_schedule_extension_entries(),
        vec![
            ("verify_unit_pricing".to_string(), H_ENABLE + MIN_RUNUP_BLOCKS),
            ("passkey_p256_auth".to_string(), H_ENABLE + MIN_RUNUP_BLOCKS + 1),
        ]
    );
    assert_eq!(mempool.read().await.size(), 0, "included extensions leave the mempool");
}


/// Execute the same signed public extension in binaries with the admission
/// guard false and true; changing admission never installs the schedule.
#[tokio::test]
async fn node_share_schedule_extension_admission_tracks_binary_readiness() {
    use wrfcoin_shared_protocol::crypto::chain_binding::{
        FeatureSchedule, NODE_SHARE_3527_AND_S6_COMPLETE,
    };
    let _g = gate().await;
    let mut mempool = Mempool::new(&Config::default());
    let extension = signed(with_entries(&[("node_share_settlement", 48_000)]));
    let result = mempool.add_transaction(tx(&extension));
    if NODE_SHARE_3527_AND_S6_COMPLETE {
        result.expect("ready binary admits the well-signed owner schedule extension");
    } else {
        let error = result.expect_err("incomplete binary refuses Node admission").to_string();
        assert!(error.contains("not schedulable"), "{error}");
    }
    assert!(!FeatureSchedule::default().active(ConsensusFeature::NodeShareSettlement, 48_000));
    let parent = State::new();
    // Binary admission is not a consensus rule: blocks apply either way.
    accepts(&parent, CARRY, &extension);
    let applied = after(&parent, CARRY, &extension);
    assert_eq!(applied.feature_schedule_extension_entries(), vec![("node_share_settlement".into(), 48_000)]);
    let forged = signed_by(with_entries(&[("node_share_settlement", 48_000)]), &[outsider()]);
    rejects(&parent, CARRY, &forged, "not in the governance allowlist");
    if NODE_SHARE_3527_AND_S6_COMPLETE {
        assert!(mempool.add_transaction(tx(&forged)).unwrap_err().to_string().contains("not in the governance allowlist"));
    }
}
