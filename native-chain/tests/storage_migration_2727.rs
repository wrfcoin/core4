//! #3091 — persistence migration framework: a format-4 binary boots on a
//! format-3 volume, migrates it in place, and writes back only after the boot
//! verified (write-after-verify). Root cause #2727.
//!
//! The v3 fixture is `tests/fixtures/checkpoint_state_v3_golden.bin`, written
//! by the real format-3 code (see `tests/fixtures/README.md` for why the
//! 2026-09-10 pre-cut volume is not a v3 fixture).

use std::path::{Path, PathBuf};

use redb::{Database, ReadableTable, TableDefinition};
use wrfcoin_native_chain::storage::{
    legacy, migrations, Storage, StorageCorruptionError, StorageError,
    CHECKPOINT_ENVELOPE_MAGIC, CHECKPOINT_FORMAT_VERSION,
};
use wrfcoin_shared_protocol::accounts::paymaster::PaymasterAccount;
use wrfcoin_shared_protocol::accounts::recovery::AccountRecoveryState;
use wrfcoin_shared_protocol::accounts::session_keys::OwnerSessionKeys;
use wrfcoin_shared_protocol::accounts::authorization::AccountAuthorizationDeclaration;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::{Address, Balance};

const CHECKPOINTS_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("checkpoints");
const HEADER_LEN: usize = 12;

#[test]
fn owner_ledger_checkpoint_allocation_preserves_deployed_weather_format() {
    // Formats 10/11 retain weather/owner layout; bond owns12, receipts13,
    // unminted settlement14, node fraud/forfeiture 15, send allowlists 16
    // (written only when non-empty). Old deployed fixture headers remain pinned.
    assert_eq!(CHECKPOINT_FORMAT_VERSION, 16);
    let bonded = fixture("checkpoint_state_v12_golden.bin");
    assert_eq!(u32::from_le_bytes(bonded[8..12].try_into().unwrap()), 12);
    assert!(migrations::has_path(12));
    let owner = fixture("checkpoint_state_v11_golden.bin");
    assert_eq!(u32::from_le_bytes(owner[8..12].try_into().unwrap()), 11);
    assert!(migrations::has_path(11));
    let deployed = fixture("checkpoint_state_v10_golden.bin");
    assert_eq!(u32::from_le_bytes(deployed[8..12].try_into().unwrap()), 10);
    assert!(migrations::has_path(10));
}

/// Pinned from the first successful build container run (#3091 test (b)); explained in
/// the PR. The stored root is what the format-3 code committed into the
/// golden: all zeros, because the regenerator builds the fixture State
/// directly and never calls `recalculate_state_root`. The recomputed root is
/// what the CURRENT domain derives from the migrated fields; it moves only if
/// a field's value or the root material changes, never by migration alone.
const V3_GOLDEN_STORED_STATE_ROOT: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";
const V3_GOLDEN_RECOMPUTED_STATE_ROOT: &str =
    "20a1ab648c634367c268421e34bafe209cc9de681e16f824cb324f05bf5bf677";

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

fn body(envelope: &[u8]) -> &[u8] {
    &envelope[HEADER_LEN..]
}

fn envelope(version: u32, body: &[u8]) -> Vec<u8> {
    migrations::envelope(version, body)
}

fn decode_current(envelope_bytes: &[u8]) -> State {
    use bincode::Options;
    let version = u32::from_le_bytes(envelope_bytes[8..12].try_into().unwrap());
    let migrated = migrations::migrate_to_current(version, body(envelope_bytes)).unwrap();
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize(body(&migrated))
        .expect("current State decode")
}

fn temp_db() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrfcoin.redb");
    (dir, path)
}

fn addr(b: u8) -> Address {
    Address([b; 20])
}

/// Build a real chain through the public API: `blocks` blocks with a
/// checkpoint every `interval` (so with `blocks > interval` the tail lives in
/// the WAL). Mirrors the storage unit-test helper.
async fn persist_chain(path: &Path, interval: u64, blocks: u64, user: Address) {
    let mut storage = Storage::open(path, interval).unwrap();
    let mut blockchain = Blockchain::default();
    blockchain.state.create_account(user, Balance(0));
    for height in 1..=blocks {
        let transition = StateTransition::ApplyReward {
            address: user,
            amount: Balance(11),
            reason: "migration-test".to_string(),
        };
        blockchain.apply_state_transition(transition.clone()).unwrap();
        let mut block = Block::new(3, blockchain.tip, vec![], 1, height);
        let hash = block.calculate_hash();
        block.set_hash(hash);
        blockchain.add_block(block.clone()).unwrap();
        storage
            .persist_block(
                block.clone(),
                vec![transition],
                storage.checkpoint_state_bytes(block.height, &blockchain.state).unwrap(),
            )
            .await
            .unwrap();
    }
}

fn read_row(path: &Path, height: u64) -> Vec<u8> {
    let db = Database::open(path).unwrap();
    let txn = db.begin_read().unwrap();
    let table = txn.open_table(CHECKPOINTS_TABLE).unwrap();
    table.get(height).unwrap().expect("row").value().to_vec()
}

fn write_row(path: &Path, height: u64, bytes: &[u8]) {
    let db = Database::open(path).unwrap();
    let txn = db.begin_write().unwrap();
    {
        let mut table = txn.open_table(CHECKPOINTS_TABLE).unwrap();
        table.insert(height, bytes).unwrap();
    }
    txn.commit().unwrap();
}

/// Turn a freshly written format-10 row into what the format-3 binary would
/// have written for the same state. Remove the append-only defaults in reverse
/// order: `station_weather_accepted_block_unix` (#3270) first, then the
/// empty extension list (#3277), then `paymasters`, declarations, session
/// keys, recovery, and
/// finally `last_producer_settlement = None` (`0x00`). This is valid only when
/// all appended fields have their defaults, which the test chain guarantees.
fn downgrade_to_v3(current_row: &[u8]) -> Vec<u8> {
    assert_eq!(&current_row[..8], &CHECKPOINT_ENVELOPE_MAGIC);
    assert_eq!(
        u32::from_le_bytes(current_row[8..12].try_into().unwrap()),
        10
    );
    let current = decode_current(current_row);
    assert!(current.last_producer_settlement.is_none());
    assert!(current.account_recovery.is_empty());
    assert!(current.session_keys.is_empty());
    assert!(current.authorization_declarations.is_empty());
    assert!(current.paymasters.is_empty());
    assert!(current.feature_schedule_extensions.is_empty());
    assert!(current.station_owner_ledger.is_empty());
    assert!(current.station_weather_accepted_block_unix.is_empty());
    let v10_body = body(current_row);
    let empty_station_block_time =
        bincode::serialize(&std::collections::BTreeMap::<String, i64>::new())
            .expect("serialize empty station block-time map");
    assert!(
        v10_body.ends_with(&empty_station_block_time),
        "format-10 row must end with the canonical empty station block-time map"
    );
    let v9_body = &v10_body[..v10_body.len() - empty_station_block_time.len()];
    let empty_extensions = bincode::serialize(&Vec::<
        wrfcoin_shared_protocol::governance::schedule_extension::AppliedExtension,
    >::new())
    .expect("serialize empty extension list");
    assert!(
        v9_body.ends_with(&empty_extensions),
        "format-9 row must end with the canonical empty extension list"
    );
    let v8_body = &v9_body[..v9_body.len() - empty_extensions.len()];
    let empty_paymasters = bincode::serialize(
        &std::collections::BTreeMap::<Address, PaymasterAccount>::new(),
    )
    .expect("serialize empty paymaster map");
    assert!(
        v8_body.ends_with(&empty_paymasters),
        "format-8 row must end with canonical empty paymasters"
    );
    let v7_body = &v8_body[..v8_body.len() - empty_paymasters.len()];
    let empty_declarations = bincode::serialize(&std::collections::BTreeMap::<Address, AccountAuthorizationDeclaration>::new()).expect("serialize empty declaration map");
    assert!(v7_body.ends_with(&empty_declarations), "format-7 row must end with canonical empty declarations");
    let v6_body = &v7_body[..v7_body.len() - empty_declarations.len()];
    let empty_session_keys =
        bincode::serialize(&std::collections::BTreeMap::<Address, OwnerSessionKeys>::new())
            .expect("serialize empty session-key map");
    assert!(
        v6_body.ends_with(&empty_session_keys),
        "format-6 row must end with the canonical empty session-key map"
    );
    let v5_body = &v6_body[..v6_body.len() - empty_session_keys.len()];
    let empty_recovery = bincode::serialize(
        &std::collections::BTreeMap::<Address, AccountRecoveryState>::new(),
    )
    .expect("serialize empty recovery map");
    assert!(
        v5_body.ends_with(&empty_recovery),
        "format-5 row must end with the canonical empty recovery map"
    );
    let v4_body = &v5_body[..v5_body.len() - empty_recovery.len()];
    assert_eq!(*v4_body.last().unwrap(), 0u8, "trailing Option must encode as None");
    let v3_body = &v4_body[..v4_body.len() - 1];
    use bincode::Options;
    let _: legacy::state_v3::State = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .reject_trailing_bytes()
        .deserialize(v3_body)
        .expect("downgraded row must be a complete format-3 State");
    envelope(3, v3_body)
}

fn sorted_keys<V>(m: &std::collections::HashMap<String, V>) -> Vec<String> {
    let mut k: Vec<String> = m.keys().cloned().collect();
    k.sort();
    k
}

// ---------------------------------------------------------------------------
// (a) v3 fixture -> v4 decodes and every migrated field equals the frozen-type decode
// ---------------------------------------------------------------------------
#[test]
fn v3_fixture_migrates_to_v4_and_every_field_matches_the_frozen_decode() {
    use bincode::Options;
    let v3 = fixture("checkpoint_state_v3_golden.bin");
    let frozen: legacy::state_v3::State = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .reject_trailing_bytes()
        .deserialize(body(&v3))
        .expect("frozen v3 decode");

    let migrated = migrations::migrate_to_current(3, body(&v3)).expect("v3 -> v4");
    assert_eq!(u32::from_le_bytes(migrated[8..12].try_into().unwrap()), CHECKPOINT_FORMAT_VERSION);
    let current = decode_current(&migrated);

    // The appended field is the default.
    assert!(current.last_producer_settlement.is_none());

    // Scalars and ordered containers: byte-for-byte via bincode.
    assert_eq!(current.state_root, frozen.state_root);
    assert_eq!(current.oracle_quorum, frozen.oracle_quorum);
    assert_eq!(current.last_state_pruning_height, frozen.last_state_pruning_height);
    assert_eq!(current.archived_quality_policy_event_count, frozen.archived_quality_policy_event_count);
    assert_eq!(
        current.archived_quality_policy_event_through_event_id,
        frozen.archived_quality_policy_event_through_event_id
    );
    assert_eq!(current.supply_genesis, frozen.supply_genesis);
    assert_eq!(current.supply_authorized_minting, frozen.supply_authorized_minting);
    assert_eq!(current.supply_authorized_burning, frozen.supply_authorized_burning);
    assert_eq!(current.supply_baseline_initialized, frozen.supply_baseline_initialized);
    assert_eq!(current.shielded_pool, frozen.shielded_pool);
    assert_eq!(current.supply_violation, frozen.supply_violation);
    assert_eq!(current.emission_window_start, frozen.emission_window_start);
    macro_rules! same_bytes {
        ($field:ident) => {
            assert_eq!(
                bincode::serialize(&current.$field).unwrap(),
                bincode::serialize(&frozen.$field).unwrap(),
                concat!("field `", stringify!($field), "` differs after migration")
            );
        };
    }
    same_bytes!(contribution_reward_records);
    same_bytes!(finalized_contribution_rewards);
    same_bytes!(producer_set_history);
    same_bytes!(quality_policy_events);
    same_bytes!(audit_events);
    same_bytes!(quality_policy_event_archive);
    same_bytes!(station_counts);
    same_bytes!(recent_weather_observations);
    same_bytes!(oracle_local_accuracy);
    same_bytes!(attester_settlement_due);
    same_bytes!(attester_settlements);
    same_bytes!(genesis_emission_config);

    // Unordered containers: same key sets.
    let mut cur_accounts: Vec<[u8; 20]> = current.accounts.keys().map(|a| a.0).collect();
    cur_accounts.sort();
    let mut frozen_accounts: Vec<[u8; 20]> = frozen.accounts.keys().map(|a| a.0).collect();
    frozen_accounts.sort();
    assert_eq!(cur_accounts, frozen_accounts);
    let mut cur_commits: Vec<String> = current.pending_commits.keys().cloned().collect();
    cur_commits.sort();
    assert_eq!(cur_commits, sorted_keys(&frozen.pending_commits));
    let mut cur_reveals: Vec<String> = current.pending_reveals.keys().cloned().collect();
    cur_reveals.sort();
    assert_eq!(cur_reveals, sorted_keys(&frozen.pending_reveals));
    let mut cur_oracles: Vec<String> = current.oracle_set.iter().cloned().collect();
    cur_oracles.sort();
    let mut frozen_oracles: Vec<String> = frozen.oracle_set.iter().cloned().collect();
    frozen_oracles.sort();
    assert_eq!(cur_oracles, frozen_oracles);
    assert_eq!(current.nfts.len(), frozen.nfts.len());
    assert_eq!(current.attestations.len(), frozen.attestations.len());
    assert_eq!(current.mining_pools.len(), frozen.mining_pools.len());
    assert_eq!(current.emission_window_spent.len(), frozen.emission_window_spent.len());
}

// ---------------------------------------------------------------------------
// (b) recomputed state_root under the current domain matches the pinned golden
// ---------------------------------------------------------------------------
#[test]
fn migrated_v3_fixture_state_root_matches_the_pinned_golden() {
    let v3 = fixture("checkpoint_state_v3_golden.bin");
    let migrated = migrations::migrate_to_current(3, body(&v3)).expect("v3 -> v4");
    let mut state = decode_current(&migrated);
    let stored = state.state_root.to_string();
    state.recalculate_state_root();
    let recomputed = state.state_root.to_string();
    assert_eq!(
        (stored.as_str(), recomputed.as_str()),
        (V3_GOLDEN_STORED_STATE_ROOT, V3_GOLDEN_RECOMPUTED_STATE_ROOT),
        "pin these from the first successful run: stored={stored} recomputed={recomputed}"
    );
}

// ---------------------------------------------------------------------------
// (c) a v5 envelope on a v4 binary still refuses with CheckpointFormatVersionMismatch
// ---------------------------------------------------------------------------
#[tokio::test]
async fn a_newer_envelope_still_refuses_with_the_named_version_mismatch() {
    let (_dir, path) = temp_db();
    persist_chain(&path, 2, 2, addr(70)).await;
    let original = read_row(&path, 2);
    let newer = envelope(CHECKPOINT_FORMAT_VERSION + 1, body(&original));
    write_row(&path, 2, &newer);

    let storage = Storage::open(&path, 2).unwrap();
    let err = storage.load_state().expect_err("newer format must refuse");
    match err {
        StorageError::Corruption(StorageCorruptionError::CheckpointFormatVersionMismatch {
            found,
            supported,
            ..
        }) => {
            assert_eq!(found, CHECKPOINT_FORMAT_VERSION + 1);
            assert_eq!(supported, CHECKPOINT_FORMAT_VERSION);
        },
        other => panic!("expected CheckpointFormatVersionMismatch, got {other}"),
    }
    assert!(storage.pending_checkpoint_migrations().is_empty());
    drop(storage);
    assert_eq!(read_row(&path, 2), newer, "refusal never touches the bytes");
}

// ---------------------------------------------------------------------------
// (d) garbage under a v3 header refuses as corruption, not as a migration
// ---------------------------------------------------------------------------
#[tokio::test]
async fn garbage_under_a_v3_header_is_corruption_not_a_migration() {
    let (_dir, path) = temp_db();
    persist_chain(&path, 2, 2, addr(71)).await;

    let real_body = body(&read_row(&path, 2)).to_vec();
    for junk in [vec![0xFFu8; 64], real_body[..40].to_vec(), Vec::new()] {
        let row = envelope(3, &junk);
        write_row(&path, 2, &row);
        let storage = Storage::open(&path, 2).unwrap();
        let err = storage.load_state().expect_err("garbage must refuse");
        let text = err.to_string();
        match err {
            StorageError::Corruption(StorageCorruptionError::SnapshotUnreadable {
                height,
                ..
            }) => assert_eq!(height, 2),
            other => panic!("expected SnapshotUnreadable, got {other}"),
        }
        assert!(text.contains("EXIT_CORRUPT_SNAPSHOT"), "{text}");
        assert!(!text.contains("EXIT_CHECKPOINT_MIGRATION_UNAVAILABLE"), "{text}");
        assert!(storage.pending_checkpoint_migrations().is_empty());
        drop(storage);
        assert_eq!(read_row(&path, 2), row, "a failed migration leaves the bytes untouched");
    }
}

// ---------------------------------------------------------------------------
// (e) + acceptance: a v4 binary boots on a v3 row, migrates in memory, writes back
//     only on commit, and the written row is byte-identical to a native v4 row
// ---------------------------------------------------------------------------
#[tokio::test]
async fn v3_row_boots_migrates_in_memory_and_writes_back_only_on_commit() {
    let (_dir, path) = temp_db();
    persist_chain(&path, 2, 2, addr(72)).await;
    let native_v4 = read_row(&path, 2);
    let v3_row = downgrade_to_v3(&native_v4);
    write_row(&path, 2, &v3_row);

    // Boot 1: migrates in memory, records the pending write, never commits.
    {
        let storage = Storage::open(&path, 2).unwrap();
        let chain = storage.load_state().expect("v3 row must load").expect("state present");
        assert_eq!(chain.height, 2);
        assert!(chain.state.last_producer_settlement.is_none());
        assert!(chain.state.account_recovery.is_empty());
        assert_eq!(storage.pending_checkpoint_migrations(), vec![(2, 3)]);
        // No commit: simulates a boot that failed a later assertion.
    }
    assert_eq!(read_row(&path, 2), v3_row, "write-after-verify: nothing written without commit");

    // Boot 2: migrates again, then the verified boot commits the write-back.
    {
        let storage = Storage::open(&path, 2).unwrap();
        let chain = storage.load_state().unwrap().unwrap();
        assert_eq!(chain.height, 2);
        assert_eq!(storage.pending_checkpoint_migrations(), vec![(2, 3)]);
        let written = storage.commit_migrated_checkpoints().expect("write-back");
        assert_eq!(written, vec![(2, 3)]);
        assert!(storage.pending_checkpoint_migrations().is_empty());
        assert_eq!(storage.commit_migrated_checkpoints().unwrap(), vec![], "idempotent");
    }
    assert_eq!(
        read_row(&path, 2),
        native_v4,
        "the migrated row is byte-identical to what a v4 binary writes for this state"
    );

    // Boot 3: nothing left to migrate.
    let storage = Storage::open(&path, 2).unwrap();
    let chain = storage.load_state().unwrap().unwrap();
    assert_eq!(chain.height, 2);
    assert!(storage.pending_checkpoint_migrations().is_empty());
    assert_eq!(storage.commit_migrated_checkpoints().unwrap(), vec![]);
}

// ---------------------------------------------------------------------------
// WAL: a v3-era WAL replays on top of a migrated checkpoint unchanged
// ---------------------------------------------------------------------------
#[tokio::test]
async fn wal_v3_era_entries_replay_on_top_of_a_migrated_checkpoint() {
    let (_dir, path) = temp_db();
    // Checkpoint at 4; blocks 5 and 6 live only in the WAL.
    persist_chain(&path, 4, 6, addr(73)).await;
    let native_v4 = read_row(&path, 4);
    write_row(&path, 4, &downgrade_to_v3(&native_v4));

    let storage = Storage::open(&path, 4).unwrap();
    let chain = storage.load_state().expect("load").expect("state");
    assert_eq!(chain.height, 6, "WAL entries after the migrated checkpoint replayed");
    assert!(chain.state.account_recovery.is_empty());
    assert_eq!(storage.pending_checkpoint_migrations(), vec![(4, 3)]);
    assert_eq!(
        chain.state.accounts.get(&addr(73)).map(|a| a.balance),
        Some(Balance(66)),
        "six ApplyReward transitions of 11 each"
    );
}

// ---------------------------------------------------------------------------
// No registered path: the named migration gap, never a decode, bytes untouched
// ---------------------------------------------------------------------------
#[tokio::test]
async fn a_v2_row_refuses_with_the_named_migration_gap() {
    let (_dir, path) = temp_db();
    persist_chain(&path, 2, 2, addr(74)).await;
    let v2 = fixture("checkpoint_state_v2_golden.bin");
    assert_eq!(u32::from_le_bytes(v2[8..12].try_into().unwrap()), 2);
    write_row(&path, 2, &v2);

    let storage = Storage::open(&path, 2).unwrap();
    let err = storage.load_state().expect_err("v2 has no path");
    let text = err.to_string();
    match err {
        StorageError::Corruption(StorageCorruptionError::CheckpointMigrationUnavailable {
            height,
            found,
            supported,
        }) => {
            assert_eq!((height, found, supported), (2, 2, CHECKPOINT_FORMAT_VERSION));
        },
        other => panic!("expected CheckpointMigrationUnavailable, got {other}"),
    }
    assert!(text.contains("EXIT_CHECKPOINT_MIGRATION_UNAVAILABLE"), "{text}");
    assert!(text.contains("NOT corrupt"), "{text}");
    assert!(!migrations::has_path(2));
    drop(storage);
    assert_eq!(read_row(&path, 2), v2);
}

#[test]
fn frozen_legacy_module_has_no_callers_outside_migrations() {
    // Structural guard for the acceptance line "zero non-test callers
    // outside migrations.rs": grep the crate source for the module path.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().map(|e| e == "rs").unwrap_or(false) {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&src, &mut files);
    for file in files {
        let rel = file.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        if rel == "storage/migrations.rs" || rel.starts_with("storage/legacy/") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        if text.contains("legacy::state_v3") || text.contains("storage::legacy") {
            offenders.push(rel);
        }
    }
    assert!(offenders.is_empty(), "frozen layouts referenced outside migrations.rs: {offenders:?}");
}
