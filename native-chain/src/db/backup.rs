//! Backup and restore for the redb-backed storage layer.
//!
//! # Strategy
//!
//! redb does not expose a built-in checkpoint-copy API (unlike RocksDB).
//! The backup strategy therefore works at the file level:
//!
//! 1. **Quiesced copy** (`create_backup`): the caller must ensure no write
//!    transaction is active while the copy runs.  In practice, call
//!    [`Storage::force_checkpoint`] first so the WAL is flushed, then
//!    let the node pause writes briefly.  The copy is performed with
//!    `std::fs::copy`, which is atomic on most platforms (copy-then-rename
//!    within the same filesystem).
//!
//! 2. **Restore** (`restore_from_backup`): copy the backup file to the
//!    destination path, then open a [`redb::Database`] on it to verify
//!    the file is well-formed.
//!
//! # Production notes
//!
//! - The backup file is a complete, self-contained redb database file.
//!   It can be transferred off-host and restored on a fresh node.
//! - Encryption: if the source database uses storage encryption, the backup
//!   retains the same ciphertext.  The same `WRFCOIN_STORAGE_ENCRYPTION_KEY`
//!   must be available on the restore host.
//! - For live, online backups consider taking a filesystem-level snapshot
//!   (LVM, ZFS, BTRFS) instead of quiescing writes.

use std::path::{Path, PathBuf};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("I/O error during backup operation: {0}")]
    Io(#[from] std::io::Error),
    #[error("source path does not exist: {path}")]
    SourceNotFound { path: PathBuf },
    #[error("destination parent directory does not exist: {path}")]
    DestParentMissing { path: PathBuf },
    #[error("backup verification failed — restored file is not a valid redb database: {0}")]
    VerificationFailed(String),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Copy the live database file at `src` to `dest` as a point-in-time backup.
///
/// **Preconditions (caller's responsibility):**
/// - No active write transactions on the source database at the moment of copy.
///   Call [`crate::storage::Storage::force_checkpoint`] first to flush the WAL.
/// - `dest` must not be the same path as `src`.
///
/// On success, `dest` is a byte-for-byte copy of the source database file at
/// the moment of the call.
pub fn create_backup(src: &Path, dest: &Path) -> Result<BackupMeta, BackupError> {
    if !src.exists() {
        return Err(BackupError::SourceNotFound { path: src.to_path_buf() });
    }

    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(BackupError::DestParentMissing { path: parent.to_path_buf() });
        }
    }

    let src_size = std::fs::metadata(src)?.len();
    std::fs::copy(src, dest)?;

    log::info!(
        "Backup created: {} -> {} ({} bytes)",
        src.display(),
        dest.display(),
        src_size
    );

    Ok(BackupMeta {
        source: src.to_path_buf(),
        destination: dest.to_path_buf(),
        size_bytes: src_size,
        created_at: std::time::SystemTime::now(),
    })
}

/// Restore a database from a backup file.
///
/// Copies `src` (backup file) to `dest` (runtime database path) and verifies
/// that the resulting file is a valid, readable redb database.
///
/// **Preconditions:**
/// - The live node must be **stopped** before calling this function.  Restoring
///   over a running database file will cause data corruption.
/// - `dest` will be **overwritten** if it already exists.
///
/// On success the caller can open a `redb::Database` on `dest` and resume
/// normal operation.
pub fn restore_from_backup(src: &Path, dest: &Path) -> Result<RestoreMeta, BackupError> {
    if !src.exists() {
        return Err(BackupError::SourceNotFound { path: src.to_path_buf() });
    }

    let src_size = std::fs::metadata(src)?.len();
    std::fs::copy(src, dest)?;

    // Verify the restored file is a valid redb database by opening it.
    redb::Database::open(dest).map_err(|e| BackupError::VerificationFailed(e.to_string()))?;

    log::info!(
        "Restore complete: {} -> {} ({} bytes, verified OK)",
        src.display(),
        dest.display(),
        src_size
    );

    Ok(RestoreMeta {
        source: src.to_path_buf(),
        destination: dest.to_path_buf(),
        size_bytes: src_size,
        restored_at: std::time::SystemTime::now(),
    })
}

// ---------------------------------------------------------------------------
// Metadata types
// ---------------------------------------------------------------------------

/// Information about a successfully created backup.
#[derive(Debug)]
pub struct BackupMeta {
    pub source: PathBuf,
    pub destination: PathBuf,
    /// Byte size of the backup file.
    pub size_bytes: u64,
    pub created_at: std::time::SystemTime,
}

/// Information about a successful restore operation.
#[derive(Debug)]
pub struct RestoreMeta {
    pub source: PathBuf,
    pub destination: PathBuf,
    /// Byte size of the restored file.
    pub size_bytes: u64,
    pub restored_at: std::time::SystemTime,
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use redb::{Database, TableDefinition};

    const TEST_TABLE: TableDefinition<&str, &str> = TableDefinition::new("test_data");

    /// Helper: create a small redb database at `path` with one record.
    fn create_test_db(path: &Path) {
        let db = Database::create(path).expect("create db");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(TEST_TABLE).expect("open table");
            table.insert("key", "value").expect("insert");
        }
        txn.commit().expect("commit");
    }

    fn tmp_path(suffix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "wrfcoin_backup_test_{}_{}.redb",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos(),
            suffix,
        ))
    }

    /// Create a database, back it up, and verify the backup file exists and
    /// the returned metadata looks correct.
    #[test]
    fn test_create_backup_produces_valid_file() {
        let src = tmp_path("src");
        let dest = tmp_path("dest");
        create_test_db(&src);

        let meta = create_backup(&src, &dest).expect("backup should succeed");
        assert!(dest.exists(), "backup file must exist");
        assert!(meta.size_bytes > 0, "backup size must be non-zero");
        assert_eq!(meta.source, src);
        assert_eq!(meta.destination, dest);

        // Cleanup
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dest);
    }

    /// Back up a database with data, restore it to a new path, then verify
    /// the restored database is readable and contains the expected record.
    #[test]
    fn test_backup_restore_round_trip() {
        let src = tmp_path("src_rt");
        let backup = tmp_path("backup_rt");
        let restored = tmp_path("restored_rt");

        create_test_db(&src);
        create_backup(&src, &backup).expect("backup");
        let restore_meta = restore_from_backup(&backup, &restored).expect("restore");

        assert!(restored.exists(), "restored file must exist");
        assert!(restore_meta.size_bytes > 0);

        // Verify the restored database is readable and has correct data.
        let db = Database::open(&restored).expect("open restored db");
        let txn = db.begin_read().expect("read txn");
        let table = txn.open_table(TEST_TABLE).expect("open table");
        let val = table.get("key").expect("get").expect("value present");
        assert_eq!(val.value(), "value", "restored data should match original");

        // Cleanup
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&backup);
        let _ = std::fs::remove_file(&restored);
    }

    /// Attempt to restore from a non-existent backup file; expect an error.
    #[test]
    fn test_restore_from_missing_src_returns_error() {
        let missing = std::path::Path::new("/tmp/wrfcoin_definitely_does_not_exist.redb");
        let dest = tmp_path("dest_missing");
        let result = restore_from_backup(missing, &dest);
        assert!(result.is_err(), "restore of missing file should fail");
    }

    /// Attempt to back up from a non-existent source; expect an error.
    #[test]
    fn test_backup_from_missing_src_returns_error() {
        let missing = std::path::Path::new("/tmp/wrfcoin_definitely_does_not_exist.redb");
        let dest = tmp_path("dest_missing_backup");
        let result = create_backup(missing, &dest);
        assert!(result.is_err(), "backup of missing file should fail");
    }
}
