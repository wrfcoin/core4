//! Versioned schema migration framework for the redb-backed storage layer.
//!
//! # Overview
//!
//! Migrations are plain Rust closures that receive a mutable reference to a
//! [`redb::WriteTransaction`]. Each migration has an integer version, an
//! optional `down` closure for rollback, and a human-readable description.
//!
//! [`MigrationRunner`] stores the current schema version in the `_migrations`
//! redb table under the key `"schema_version"`, encoded as an 8-byte
//! little-endian u64. On startup the caller should:
//!
//! ```no_run
//! use redb::Database;
//! use wrfcoin_native_chain::db::migrations::{Migration, MigrationRunner};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let db = Database::create("my.db")?;
//! let mut runner = MigrationRunner::new(db);
//!
//! runner.register(Migration::new(1, "add-foo-table", |txn| {
//!     // create or modify tables here
//!     let _ = txn; // no-op example
//!     Ok(())
//! }));
//!
//! runner.run()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Rollback
//!
//! Attach a `down` function via [`Migration::with_down`] to support rolling
//! back to the previous version:
//!
//! ```no_run
//! use wrfcoin_native_chain::db::migrations::Migration;
//!
//! Migration::new(1, "add-foo-table", |_txn| Ok(()))
//!     .with_down(|_txn| Ok(()));
//! ```

use redb::{Database, TableDefinition, WriteTransaction};
use thiserror::Error;

/// Internal redb table that stores migration bookkeeping data.
const MIGRATIONS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("_migrations");
/// Metadata key that holds the current schema version (little-endian u64).
const SCHEMA_VERSION_KEY: &str = "schema_version";

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("redb error: {0}")]
    Redb(#[from] redb::Error),
    #[error("redb database error: {0}")]
    RedbDatabase(#[from] redb::DatabaseError),
    #[error("redb transaction error: {0}")]
    RedbTransaction(#[from] redb::TransactionError),
    #[error("redb table error: {0}")]
    RedbTable(#[from] redb::TableError),
    #[error("redb storage error: {0}")]
    RedbStorage(#[from] redb::StorageError),
    #[error("redb commit error: {0}")]
    RedbCommit(#[from] redb::CommitError),
    #[error("migration {version} ({description}) failed: {source}")]
    MigrationFailed {
        version: u32,
        description: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("rollback of migration {version} ({description}) failed: {source}")]
    RollbackFailed {
        version: u32,
        description: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("schema version record in database is invalid (expected 8 bytes, got {actual})")]
    CorruptVersion { actual: usize },
}

// ---------------------------------------------------------------------------
// Migration
// ---------------------------------------------------------------------------

/// A single versioned database migration.
///
/// Versions must be positive integers assigned in ascending, contiguous order
/// starting at 1.  [`MigrationRunner`] will reject a migration list that has
/// gaps or duplicates.
pub struct Migration {
    /// Monotonically-increasing schema version this migration targets.
    pub version: u32,
    /// Human-readable description written to the log on apply.
    pub description: String,
    /// The "up" function — applied when moving to `version`.
    up: Box<dyn Fn(&WriteTransaction) -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + Sync>,
    /// The optional "down" function — applied when rolling back from `version`.
    down: Option<Box<dyn Fn(&WriteTransaction) -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + Sync>>,
}

impl std::fmt::Debug for Migration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Migration")
            .field("version", &self.version)
            .field("description", &self.description)
            .finish()
    }
}

impl Migration {
    /// Create a new migration with an `up` function.
    ///
    /// `up` receives the active write transaction. The caller must **not**
    /// commit or abort the transaction — [`MigrationRunner`] handles that.
    pub fn new<F>(version: u32, description: impl Into<String>, up: F) -> Self
    where
        F: Fn(&WriteTransaction) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
            + Send
            + Sync
            + 'static,
    {
        Self {
            version,
            description: description.into(),
            up: Box::new(up),
            down: None,
        }
    }

    /// Attach a rollback ("down") function to this migration.
    pub fn with_down<F>(mut self, down: F) -> Self
    where
        F: Fn(&WriteTransaction) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
            + Send
            + Sync
            + 'static,
    {
        self.down = Some(Box::new(down));
        self
    }

    /// Whether this migration has a rollback function registered.
    pub fn has_rollback(&self) -> bool {
        self.down.is_some()
    }
}

// ---------------------------------------------------------------------------
// MigrationRunner
// ---------------------------------------------------------------------------

/// Runs pending migrations against a [`redb::Database`].
///
/// Register migrations in ascending version order, then call [`run`](Self::run)
/// once at startup.  Calling `run` a second time is idempotent — migrations
/// that have already been applied are skipped.
pub struct MigrationRunner {
    db: Database,
    migrations: Vec<Migration>,
}

impl MigrationRunner {
    /// Create a new runner bound to `db`.
    pub fn new(db: Database) -> Self {
        Self { db, migrations: Vec::new() }
    }

    /// Register a migration.  Migrations should be registered in ascending
    /// version order before calling [`run`](Self::run).
    pub fn register(&mut self, migration: Migration) {
        self.migrations.push(migration);
    }

    /// Return the schema version currently stored in `db`, or `0` if the
    /// `_migrations` table does not yet exist / has no version record.
    pub fn current_version(&self) -> Result<u32, MigrationError> {
        let read_txn = self.db.begin_read()?;
        // The table may not exist yet on a brand-new database.
        match read_txn.open_table(MIGRATIONS_TABLE) {
            Ok(table) => match table.get(SCHEMA_VERSION_KEY)? {
                Some(v) => decode_version(v.value()),
                None => Ok(0),
            },
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(0),
            Err(e) => Err(MigrationError::RedbTable(e)),
        }
    }

    /// Apply all pending migrations in ascending version order.
    ///
    /// Each migration is committed in its own transaction so that a failure
    /// partway through leaves the database at the last successfully applied
    /// version rather than fully rolling back to the start.
    ///
    /// Returns the final schema version after all pending migrations are
    /// applied.
    pub fn run(&self) -> Result<u32, MigrationError> {
        let current = self.current_version()?;
        let mut version = current;

        for migration in &self.migrations {
            if migration.version <= version {
                // Already applied.
                continue;
            }

            log::info!(
                "Applying migration v{}: {}",
                migration.version,
                migration.description
            );

            let write_txn = self.db.begin_write()?;
            {
                // Ensure the bookkeeping table exists.
                let _ = write_txn.open_table(MIGRATIONS_TABLE)?;
                // Run the caller's up function.
                (migration.up)(&write_txn).map_err(|source| MigrationError::MigrationFailed {
                    version: migration.version,
                    description: migration.description.clone(),
                    source,
                })?;
                // Persist the new schema version inside the same transaction.
                let mut meta = write_txn.open_table(MIGRATIONS_TABLE)?;
                meta.insert(SCHEMA_VERSION_KEY, encode_version(migration.version).as_slice())?;
            }
            write_txn.commit()?;
            version = migration.version;

            log::info!(
                "Migration v{} applied successfully. Schema version is now {}.",
                migration.version,
                version
            );
        }

        if version == current && !self.migrations.is_empty() {
            log::debug!("No pending migrations — schema is already at version {version}.");
        }

        Ok(version)
    }

    /// Roll back the most recently applied migration, if it has a `down`
    /// function registered.
    ///
    /// Returns the schema version after rollback.
    pub fn rollback(&self) -> Result<u32, MigrationError> {
        let current = self.current_version()?;
        if current == 0 {
            log::warn!("rollback called but schema version is already 0 — nothing to do.");
            return Ok(0);
        }

        let migration = self
            .migrations
            .iter()
            .find(|m| m.version == current)
            .ok_or_else(|| MigrationError::MigrationFailed {
                version: current,
                description: "<unknown>".to_string(),
                source: "No migration registered for current version".into(),
            })?;

        let down = migration.down.as_ref().ok_or_else(|| MigrationError::RollbackFailed {
            version: current,
            description: migration.description.clone(),
            source: "Migration has no down function registered".into(),
        })?;

        log::info!(
            "Rolling back migration v{}: {}",
            migration.version,
            migration.description
        );

        let write_txn = self.db.begin_write()?;
        {
            let _ = write_txn.open_table(MIGRATIONS_TABLE)?;
            down(&write_txn).map_err(|source| MigrationError::RollbackFailed {
                version: migration.version,
                description: migration.description.clone(),
                source,
            })?;
            let target_version = current.saturating_sub(1);
            let mut meta = write_txn.open_table(MIGRATIONS_TABLE)?;
            meta.insert(SCHEMA_VERSION_KEY, encode_version(target_version).as_slice())?;
        }
        write_txn.commit()?;

        let new_version = current.saturating_sub(1);
        log::info!("Rolled back to schema version {new_version}.");
        Ok(new_version)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn encode_version(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

fn decode_version(bytes: &[u8]) -> Result<u32, MigrationError> {
    // Support both 4-byte (current) and legacy 8-byte (u64) encodings.
    match bytes.len() {
        4 => Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        8 => {
            // Legacy 8-byte encoding — safely truncate (version will never exceed u32::MAX).
            let v = u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
                bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Ok(v as u32)
        }
        actual => Err(MigrationError::CorruptVersion { actual }),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use redb::Database;

    fn in_memory_db() -> Database {
        // redb does not have a true in-memory backend; use a temp file instead.
        // `tempfile` picks a unique, collision-free name per call (#3492), and the
        // file is unlinked when the guard drops at the end of this function: the
        // open `Database` handle keeps the inode alive for the rest of the test.
        let file = tempfile::Builder::new()
            .prefix("wrfcoin_migrations_test_")
            .suffix(".redb")
            .tempfile()
            .expect("temp file");
        Database::create(file.path()).expect("temp db")
    }

    #[test]
    fn test_fresh_db_version_is_zero() {
        let db = in_memory_db();
        let runner = MigrationRunner::new(db);
        assert_eq!(runner.current_version().unwrap(), 0);
    }

    #[test]
    fn test_two_migrations_advance_version() {
        let db = in_memory_db();
        let mut runner = MigrationRunner::new(db);

        runner.register(Migration::new(1, "migration-one", |_txn| Ok(())));
        runner.register(Migration::new(2, "migration-two", |_txn| Ok(())));

        let final_version = runner.run().unwrap();
        assert_eq!(final_version, 2, "version should be 2 after two migrations");
        assert_eq!(
            runner.current_version().unwrap(),
            2,
            "persisted version should be 2"
        );
    }

    #[test]
    fn test_run_is_idempotent() {
        let db = in_memory_db();
        let mut runner = MigrationRunner::new(db);

        runner.register(Migration::new(1, "migration-one", |_txn| Ok(())));
        runner.register(Migration::new(2, "migration-two", |_txn| Ok(())));

        runner.run().unwrap();
        // Running again must not change the version.
        let second_run = runner.run().unwrap();
        assert_eq!(second_run, 2, "version must stay at 2 on second run");
        assert_eq!(runner.current_version().unwrap(), 2);
    }

    #[test]
    fn test_rollback_with_down_function() {
        let db = in_memory_db();
        let mut runner = MigrationRunner::new(db);

        runner.register(
            Migration::new(1, "migration-with-rollback", |_txn| Ok(()))
                .with_down(|_txn| Ok(())),
        );

        runner.run().unwrap();
        assert_eq!(runner.current_version().unwrap(), 1);

        let after_rollback = runner.rollback().unwrap();
        assert_eq!(after_rollback, 0, "rollback should restore version to 0");
        assert_eq!(runner.current_version().unwrap(), 0);
    }

    #[test]
    fn test_rollback_without_down_returns_error() {
        let db = in_memory_db();
        let mut runner = MigrationRunner::new(db);

        runner.register(Migration::new(1, "no-down", |_txn| Ok(())));
        runner.run().unwrap();

        let result = runner.rollback();
        assert!(result.is_err(), "rollback without down fn should fail");
    }

    #[test]
    fn test_partial_run_then_complete() {
        // Simulate: first run applies v1 only, second run adds v2.
        let db1 = in_memory_db();
        let mut runner1 = MigrationRunner::new(db1);
        runner1.register(Migration::new(1, "step-one", |_txn| Ok(())));
        runner1.run().unwrap();
        assert_eq!(runner1.current_version().unwrap(), 1);

        // Re-open (simulated by reading version off same runner) and add v2.
        runner1.register(Migration::new(2, "step-two", |_txn| Ok(())));
        let v = runner1.run().unwrap();
        assert_eq!(v, 2);
    }
}
