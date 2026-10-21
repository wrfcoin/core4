//! #3270 activation: `ConsensusFeature::WeatherSkewBlockTimeAllowance` through the REAL
//! `State::apply_transition` (`RegisterCommit` → `ApplyReveal`), replaying the
//! 2026-09-22 → 2026-09-24 halt from node container's block headers.
//!
//! The feature schedule is a process-wide, first-install-wins `OnceLock`, so
//! every leg re-executes this test binary with `--exact` and runs in its own
//! process: the "live chain" leg installs no schedule at all (the running
//! testnet carries none), the activated legs install one.

use std::process::Command;

use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{CommitPayload, RevealPayload, Transaction};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, FeatureSchedule, feature_active, install_feature_schedule,
    try_feature_schedule,
};

const LEG_ENV: &str = "WRFCOIN_3270_LEG";
const STATION: &str = "PWS-EXAMPLE-WS5000";
const CONTRIBUTION: &str = "contribution";

/// Header timestamps read from `GET /chain/block/{h}` on node container.
const H21101: (u64, i64) = (21_101, 1_790_112_212); // 2026-09-22T21:23:32Z
const H21105: (u64, i64) = (21_105, 1_790_112_541); // 2026-09-22T21:29:01Z, last pre-halt block
const H21106: (u64, i64) = (21_106, 1_790_220_929); // 2026-09-24T03:35:29Z, first post-halt block
const H21107: (u64, i64) = (21_107, 1_790_220_930); // 2026-09-24T03:35:30Z
const H21108: (u64, i64) = (21_108, 1_790_220_934); // 2026-09-24T03:35:34Z

/// the reference station’s last accepted reading (observed 21:20:00Z), and the backlog reading
/// 105600 s later (02:40:00Z on the 24th) the node skipped at h21108.
const PREV_OBS_UNIX: i64 = 1_790_112_000;
const BACKLOG_OBS_UNIX: i64 = PREV_OBS_UNIX + 105_600;

const LIVE_ERROR: &str = "future timestamp skew too large: delta=105600s exceeds limit=26100s \
                          for 7 elapsed blocks";

/// Run `test` in a child process with `LEG_ENV=leg`; returns true in the child.
fn in_child(test: &str, leg: &str) -> bool {
    if std::env::var(LEG_ENV).as_deref() == Ok(leg) {
        return true;
    }
    let status = Command::new(std::env::current_exe().expect("current exe"))
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(LEG_ENV, leg)
        .status()
        .expect("re-exec child");
    assert!(status.success(), "isolated #3270 leg {leg:?} failed");
    false
}

fn schedule_at(height: u64) {
    assert!(try_feature_schedule().is_none(), "this leg owns its process");
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![(
            ConsensusFeature::WeatherSkewBlockTimeAllowance.name().to_string(),
            height,
        )])
        .expect("schedule"),
    );
}

fn reading(temperature: f64) -> serde_json::Value {
    serde_json::json!({
        "temperature": temperature,
        "humidity": 70.0,
        "pressure": 990.0,
        "dewpoint": 6.8
    })
}

/// Commit at `commit_at`, reveal at `reveal_at` with the reveal block's header
/// timestamp — the same pair of transitions the producer and the P2P
/// extractor build.
fn commit_and_reveal(
    state: &mut State,
    nonce: &str,
    observed_unix: i64,
    commit_at: u64,
    (reveal_height, reveal_block_unix): (u64, i64),
) -> Result<(), String> {
    let data = reading(12.0);
    let commit_id =
        Transaction::compute_commit_id(&data, nonce, STATION, observed_unix, CONTRIBUTION);
    state
        .apply_transition(StateTransition::RegisterCommit {
            payload: CommitPayload {
                commit_id: commit_id.clone(),
                station_id: STATION.to_string(),
                timestamp: observed_unix,
                contribution_type: CONTRIBUTION.to_string(),
                auth_pubkey: None,
                auth_signature: None,
            },
            block_height: commit_at,
        })
        .expect("commit registers");
    state
        .apply_transition(StateTransition::ApplyReveal {
            payload: RevealPayload {
                commit_id,
                station_id: STATION.to_string(),
                timestamp: observed_unix,
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
            },
            block_height: reveal_height,
            block_timestamp: reveal_block_unix,
        })
        .map_err(|e| e.to_string())
}

fn has_block_time_section(state: &State) -> bool {
    let material = state.state_root_material();
    let tag = b"wrfcoin_station_weather_block_time_v1";
    material.windows(tag.len()).any(|w| w == tag)
}

/// Live chain (no schedule): the 2026-09-24 backlog reveal is rejected with the
/// exact error N5 logged, the accepting-block map is never written, and the
/// carried block timestamp is inert — two states that differ only in it have
/// byte-identical root material.
#[test]
fn live_chain_rejects_the_halt_backlog_with_the_logged_error_and_writes_nothing() {
    if !in_child(
        "live_chain_rejects_the_halt_backlog_with_the_logged_error_and_writes_nothing",
        "live",
    ) {
        return;
    }
    assert!(try_feature_schedule().is_none());
    assert!(!feature_active(ConsensusFeature::WeatherSkewBlockTimeAllowance, u64::MAX));

    let mut state = State::new();
    commit_and_reveal(&mut state, "n-prev", PREV_OBS_UNIX, H21101.0 - 1, H21101)
        .expect("pre-halt reading accepted");
    assert!(state.station_weather_accepted_block_unix.is_empty());
    assert!(!has_block_time_section(&state));

    // Timestamp inertness: same transitions, a different carried block time.
    let mut other = State::new();
    commit_and_reveal(&mut other, "n-prev", PREV_OBS_UNIX, H21101.0 - 1, (H21101.0, 7))
        .expect("pre-halt reading accepted");
    assert_eq!(state.state_root_material(), other.state_root_material());
    assert_eq!(state.state_root, other.state_root);

    let err = commit_and_reveal(&mut state, "n-backlog", BACKLOG_OBS_UNIX, H21107.0, H21108)
        .expect_err("pre-activation: the halt backlog is rejected");
    assert!(err.ends_with(LIVE_ERROR), "{err}");
    assert!(state.station_weather_accepted_block_unix.is_empty());
    let _ = (H21105, H21106);
}

/// Scheduled at 21000 (before the reference station’s pre-halt reading): the same backlog reveal
/// at h21108 is ACCEPTED, the accepting block's timestamp is recorded and
/// hashed, and a far-future reading after it is still rejected.
#[test]
fn activated_chain_accepts_the_halt_backlog_and_still_rejects_the_far_future() {
    if !in_child(
        "activated_chain_accepts_the_halt_backlog_and_still_rejects_the_far_future",
        "activated",
    ) {
        return;
    }
    schedule_at(21_000);
    assert!(feature_active(ConsensusFeature::WeatherSkewBlockTimeAllowance, H21101.0));

    let mut state = State::new();
    commit_and_reveal(&mut state, "n-prev", PREV_OBS_UNIX, H21101.0 - 1, H21101)
        .expect("pre-halt reading accepted");
    assert_eq!(
        state.station_weather_accepted_block_unix.get(STATION),
        Some(&H21101.1),
        "the accepting block's HEADER timestamp is recorded"
    );
    assert!(has_block_time_section(&state), "post-activation the map is root material");

    commit_and_reveal(&mut state, "n-backlog", BACKLOG_OBS_UNIX, H21107.0, H21108)
        .expect("post-activation: the halt backlog is accepted");
    assert_eq!(state.station_weather_accepted_block_unix.get(STATION), Some(&H21108.1));

    // Normal cadence after recovery: a reading 5 min later, one block on.
    commit_and_reveal(
        &mut state,
        "n-cadence",
        BACKLOG_OBS_UNIX + 300,
        H21108.0,
        (H21108.0 + 1, H21108.1 + 300),
    )
    .expect("on-cadence reading accepted");

    // Attack: ten days ahead, one block and 5 min of block time later.
    let before = state.station_weather_accepted_block_unix.clone();
    let err = commit_and_reveal(
        &mut state,
        "n-future",
        BACKLOG_OBS_UNIX + 300 + 10 * 86_400,
        H21108.0 + 1,
        (H21108.0 + 2, H21108.1 + 600),
    )
    .expect_err("a far-future reading is still rejected");
    assert!(err.contains("future timestamp skew too large"), "{err}");
    assert_eq!(state.station_weather_accepted_block_unix, before, "a rejection records nothing");

    // The carried timestamp is now consensus-relevant: it changes the root.
    let mut a = State::new();
    let mut b = State::new();
    commit_and_reveal(&mut a, "n-prev", PREV_OBS_UNIX, H21101.0 - 1, H21101).unwrap();
    commit_and_reveal(&mut b, "n-prev", PREV_OBS_UNIX, H21101.0 - 1, (H21101.0, H21101.1 + 1))
        .unwrap();
    assert_ne!(a.state_root, b.state_root);
}

/// Scheduled at 21103 — AFTER the reference station’s pre-halt reading was accepted (h21101).
/// No accepting-block timestamp exists for it, so the backlog falls back to
/// the block-count rule and fails exactly as before; the first reading
/// accepted after activation arms the block-time term for the next halt.
#[test]
fn activation_between_reading_and_halt_falls_back_to_the_block_count_rule() {
    if !in_child(
        "activation_between_reading_and_halt_falls_back_to_the_block_count_rule",
        "armed-late",
    ) {
        return;
    }
    schedule_at(21_103);
    let mut state = State::new();
    commit_and_reveal(&mut state, "n-prev", PREV_OBS_UNIX, H21101.0 - 1, H21101)
        .expect("pre-activation reading accepted");
    assert!(state.station_weather_accepted_block_unix.is_empty());

    let err = commit_and_reveal(&mut state, "n-backlog", BACKLOG_OBS_UNIX, H21107.0, H21108)
        .expect_err("no recorded accepting block → block-count rule only");
    assert!(err.ends_with(LIVE_ERROR), "{err}");
    assert!(state.station_weather_accepted_block_unix.is_empty());
}

// ── Producer: the post-halt backlog survives block assembly ──────────────
//
// The producer prunes a reveal against a scratch copy of its state BEFORE it
// builds the header, sizing the allowance with the candidate header's time
// (`prune_invalid_launch_transactions`). If pruning read any other time, the
// backlog would be dropped at the producer even though every applier would
// accept it — #3270's loss, moved one step earlier. Both legs start from the
// same state: the reference station’s pre-halt reading accepted with header time h21101, the
// backlog commit registered, the backlog reveal in the mempool, and the
// producer's clock at h21108's header time.

struct FixedClock(chrono::DateTime<chrono::Utc>);

impl wrfcoin_shared_protocol::consensus::proof_of_publishing::Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.0
    }
}

/// Mine one block over the halt; returns whether it carried the backlog
/// reveal, and the producer's accepting-block map afterwards.
async fn producer_mines_the_backlog() -> (bool, std::collections::BTreeMap<String, i64>) {
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
    use wrfcoin_shared_protocol::blockchain::transaction::{
        TransactionInput, TransactionOutput, TransactionType,
    };
    use wrfcoin_shared_protocol::config::settings::Config;
    use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
    use wrfcoin_shared_protocol::mempool::pool::Mempool;

    let mut state = State::new();
    commit_and_reveal(&mut state, "n-prev", PREV_OBS_UNIX, 0, (0, H21101.1))
        .expect("pre-halt reading accepted");

    let data = reading(12.0);
    let commit_id =
        Transaction::compute_commit_id(&data, "n-backlog", STATION, BACKLOG_OBS_UNIX, CONTRIBUTION);
    state
        .apply_transition(StateTransition::RegisterCommit {
            payload: CommitPayload {
                commit_id: commit_id.clone(),
                station_id: STATION.to_string(),
                timestamp: BACKLOG_OBS_UNIX,
                contribution_type: CONTRIBUTION.to_string(),
                auth_pubkey: None,
                auth_signature: None,
            },
            block_height: 0,
        })
        .expect("backlog commit registers");
    let reveal = RevealPayload {
        commit_id,
        station_id: STATION.to_string(),
        timestamp: BACKLOG_OBS_UNIX,
        contribution_type: CONTRIBUTION.to_string(),
        data,
        nonce: "n-backlog".to_string(),
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: None,
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: wrfcoin_shared_protocol::crypto::hash::hash_string("in-backlog-reveal"),
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
    tx.tx_type = TransactionType::ContributionReveal;
    tx.timestamp = chrono::DateTime::from_timestamp(H21108.1 - 5, 0).expect("ts");
    tx.data = Some(serde_json::to_string(&reveal).expect("reveal json"));
    tx.fee = 1;
    tx.set_id();

    let mut chain = Blockchain::default();
    chain.state = state;
    let chain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    mempool.write().await.add_transaction(tx).expect("mempool admits the reveal");
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    pop.declare_no_genesis_oracles();
    pop.set_clock(Arc::new(FixedClock(
        chrono::DateTime::from_timestamp(H21108.1, 0).expect("ts"),
    )));
    let block = pop
        .create_next_block(chain.clone(), mempool.clone(), &hex::encode([0x42u8; 20]))
        .await
        .expect("block over the halt");
    assert_eq!(block.timestamp.timestamp(), H21108.1, "fixture: header time is h21108's");
    let carried = block
        .transactions
        .iter()
        .any(|t| t.tx_type == TransactionType::ContributionReveal);
    let map = chain.read().await.state.station_weather_accepted_block_unix.clone();
    (carried, map)
}

#[tokio::test]
async fn producer_keeps_the_halt_backlog_only_once_the_feature_is_active() {
    const TEST: &str = "producer_keeps_the_halt_backlog_only_once_the_feature_is_active";
    match std::env::var(LEG_ENV).as_deref() {
        Ok("producer-live") => {
            // The running testnet: no schedule.
            assert!(try_feature_schedule().is_none());
            let (carried, map) = producer_mines_the_backlog().await;
            assert!(!carried, "pre-activation the producer prunes the backlog (block-count rule)");
            assert!(map.is_empty());
        },
        Ok("producer-on") => {
            schedule_at(0);
            let (carried, map) = producer_mines_the_backlog().await;
            assert!(carried, "post-activation the producer keeps the halt backlog");
            assert_eq!(map.get(STATION), Some(&H21108.1), "accepted at the header's time");
        },
        Ok(other) => panic!("unknown leg {other:?}"),
        Err(_) => {
            assert!(!in_child(TEST, "producer-live"));
            assert!(!in_child(TEST, "producer-on"));
        },
    }
}
