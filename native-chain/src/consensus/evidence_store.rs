//! Durable equivocation evidence store (#1218).
//!
//! `consensus-engines::equivocation::InMemoryEvidenceStore` is the default
//! backend and is lost on restart — which in production means that a
//! malicious validator who double-signs can walk away unslashed if the
//! detecting node reboots before the evidence is exported or the slashing
//! transaction is broadcast.
//!
//! This module plugs the gap with a [`redb`]-backed implementation that
//! reuses the same on-disk KV store technology the rest of `native-chain`
//! already depends on (see [`crate::storage`]). Each piece of evidence is
//! stored under its canonical
//! [`EquivocationEvidence::storage_key`] in a dedicated table, serialized
//! with `bincode` (matching the `Storage::persist_block` encoding for
//! block bodies). The backing database file is separate from the main
//! blockchain database so that:
//!
//! * slashing-evidence backups can be taken independently, and
//! * the evidence store can be opened read-only by operator tooling while
//!   the node is live.
//!
//! Persistence is idempotent: persisting the same evidence twice
//! overwrites the existing row with identical bytes, satisfying the
//! [`EvidenceStore::persist`] contract required by the
//! `pos_duplicate_observation_does_not_persist_evidence` test in
//! `consensus-engines::algorithms::pos`.
//!
//! ## Production wiring
//!
//! At node startup, `launch_api/main.rs` opens the durable store at
//! `<data_dir>/equivocation_evidence.redb` and keeps an `Arc<dyn
//! EvidenceStore>` alive for the lifetime of the process. When the PoS
//! engine is constructed (today behind a factory helper; tomorrow as the
//! active consensus), it is attached via
//! `ProofOfStake::with_evidence_store` so that
//! `record_double_sign` persists through to disk.
//!
//! Refs #720 GAP-3, closes #1218.

#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use redb::{Database, ReadableTable, TableDefinition};

use wrfcoin_consensus_engines::equivocation::{
    EquivocationEvidence, EvidenceStore, SubmitterCursor,
};
use wrfcoin_consensus_engines::{ConsensusError, ConsensusResult};

/// Canonical filename for the durable evidence DB under `data_dir`.
///
/// Kept as a constant so operator tooling (audit exports, backups) can
/// locate the file without string-search through the source tree.
pub const EVIDENCE_DB_FILENAME: &str = "equivocation_evidence.redb";

/// redb table holding serialized `EquivocationEvidence` keyed by
/// `EquivocationEvidence::storage_key()` (format `"{validator}:{height:020}"`).
///
/// The zero-padded height key means lexicographic range scans over a
/// validator's evidence also return results in numeric height order, which
/// matches the `load_for_validator` contract.
const EVIDENCE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("equivocation_evidence");

/// Durable [`EvidenceStore`] implementation backed by [`redb`].
///
/// Cloneable because the inner handle is an [`Arc<Database>`]; cloning
/// shares the same underlying database, which is the intended semantics
/// for the runtime-wide evidence store.
#[derive(Debug, Clone)]
pub struct DurableEvidenceStore {
    db: Arc<Database>,
    /// Filesystem path the DB was opened from. Stored purely for logging
    /// and diagnostics; the redb handle itself does not expose it.
    path: PathBuf,
}

impl DurableEvidenceStore {
    /// Open or create the durable evidence database at the given path.
    ///
    /// Creates the `equivocation_evidence` table if it does not exist.
    /// Any error opening the DB or initialising the table surfaces as
    /// [`ConsensusError::InitializationError`] so that the caller at node
    /// startup can decide whether to abort or fall back to in-memory.
    pub fn open(path: impl AsRef<Path>) -> ConsensusResult<Self> {
        let path = path.as_ref().to_path_buf();
        let db = Database::create(&path).map_err(|e| {
            ConsensusError::InitializationError(format!(
                "failed to open evidence store at {}: {e}",
                path.display()
            ))
        })?;

        // Ensure the table exists so the first `persist` call does not
        // have to take a write lock just to create it.
        let write_txn = db.begin_write().map_err(|e| {
            ConsensusError::InitializationError(format!(
                "failed to begin write txn on evidence store: {e}"
            ))
        })?;
        {
            let _ = write_txn.open_table(EVIDENCE_TABLE).map_err(|e| {
                ConsensusError::InitializationError(format!("failed to open evidence table: {e}"))
            })?;
        }
        write_txn.commit().map_err(|e| {
            ConsensusError::InitializationError(format!(
                "failed to commit evidence table init: {e}"
            ))
        })?;

        Ok(Self {
            db: Arc::new(db),
            path,
        })
    }

    /// Open the durable store at `<data_dir>/equivocation_evidence.redb`.
    ///
    /// Convenience wrapper used by `launch_api` so the runtime does not
    /// have to know the filename.
    pub fn open_in_data_dir(data_dir: impl AsRef<Path>) -> ConsensusResult<Self> {
        Self::open(data_dir.as_ref().join(EVIDENCE_DB_FILENAME))
    }

    /// Path the store was opened from. Useful for startup logs and
    /// operator diagnostics.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl EvidenceStore for DurableEvidenceStore {
    async fn persist(&self, evidence: EquivocationEvidence) -> ConsensusResult<()> {
        // redb is synchronous; move onto the blocking pool so the async
        // runtime is not stalled by fsync on slow disks. Matches the
        // pattern used by `crate::storage::Storage::persist_block`.
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || persist_sync(&db, &evidence))
            .await
            .map_err(|e| {
                ConsensusError::SerializationError(format!(
                    "evidence persist spawn_blocking join failed: {e}"
                ))
            })?
    }

    async fn load_for_validator(
        &self,
        validator: &str,
    ) -> ConsensusResult<Vec<EquivocationEvidence>> {
        let db = Arc::clone(&self.db);
        let validator = validator.to_string();
        tokio::task::spawn_blocking(move || load_for_validator_sync(&db, &validator))
            .await
            .map_err(|e| {
                ConsensusError::SerializationError(format!(
                    "evidence load spawn_blocking join failed: {e}"
                ))
            })?
    }

    async fn count(&self) -> ConsensusResult<usize> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || count_sync(&db))
            .await
            .map_err(|e| {
                ConsensusError::SerializationError(format!(
                    "evidence count spawn_blocking join failed: {e}"
                ))
            })?
    }

    async fn list_since(
        &self,
        cursor: &SubmitterCursor,
    ) -> ConsensusResult<Vec<EquivocationEvidence>> {
        let db = Arc::clone(&self.db);
        // Clone the cursor into the blocking task — redb calls are
        // synchronous and we want the task to own its filter state.
        let cursor = cursor.clone();
        tokio::task::spawn_blocking(move || list_since_sync(&db, &cursor))
            .await
            .map_err(|e| {
                ConsensusError::SerializationError(format!(
                    "evidence list_since spawn_blocking join failed: {e}"
                ))
            })?
    }
}

fn persist_sync(db: &Database, evidence: &EquivocationEvidence) -> ConsensusResult<()> {
    let bytes = bincode::serialize(evidence).map_err(|e| {
        ConsensusError::SerializationError(format!("evidence bincode encode failed: {e}"))
    })?;

    let write_txn = db.begin_write().map_err(|e| {
        ConsensusError::SerializationError(format!("evidence store begin_write failed: {e}"))
    })?;

    {
        let mut table = write_txn.open_table(EVIDENCE_TABLE).map_err(|e| {
            ConsensusError::SerializationError(format!(
                "evidence store open_table(write) failed: {e}"
            ))
        })?;

        let key = evidence.storage_key();
        // Idempotent: overwriting with identical bytes is a no-op from
        // the caller's perspective and matches the trait contract.
        table.insert(key.as_str(), bytes.as_slice()).map_err(|e| {
            ConsensusError::SerializationError(format!("evidence store insert failed: {e}"))
        })?;
    }

    write_txn.commit().map_err(|e| {
        ConsensusError::SerializationError(format!("evidence store commit failed: {e}"))
    })?;

    Ok(())
}

fn load_for_validator_sync(
    db: &Database,
    validator: &str,
) -> ConsensusResult<Vec<EquivocationEvidence>> {
    let read_txn = db.begin_read().map_err(|e| {
        ConsensusError::SerializationError(format!("evidence store begin_read failed: {e}"))
    })?;

    let table = match read_txn.open_table(EVIDENCE_TABLE) {
        Ok(t) => t,
        // If the table doesn't exist yet there's simply no evidence —
        // return empty rather than error.
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(e) => {
            return Err(ConsensusError::SerializationError(format!(
                "evidence store open_table(read) failed: {e}"
            )));
        },
    };

    // Range scan: all keys starting with "<validator>:". The `:` byte
    // (0x3A) is the terminator; the upper bound `<validator>;` (0x3B)
    // excludes everything past this validator's keys.
    let mut lower = String::with_capacity(validator.len() + 1);
    lower.push_str(validator);
    lower.push(':');
    let mut upper = String::with_capacity(validator.len() + 1);
    upper.push_str(validator);
    upper.push(';');

    let mut out = Vec::new();
    let iter = table
        .range::<&str>(lower.as_str()..upper.as_str())
        .map_err(|e| {
            ConsensusError::SerializationError(format!("evidence store range scan failed: {e}"))
        })?;

    for entry in iter {
        let (_key, value) = entry.map_err(|e| {
            ConsensusError::SerializationError(format!("evidence store iter entry failed: {e}"))
        })?;
        let ev: EquivocationEvidence = bincode::deserialize(value.value()).map_err(|e| {
            ConsensusError::SerializationError(format!("evidence bincode decode failed: {e}"))
        })?;
        out.push(ev);
    }

    // Key layout (`validator:height_zero_padded`) already orders results
    // by height ascending, but re-sort defensively so the contract holds
    // even if the key schema is ever loosened.
    out.sort_by_key(|ev| ev.height);
    Ok(out)
}

fn list_since_sync(
    db: &Database,
    cursor: &SubmitterCursor,
) -> ConsensusResult<Vec<EquivocationEvidence>> {
    let read_txn = db.begin_read().map_err(|e| {
        ConsensusError::SerializationError(format!("evidence store begin_read failed: {e}"))
    })?;

    let table = match read_txn.open_table(EVIDENCE_TABLE) {
        Ok(t) => t,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(e) => {
            return Err(ConsensusError::SerializationError(format!(
                "evidence store open_table(read) failed: {e}"
            )));
        },
    };

    // Filter uses the per-validator watermark carried on `cursor` —
    // see `SubmitterCursor::has_processed`. A naive single-string
    // compare would drop evidence for validators whose ids sort before
    // the cursor's validator, even when the dropped records have a
    // strictly larger height. The map-based check cannot exhibit that
    // failure mode.
    let mut out = Vec::new();
    for entry in table.iter().map_err(|e| {
        ConsensusError::SerializationError(format!("evidence store iter failed: {e}"))
    })? {
        let (_key, value) = entry.map_err(|e| {
            ConsensusError::SerializationError(format!("evidence store iter entry failed: {e}"))
        })?;
        let ev: EquivocationEvidence = bincode::deserialize(value.value()).map_err(|e| {
            ConsensusError::SerializationError(format!("evidence bincode decode failed: {e}"))
        })?;
        if !cursor.has_processed(&ev) {
            out.push(ev);
        }
    }

    // Table iter is already in key order; sort defensively for robustness
    // to future key-schema tweaks.
    out.sort_by(|a, b| a.storage_key().cmp(&b.storage_key()));
    Ok(out)
}

fn count_sync(db: &Database) -> ConsensusResult<usize> {
    let read_txn = db.begin_read().map_err(|e| {
        ConsensusError::SerializationError(format!("evidence store begin_read failed: {e}"))
    })?;

    let table = match read_txn.open_table(EVIDENCE_TABLE) {
        Ok(t) => t,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
        Err(e) => {
            return Err(ConsensusError::SerializationError(format!(
                "evidence store open_table(read) failed: {e}"
            )));
        },
    };

    let mut n = 0usize;
    for entry in table.iter().map_err(|e| {
        ConsensusError::SerializationError(format!("evidence store iter failed: {e}"))
    })? {
        let _ = entry.map_err(|e| {
            ConsensusError::SerializationError(format!("evidence store iter entry failed: {e}"))
        })?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wrfcoin_consensus_engines::equivocation::{EquivocationEvidence, EvidenceStore};
    use wrfcoin_shared_protocol::crypto::hash::Hash;

    fn sample(validator: &str, height: u64, tag: u8) -> EquivocationEvidence {
        let mut first = [0u8; 32];
        first[0] = tag;
        let mut second = [0u8; 32];
        second[0] = tag ^ 0xFF;
        EquivocationEvidence {
            validator: validator.to_string(),
            height,
            first_block_hash: Hash::from(first),
            second_block_hash: Hash::from(second),
            detected_at: 1_700_000_000 + height,
        }
    }

    fn temp_db_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wrfcoin-evidence-{tag}-{}.redb", fastrand::u64(..)))
    }

    #[tokio::test]
    async fn durable_store_persists_and_loads() {
        let path = temp_db_path("persist-and-load");
        {
            let store = DurableEvidenceStore::open(&path).unwrap();
            store.persist(sample("alice", 10, 1)).await.unwrap();
            let loaded = store.load_for_validator("alice").await.unwrap();
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].height, 10);
        }
        let _ = std::fs::remove_file(&path);
    }

    /// The core contract for #1218: evidence must survive dropping the
    /// store and reopening it at the same path.
    #[tokio::test]
    async fn durable_store_survives_restart() {
        let path = temp_db_path("restart");

        // Phase 1: open, persist two pieces of evidence for alice and one
        // for bob, then drop the store (simulating node shutdown).
        {
            let store = DurableEvidenceStore::open(&path).unwrap();
            store.persist(sample("alice", 10, 1)).await.unwrap();
            store.persist(sample("alice", 25, 2)).await.unwrap();
            store.persist(sample("bob", 7, 3)).await.unwrap();
            assert_eq!(store.count().await.unwrap(), 3);
        } // <-- store dropped here

        // Phase 2: reopen at the same path and assert evidence is intact.
        {
            let store = DurableEvidenceStore::open(&path).unwrap();
            assert_eq!(
                store.count().await.unwrap(),
                3,
                "evidence must survive restart"
            );

            let alice_ev = store.load_for_validator("alice").await.unwrap();
            assert_eq!(alice_ev.len(), 2);
            assert_eq!(alice_ev[0].height, 10, "results must be height-sorted");
            assert_eq!(alice_ev[1].height, 25);
            // Tag 1 → first byte = 1, rest zero.
            let mut expected_first = [0u8; 32];
            expected_first[0] = 1;
            assert_eq!(alice_ev[0].first_block_hash, Hash::from(expected_first));

            let bob_ev = store.load_for_validator("bob").await.unwrap();
            assert_eq!(bob_ev.len(), 1);
            assert_eq!(bob_ev[0].validator, "bob");
            assert_eq!(bob_ev[0].height, 7);
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Persisting the same evidence twice (e.g. duplicate block
    /// observation from a flaky peer) must not inflate the count.
    /// Matches `pos_idempotent_double_sign` at the store level.
    #[tokio::test]
    async fn durable_store_persist_is_idempotent() {
        let path = temp_db_path("idempotent");
        {
            let store = DurableEvidenceStore::open(&path).unwrap();
            let ev = sample("alice", 10, 1);
            store.persist(ev.clone()).await.unwrap();
            store.persist(ev.clone()).await.unwrap();
            store.persist(ev).await.unwrap();
            assert_eq!(store.count().await.unwrap(), 1);

            let loaded = store.load_for_validator("alice").await.unwrap();
            assert_eq!(loaded.len(), 1);
        }

        // And survives restart with a single entry still.
        {
            let store = DurableEvidenceStore::open(&path).unwrap();
            assert_eq!(store.count().await.unwrap(), 1);
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn durable_store_load_is_height_sorted() {
        let path = temp_db_path("sorted");
        {
            let store = DurableEvidenceStore::open(&path).unwrap();
            // Insert out of order.
            store.persist(sample("alice", 100, 1)).await.unwrap();
            store.persist(sample("alice", 5, 2)).await.unwrap();
            store.persist(sample("alice", 50, 3)).await.unwrap();

            let loaded = store.load_for_validator("alice").await.unwrap();
            let heights: Vec<u64> = loaded.iter().map(|e| e.height).collect();
            assert_eq!(heights, vec![5, 50, 100]);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn durable_store_isolates_by_validator() {
        let path = temp_db_path("isolate");
        {
            let store = DurableEvidenceStore::open(&path).unwrap();
            store.persist(sample("alice", 1, 1)).await.unwrap();
            store.persist(sample("alice2", 1, 2)).await.unwrap();
            store.persist(sample("bob", 1, 3)).await.unwrap();

            let alice_ev = store.load_for_validator("alice").await.unwrap();
            assert_eq!(
                alice_ev.len(),
                1,
                "prefix scan must not match 'alice2' when asked for 'alice'"
            );
            let mut expected_alice = [0u8; 32];
            expected_alice[0] = 1;
            assert_eq!(alice_ev[0].first_block_hash, Hash::from(expected_alice));

            let alice2_ev = store.load_for_validator("alice2").await.unwrap();
            assert_eq!(alice2_ev.len(), 1);
            let mut expected_alice2 = [0u8; 32];
            expected_alice2[0] = 2;
            assert_eq!(alice2_ev[0].first_block_hash, Hash::from(expected_alice2));
        }
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn durable_store_empty_validator_returns_empty_vec() {
        let path = temp_db_path("empty");
        {
            let store = DurableEvidenceStore::open(&path).unwrap();
            let loaded = store.load_for_validator("nobody").await.unwrap();
            assert!(loaded.is_empty());
            assert_eq!(store.count().await.unwrap(), 0);
        }
        let _ = std::fs::remove_file(&path);
    }

    /// Integration test: wire the durable store into a PoS engine via
    /// `with_evidence_store`, trigger an equivocation, drop everything,
    /// reopen the store, and verify the evidence is still there. This is
    /// the end-to-end contract that #1218 is closing.
    #[tokio::test]
    async fn pos_engine_double_sign_evidence_survives_restart() {
        use wrfcoin_consensus_engines::ConsensusEngine;
        use wrfcoin_consensus_engines::algorithms::pos::ProofOfStake;
        use wrfcoin_shared_protocol::blockchain::block::Block;
        use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
        use wrfcoin_shared_protocol::crypto::hash::hash_bytes;

        let path = temp_db_path("pos-restart");

        // Phase 1: durable store + PoS engine, produce equivocation.
        {
            let store: Arc<dyn EvidenceStore> =
                Arc::new(DurableEvidenceStore::open(&path).unwrap());
            let engine = ProofOfStake::new(10, 5).with_evidence_store(Arc::clone(&store));
            engine
                .add_validator("validator-1".into(), 100)
                .await
                .unwrap();

            let chain = Blockchain::default();
            let block_a = engine
                .create_block(
                    Vec::new(),
                    &Hash::default().to_string(),
                    "validator-1",
                    &chain,
                )
                .await
                .unwrap();

            let mut block_b = Block::new(3, hash_bytes(b"other-parent"), Vec::new(), 1, 1);
            block_b
                .metadata
                .insert("validator".into(), "validator-1".into());
            block_b
                .metadata
                .insert("consensus_type".into(), "ProofOfStake".into());
            let hash_b = block_b.calculate_hash();
            block_b.set_hash(hash_b);

            assert!(engine.validate_block(&block_a, &chain).await.unwrap());
            let err = engine.validate_block(&block_b, &chain).await.unwrap_err();
            assert!(matches!(
                err,
                wrfcoin_consensus_engines::ConsensusError::MaliciousValidator(_)
            ));

            // Evidence was persisted live.
            let live = store.load_for_validator("validator-1").await.unwrap();
            assert_eq!(live.len(), 1);
            assert_eq!(live[0].first_block_hash, block_a.hash.unwrap());
            assert_eq!(live[0].second_block_hash, hash_b);
        } // <-- engine + store dropped; simulates crash/restart

        // Phase 2: a brand-new store at the same path must still see the
        // equivocation evidence. This is the durability guarantee #1218
        // delivers; without it the malicious validator walks free on
        // node reboot.
        {
            let store = DurableEvidenceStore::open(&path).unwrap();
            let loaded = store.load_for_validator("validator-1").await.unwrap();
            assert_eq!(
                loaded.len(),
                1,
                "durable evidence MUST survive node restart"
            );
            let ev = &loaded[0];
            assert_eq!(ev.validator, "validator-1");
            assert_eq!(ev.height, 1);
            assert_ne!(ev.first_block_hash, ev.second_block_hash);
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Regression guard: a second observation of the SAME block is not
    /// an equivocation and must not create a new evidence row, even
    /// after the store has been reopened. Mirrors
    /// `pos_duplicate_observation_does_not_persist_evidence` but across
    /// a restart boundary.
    #[tokio::test]
    async fn pos_engine_duplicate_observation_after_restart_is_still_noop() {
        use wrfcoin_consensus_engines::ConsensusEngine;
        use wrfcoin_consensus_engines::algorithms::pos::ProofOfStake;
        use wrfcoin_shared_protocol::blockchain::chain::Blockchain;

        let path = temp_db_path("pos-dup-restart");

        // Phase 1: produce a single block (no equivocation); store empty.
        {
            let store: Arc<dyn EvidenceStore> =
                Arc::new(DurableEvidenceStore::open(&path).unwrap());
            let engine = ProofOfStake::new(10, 5).with_evidence_store(Arc::clone(&store));
            engine
                .add_validator("validator-1".into(), 100)
                .await
                .unwrap();
            let chain = Blockchain::default();
            let block = engine
                .create_block(
                    Vec::new(),
                    &Hash::default().to_string(),
                    "validator-1",
                    &chain,
                )
                .await
                .unwrap();
            assert!(engine.validate_block(&block, &chain).await.unwrap());
            assert!(engine.validate_block(&block, &chain).await.unwrap());
            assert_eq!(store.count().await.unwrap(), 0);
        }

        // Phase 2: reopen the store; it must still be empty.
        {
            let store = DurableEvidenceStore::open(&path).unwrap();
            assert_eq!(
                store.count().await.unwrap(),
                0,
                "idempotent re-submission must not generate evidence, even across restart"
            );
        }

        let _ = std::fs::remove_file(&path);
    }
}
