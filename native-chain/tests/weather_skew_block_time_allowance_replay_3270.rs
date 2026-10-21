//! #3270 S9: `replay_verify` (the production `replay_persisted_chain`, no
//! observer) replays the 2026-09-22 → 2026-09-24 halt across
//! `ConsensusFeature::WeatherSkewBlockTimeAllowance` from real weather
//! COMMIT / REVEAL transactions carried in blocks, using the blocks' own
//! HEADER timestamps (node container's h21101…h21108 gaps, compressed to heights
//! 4…11 with the same 7-block elapsed count).
//!
//! - `activated`: the chain's block 1 schedules the feature from height 1.
//!   The producer extracts and applies the backlog reveal (105 600 s newer
//!   after 7 blocks and a 30 h header gap) and records the accepting block's
//!   header time. A fresh `replay_verify` process with NOTHING installed arms
//!   the schedule from the chain's own block 1 (#3277 S5), re-extracts every
//!   block and verifies every committed root.
//! - `unstamped`: the same blocks and committed roots, but a block 1 that does
//!   not stamp the schedule — what a chain without the feature looks like to
//!   replay. It diverges at the FIRST accepted reading (h4): there the
//!   activated producer recorded the accepting block's header time into root
//!   material, which a node without the feature never writes. (Its later
//!   backlog reveal would also fail with the block-count error N5 logged; the
//!   shared-protocol `weather_skew_block_time_allowance_3270` legs pin that
//!   error.) So the fixture discriminates: a replay that ignored the chain's
//!   schedule or the carried header time cannot verify it.
//!
//! Own process per leg: the feature schedule is process-wide and
//! first-install-wins.

// The extractor's own #[cfg(test)] module references
// `crate::helpers::test_env`; same shim as replay_harness_2750.rs.
mod helpers {
    pub(crate) mod test_env {
        // Mount the canonical test-only lock and fixture pins (#3521).
        include!("../src/bin/launch_api/test_env.rs");
    }
}

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/replay_verify/replay_core.rs"]
mod replay_core;

use std::path::Path;

use rand::SeedableRng;
use rand::rngs::StdRng;

use replay_core::{ReplayVerdict, replay_persisted_chain};
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::block::{Block, STATE_ROOT_METADATA_KEY};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, RevealPayload, Transaction, TransactionInput, TransactionOutput,
    TransactionType,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion, feature_active,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::signature::PrivateKey;

const LEG_ENV: &str = "WRFCOIN_3270_REPLAY_LEG";
const DB_ENV: &str = "WRFCOIN_3270_REPLAY_DB";
const STATION: &str = "PWS-EXAMPLE-WS5000";
const CONTRIBUTION: &str = "contribution";

/// Header timestamps from node container (`GET /chain/block/{h}`), mapped onto a short
/// chain with the SAME elapsed-block count between the two readings (7).
const PRE_HALT_ACCEPT: (u64, i64) = (4, 1_790_112_212); // h21101
const LAST_PRE_HALT: (u64, i64) = (8, 1_790_112_541); // h21105
const FIRST_POST_HALT: (u64, i64) = (9, 1_790_220_929); // h21106
const BACKLOG_COMMIT: (u64, i64) = (10, 1_790_220_930); // h21107
const BACKLOG_REVEAL: (u64, i64) = (11, 1_790_220_934); // h21108
const TIP: u64 = BACKLOG_REVEAL.0;

const PREV_OBS_UNIX: i64 = 1_790_112_000;
const BACKLOG_OBS_UNIX: i64 = PREV_OBS_UNIX + 105_600;

fn schedule_raw() -> String {
    format!(
        "0:v1,1:{}",
        ConsensusFeature::WeatherSkewBlockTimeAllowance.name()
    )
}

/// What a node arms from this chain's block 1 before applying anything.
fn install_chain_schedule() {
    let raw = schedule_raw();
    let pairs = vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
        .expect("activation table");
    let features = resolve_feature_schedule(pairs, &table).expect("feature schedule");
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
}

/// Header time of every block: linear before the halt, then the 30 h gap.
fn header_unix(height: u64) -> i64 {
    match height {
        h if h <= PRE_HALT_ACCEPT.0 => PRE_HALT_ACCEPT.1 - (PRE_HALT_ACCEPT.0 - h) as i64 * 60,
        h if h <= LAST_PRE_HALT.0 => {
            PRE_HALT_ACCEPT.1
                + (h - PRE_HALT_ACCEPT.0) as i64 * (LAST_PRE_HALT.1 - PRE_HALT_ACCEPT.1)
                    / (LAST_PRE_HALT.0 - PRE_HALT_ACCEPT.0) as i64
        },
        h if h == FIRST_POST_HALT.0 => FIRST_POST_HALT.1,
        h if h == BACKLOG_COMMIT.0 => BACKLOG_COMMIT.1,
        _ => BACKLOG_REVEAL.1,
    }
}

fn reading() -> serde_json::Value {
    serde_json::json!({
        "temperature": 12.0,
        "humidity": 70.0,
        "pressure": 990.0,
        "dewpoint": 6.8
    })
}

fn tx(tx_type: TransactionType, tag: &str, data: String, unix: i64) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: wrfcoin_shared_protocol::crypto::hash::hash_string(&format!("in-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: format!("0x{}", "11".repeat(20)),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = tx_type;
    tx.timestamp = chrono::DateTime::from_timestamp(unix, 0).expect("ts");
    tx.data = Some(data);
    tx.fee = 1;
    tx.set_id();
    tx
}

/// The commit and the reveal of one weather reading observed at `observed`.
fn commit_reveal_txs(nonce: &str, observed: i64) -> (Transaction, Transaction) {
    let data = reading();
    let commit_id = Transaction::compute_commit_id(&data, nonce, STATION, observed, CONTRIBUTION);
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: STATION.to_string(),
        timestamp: observed,
        contribution_type: CONTRIBUTION.to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit.sign_authorization(&PrivateKey::generate(&mut StdRng::seed_from_u64(3270)));
    let reveal = RevealPayload {
        commit_id,
        station_id: STATION.to_string(),
        timestamp: observed,
        contribution_type: CONTRIBUTION.to_string(),
        data,
        nonce: nonce.to_string(),
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: None,
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };
    (
        tx(
            TransactionType::ContributionCommit,
            &format!("commit-{nonce}"),
            serde_json::to_string(&commit).unwrap(),
            observed,
        ),
        tx(
            TransactionType::ContributionReveal,
            &format!("reveal-{nonce}"),
            serde_json::to_string(&reveal).unwrap(),
            observed,
        ),
    )
}

fn reveals(transitions: &[StateTransition]) -> usize {
    transitions
        .iter()
        .filter(|t| matches!(t, StateTransition::ApplyReveal { .. }))
        .count()
}

/// Produce and persist the chain as the node does (extract with the block's
/// header time, apply, finalize, commit the root) under the schedule this
/// process installed. `stamp_schedule` records that schedule in block 1, as a
/// real cut does. Returns the final state.
async fn persist_chain(path: &Path, stamp_schedule: bool) -> State {
    let (prev_commit, prev_reveal) = commit_reveal_txs("n-prev", PREV_OBS_UNIX);
    let (backlog_commit, backlog_reveal) = commit_reveal_txs("n-backlog", BACKLOG_OBS_UNIX);
    let mut storage = Storage::open(path, u64::MAX).expect("open");
    let genesis = State::new();
    storage.persist_genesis_state(&genesis).await.expect("anchor");
    let mut state = genesis;
    let mut prev = Hash::default();
    for height in 1..=TIP {
        let txs = match height {
            h if h == PRE_HALT_ACCEPT.0 - 1 => vec![prev_commit.clone()],
            h if h == PRE_HALT_ACCEPT.0 => vec![prev_reveal.clone()],
            h if h == BACKLOG_COMMIT.0 => vec![backlog_commit.clone()],
            h if h == BACKLOG_REVEAL.0 => vec![backlog_reveal.clone()],
            _ => Vec::new(),
        };
        let mut block = Block::new(1, prev.clone(), txs, 1, height);
        block.timestamp = chrono::DateTime::from_timestamp(header_unix(height), 0).expect("ts");
        if height == 1 && stamp_schedule {
            block
                .metadata
                .insert(GENESIS_ACTIVATION_SCHEDULE_KEY.to_string(), schedule_raw());
        }
        let extracted = extract_transitions(&state, &block, height, block.timestamp.timestamp());
        if height == PRE_HALT_ACCEPT.0 || height == BACKLOG_REVEAL.0 {
            assert_eq!(reveals(&extracted), 1, "the reveal at {height} is extracted");
        }
        state
            .apply_transitions_batch_at_height(height, extracted.clone().into_iter())
            .unwrap_or_else(|e| panic!("producer applies height {height}: {e}"));
        state.apply_block_finalization(height);
        block.metadata.insert(
            STATE_ROOT_METADATA_KEY.to_string(),
            state.state_root.to_string(),
        );
        let hash = block.calculate_hash();
        block.set_hash(hash.clone());
        prev = hash;
        storage
            .persist_block(block, extracted, None)
            .await
            .expect("persist block");
    }
    state
}

fn in_child(test: &str, leg: &str, db: &Path) -> bool {
    if std::env::var(LEG_ENV).as_deref() == Ok(leg) {
        return true;
    }
    let status = std::process::Command::new(std::env::current_exe().expect("current exe"))
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(LEG_ENV, leg)
        .env(DB_ENV, db)
        .status()
        .expect("re-exec child");
    assert!(status.success(), "isolated #3270 replay leg {leg:?} failed");
    false
}

#[tokio::test]
async fn replay_verify_reproduces_the_halt_backlog_across_the_block_time_allowance_3270() {
    const TEST: &str = "replay_verify_reproduces_the_halt_backlog_across_the_block_time_allowance_3270";
    if std::env::var_os(LEG_ENV).is_none() {
        // Parent: isolated legs over the persisted chains.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wrfcoin.redb");
        in_child(TEST, "produce", &path);
        in_child(TEST, "activated", &path);
        in_child(TEST, "unstamped", &path);
        return;
    }
    let db = std::path::PathBuf::from(std::env::var_os(DB_ENV).expect("db path"));
    let unstamped_db = db.with_extension("unstamped.redb");
    match std::env::var(LEG_ENV).as_deref() {
        Ok("produce") => {
            install_chain_schedule();
            for (path, stamp) in [(&db, true), (&unstamped_db, false)] {
                let state = persist_chain(path, stamp).await;
                assert_eq!(
                    state.station_weather_accepted_block_unix.get(STATION),
                    Some(&BACKLOG_REVEAL.1),
                    "the producer accepted the backlog and recorded the accepting HEADER time"
                );
            }
        },
        Ok("activated") => {
            // A fresh replay_verify process: nothing installed; replay arms
            // the chain's protocol from its own block 1.
            assert!(try_feature_schedule().is_none());
            let storage = Storage::open(&db, u64::MAX).expect("reopen");
            match replay_persisted_chain(&storage, None) {
                ReplayVerdict::Verified {
                    blocks_replayed,
                    roots_asserted,
                    last_height,
                    ..
                } => {
                    assert_eq!(blocks_replayed, TIP);
                    assert_eq!(roots_asserted, TIP, "every block's root is asserted");
                    assert_eq!(last_height, TIP);
                },
                other => panic!(
                    "replay_verify must reproduce the post-halt backlog under the chain's \
                     schedule; got {other:?}"
                ),
            }
            assert!(
                feature_active(ConsensusFeature::WeatherSkewBlockTimeAllowance, 1),
                "replay armed the feature from the chain's block 1"
            );
        },
        Ok("unstamped") => {
            // Same blocks, a block 1 without the schedule: to replay this is
            // a chain without the feature.
            assert!(try_feature_schedule().is_none());
            let storage = Storage::open(&unstamped_db, u64::MAX).expect("reopen");
            match replay_persisted_chain(&storage, None) {
                ReplayVerdict::Diverged { height, .. } => assert_eq!(
                    height, PRE_HALT_ACCEPT.0,
                    "without the feature the first accepted reading records no header time, \
                     so the root diverges exactly there"
                ),
                other => panic!(
                    "discrimination: a binary without the feature must not verify this chain; \
                     got {other:?}"
                ),
            }
        },
        other => panic!("unknown leg {other:?}"),
    }
}
