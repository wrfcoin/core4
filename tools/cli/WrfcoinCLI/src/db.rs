use anyhow::Result;
use log::debug;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Database client for Wrfcoin CLI
pub struct DbClient {
    /// Connection to the database
    conn: Arc<Mutex<Connection>>,
    /// Path to the database file
    db_path: PathBuf,
}

/// Transaction cache entry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionCacheEntry {
    /// Transaction ID
    pub tx_id: String,
    /// Sender address
    pub from: String,
    /// Recipient address
    pub to: String,
    /// Transaction amount
    pub amount: f64,
    /// Transaction fee
    pub fee: f64,
    /// Transaction timestamp
    pub timestamp: String,
    /// Transaction type
    pub tx_type: String,
    /// Transaction status
    pub status: String,
    /// Transaction block height (if confirmed)
    pub block_height: Option<i64>,
    /// Local caching timestamp
    pub cached_at: String,
}

/// Block cache entry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockCacheEntry {
    /// Block hash
    pub hash: String,
    /// Block height
    pub height: i64,
    /// Block timestamp
    pub timestamp: String,
    /// Previous block hash
    pub prev_hash: String,
    /// Merkle root
    pub merkle_root: String,
    /// Block size
    pub size: i64,
    /// Block nonce
    pub nonce: i64,
    /// Block difficulty
    pub difficulty: i64,
    /// Number of transactions
    pub transactions_count: i64,
    /// Block type
    pub block_type: String,
    /// Local caching timestamp
    pub cached_at: String,
}

/// Wallet cache entry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletCacheEntry {
    /// Wallet address
    pub address: String,
    /// Wallet balance
    pub balance: f64,
    /// Wallet creation timestamp
    pub created_at: String,
    /// Number of transactions
    pub transactions: i64,
    /// Staking status
    pub staking: bool,
    /// Staking amount
    pub staking_amount: Option<f64>,
    /// Local caching timestamp
    pub cached_at: String,
}

/// NFT cache entry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NFTCacheEntry {
    /// NFT ID
    pub id: String,
    /// NFT name
    pub name: String,
    /// NFT description
    pub description: String,
    /// NFT owner address
    pub owner: String,
    /// NFT creation timestamp
    pub created_at: String,
    /// NFT metadata (JSON)
    pub metadata: String,
    /// NFT type
    pub nft_type: String,
    /// Computational NFT status (if applicable)
    pub computational_status: Option<String>,
    /// Local caching timestamp
    pub cached_at: String,
}

/// Command history entry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandHistoryEntry {
    /// Command ID
    pub id: i64,
    /// Command executed
    pub command: String,
    /// Command arguments
    pub args: String,
    /// Execution timestamp
    pub timestamp: String,
    /// Execution status
    pub status: String,
    /// Error message (if any)
    pub error: Option<String>,
}

/// Weather observation queue entry for offline-first support
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WeatherQueueEntry {
    /// Queue entry ID
    pub id: i64,
    /// Unique nonce to prevent replay attacks
    pub nonce: String,
    /// Station ID
    pub station_id: String,
    /// JSON payload of the observation
    pub payload: String,
    /// Cryptographic signature
    pub signature: String,
    /// When the entry was created
    pub created_at: String,
    /// Queue status: pending, synced, conflict, error
    pub status: String,
    /// Last error message (if any)
    pub last_error: Option<String>,
    /// Number of sync attempts
    pub sync_attempts: i64,
}

impl DbClient {
    /// Create a new database client
    pub fn new(db_path: PathBuf) -> Result<Self> {
        // Create directory if it doesn't exist
        if let Some(parent) = db_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
                debug!("Created directory: {}", parent.display());
            }
        }

        // Create connection to the database
        let conn = Connection::open(&db_path)?;

        // Create tables if they don't exist
        Self::init_db(&conn)?;

        debug!("Database initialized: {}", db_path.display());

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }

    /// Initialize the database
    fn init_db(conn: &Connection) -> Result<()> {
        // Create transaction cache table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS transaction_cache (
                tx_id TEXT PRIMARY KEY,
                from_address TEXT NOT NULL,
                to_address TEXT NOT NULL,
                amount REAL NOT NULL,
                fee REAL NOT NULL,
                timestamp TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                status TEXT NOT NULL,
                block_height INTEGER,
                cached_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create block cache table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS block_cache (
                hash TEXT PRIMARY KEY,
                height INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                prev_hash TEXT NOT NULL,
                merkle_root TEXT NOT NULL,
                size INTEGER NOT NULL,
                nonce INTEGER NOT NULL,
                difficulty INTEGER NOT NULL,
                transactions_count INTEGER NOT NULL,
                block_type TEXT NOT NULL,
                cached_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create wallet cache table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS wallet_cache (
                address TEXT PRIMARY KEY,
                balance REAL NOT NULL,
                created_at TEXT NOT NULL,
                transactions INTEGER NOT NULL,
                staking BOOLEAN NOT NULL,
                staking_amount REAL,
                cached_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create NFT cache table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS nft_cache (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                owner TEXT NOT NULL,
                created_at TEXT NOT NULL,
                metadata TEXT NOT NULL,
                nft_type TEXT NOT NULL,
                computational_status TEXT,
                cached_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create command history table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS command_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                command TEXT NOT NULL,
                args TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                status TEXT NOT NULL,
                error TEXT
            )",
            [],
        )?;

        // Create weather queue table for offline-first support
        conn.execute(
            "CREATE TABLE IF NOT EXISTS weather_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                nonce TEXT UNIQUE NOT NULL,
                station_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                signature TEXT NOT NULL,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                last_error TEXT,
                sync_attempts INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;

        Ok(())
    }

    /// Get the database file path
    pub fn get_db_path(&self) -> &Path {
        &self.db_path
    }

    /// Delete expired cache entries
    pub fn clear_expired_cache(&self, max_age_seconds: u64) -> Result<()> {
        let now = chrono::Utc::now();
        let cutoff = now - chrono::Duration::seconds(max_age_seconds as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let conn = self.conn.lock().unwrap();

        // Delete expired transaction cache entries
        let tx_count = conn.execute(
            "DELETE FROM transaction_cache WHERE cached_at < ?",
            params![cutoff_str],
        )?;

        // Delete expired block cache entries
        let block_count = conn.execute(
            "DELETE FROM block_cache WHERE cached_at < ?",
            params![cutoff_str],
        )?;

        // Delete expired wallet cache entries
        let wallet_count = conn.execute(
            "DELETE FROM wallet_cache WHERE cached_at < ?",
            params![cutoff_str],
        )?;

        // Delete expired NFT cache entries
        let nft_count = conn.execute(
            "DELETE FROM nft_cache WHERE cached_at < ?",
            params![cutoff_str],
        )?;

        debug!(
            "Cleared expired cache entries: {tx_count} transactions, {block_count} blocks, {wallet_count} wallets, {nft_count} NFTs"
        );

        Ok(())
    }

    /// Clear all cache entries
    pub fn clear_all_cache(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // Delete all transaction cache entries
        let tx_count = conn.execute("DELETE FROM transaction_cache", [])?;

        // Delete all block cache entries
        let block_count = conn.execute("DELETE FROM block_cache", [])?;

        // Delete all wallet cache entries
        let wallet_count = conn.execute("DELETE FROM wallet_cache", [])?;

        // Delete all NFT cache entries
        let nft_count = conn.execute("DELETE FROM nft_cache", [])?;

        debug!(
            "Cleared all cache entries: {tx_count} transactions, {block_count} blocks, {wallet_count} wallets, {nft_count} NFTs"
        );

        Ok(())
    }

    // ========== Transaction Cache ==========

    /// Cache a transaction
    pub fn cache_transaction(&self, tx: &TransactionCacheEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT OR REPLACE INTO transaction_cache (
                tx_id, from_address, to_address, amount, fee,
                timestamp, tx_type, status, block_height, cached_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                tx.tx_id,
                tx.from,
                tx.to,
                tx.amount,
                tx.fee,
                tx.timestamp,
                tx.tx_type,
                tx.status,
                tx.block_height,
                tx.cached_at
            ],
        )?;

        Ok(())
    }

    /// Get a cached transaction
    pub fn get_cached_transaction(&self, tx_id: &str) -> Result<TransactionCacheEntry> {
        let conn = self.conn.lock().unwrap();

        let tx = conn.query_row(
            "SELECT
                tx_id, from_address, to_address, amount, fee,
                timestamp, tx_type, status, block_height, cached_at
            FROM transaction_cache WHERE tx_id = ?",
            params![tx_id],
            |row| {
                Ok(TransactionCacheEntry {
                    tx_id: row.get(0)?,
                    from: row.get(1)?,
                    to: row.get(2)?,
                    amount: row.get(3)?,
                    fee: row.get(4)?,
                    timestamp: row.get(5)?,
                    tx_type: row.get(6)?,
                    status: row.get(7)?,
                    block_height: row.get(8)?,
                    cached_at: row.get(9)?,
                })
            },
        )?;

        Ok(tx)
    }

    /// Get transactions for a wallet
    pub fn get_cached_transactions_for_wallet(
        &self,
        address: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<TransactionCacheEntry>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT
                tx_id, from_address, to_address, amount, fee,
                timestamp, tx_type, status, block_height, cached_at
            FROM transaction_cache
            WHERE from_address = ? OR to_address = ?
            ORDER BY timestamp DESC
            LIMIT ? OFFSET ?",
        )?;

        let tx_iter = stmt.query_map(
            params![address, address, limit as i64, offset as i64],
            |row| {
                Ok(TransactionCacheEntry {
                    tx_id: row.get(0)?,
                    from: row.get(1)?,
                    to: row.get(2)?,
                    amount: row.get(3)?,
                    fee: row.get(4)?,
                    timestamp: row.get(5)?,
                    tx_type: row.get(6)?,
                    status: row.get(7)?,
                    block_height: row.get(8)?,
                    cached_at: row.get(9)?,
                })
            },
        )?;

        let mut transactions = Vec::new();
        for tx in tx_iter {
            transactions.push(tx?);
        }

        Ok(transactions)
    }

    /// Delete a cached transaction
    pub fn delete_cached_transaction(&self, tx_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "DELETE FROM transaction_cache WHERE tx_id = ?",
            params![tx_id],
        )?;

        Ok(())
    }

    // ========== Block Cache ==========

    /// Cache a block
    pub fn cache_block(&self, block: &BlockCacheEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT OR REPLACE INTO block_cache (
                hash, height, timestamp, prev_hash, merkle_root,
                size, nonce, difficulty, transactions_count, block_type, cached_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                block.hash,
                block.height,
                block.timestamp,
                block.prev_hash,
                block.merkle_root,
                block.size,
                block.nonce,
                block.difficulty,
                block.transactions_count,
                block.block_type,
                block.cached_at
            ],
        )?;

        Ok(())
    }

    /// Get a cached block by hash
    pub fn get_cached_block_by_hash(&self, hash: &str) -> Result<BlockCacheEntry> {
        let conn = self.conn.lock().unwrap();

        let block = conn.query_row(
            "SELECT
                hash, height, timestamp, prev_hash, merkle_root,
                size, nonce, difficulty, transactions_count, block_type, cached_at
            FROM block_cache WHERE hash = ?",
            params![hash],
            |row| {
                Ok(BlockCacheEntry {
                    hash: row.get(0)?,
                    height: row.get(1)?,
                    timestamp: row.get(2)?,
                    prev_hash: row.get(3)?,
                    merkle_root: row.get(4)?,
                    size: row.get(5)?,
                    nonce: row.get(6)?,
                    difficulty: row.get(7)?,
                    transactions_count: row.get(8)?,
                    block_type: row.get(9)?,
                    cached_at: row.get(10)?,
                })
            },
        )?;

        Ok(block)
    }

    /// Get a cached block by height
    pub fn get_cached_block_by_height(&self, height: i64) -> Result<BlockCacheEntry> {
        let conn = self.conn.lock().unwrap();

        let block = conn.query_row(
            "SELECT
                hash, height, timestamp, prev_hash, merkle_root,
                size, nonce, difficulty, transactions_count, block_type, cached_at
            FROM block_cache WHERE height = ?",
            params![height],
            |row| {
                Ok(BlockCacheEntry {
                    hash: row.get(0)?,
                    height: row.get(1)?,
                    timestamp: row.get(2)?,
                    prev_hash: row.get(3)?,
                    merkle_root: row.get(4)?,
                    size: row.get(5)?,
                    nonce: row.get(6)?,
                    difficulty: row.get(7)?,
                    transactions_count: row.get(8)?,
                    block_type: row.get(9)?,
                    cached_at: row.get(10)?,
                })
            },
        )?;

        Ok(block)
    }

    /// Get latest cached blocks
    pub fn get_latest_cached_blocks(&self, limit: usize) -> Result<Vec<BlockCacheEntry>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT
                hash, height, timestamp, prev_hash, merkle_root,
                size, nonce, difficulty, transactions_count, block_type, cached_at
            FROM block_cache
            ORDER BY height DESC
            LIMIT ?",
        )?;

        let block_iter = stmt.query_map(params![limit as i64], |row| {
            Ok(BlockCacheEntry {
                hash: row.get(0)?,
                height: row.get(1)?,
                timestamp: row.get(2)?,
                prev_hash: row.get(3)?,
                merkle_root: row.get(4)?,
                size: row.get(5)?,
                nonce: row.get(6)?,
                difficulty: row.get(7)?,
                transactions_count: row.get(8)?,
                block_type: row.get(9)?,
                cached_at: row.get(10)?,
            })
        })?;

        let mut blocks = Vec::new();
        for block in block_iter {
            blocks.push(block?);
        }

        Ok(blocks)
    }

    /// Delete a cached block
    pub fn delete_cached_block(&self, hash: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute("DELETE FROM block_cache WHERE hash = ?", params![hash])?;

        Ok(())
    }

    // ========== Wallet Cache ==========

    /// Cache a wallet
    pub fn cache_wallet(&self, wallet: &WalletCacheEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT OR REPLACE INTO wallet_cache (
                address, balance, created_at, transactions,
                staking, staking_amount, cached_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                wallet.address,
                wallet.balance,
                wallet.created_at,
                wallet.transactions,
                wallet.staking,
                wallet.staking_amount,
                wallet.cached_at
            ],
        )?;

        Ok(())
    }

    /// Get a cached wallet
    pub fn get_cached_wallet(&self, address: &str) -> Result<WalletCacheEntry> {
        let conn = self.conn.lock().unwrap();

        let wallet = conn.query_row(
            "SELECT
                address, balance, created_at, transactions,
                staking, staking_amount, cached_at
            FROM wallet_cache WHERE address = ?",
            params![address],
            |row| {
                Ok(WalletCacheEntry {
                    address: row.get(0)?,
                    balance: row.get(1)?,
                    created_at: row.get(2)?,
                    transactions: row.get(3)?,
                    staking: row.get(4)?,
                    staking_amount: row.get(5)?,
                    cached_at: row.get(6)?,
                })
            },
        )?;

        Ok(wallet)
    }

    /// Get all cached wallets
    pub fn get_all_cached_wallets(&self) -> Result<Vec<WalletCacheEntry>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT
                address, balance, created_at, transactions,
                staking, staking_amount, cached_at
            FROM wallet_cache
            ORDER BY balance DESC",
        )?;

        let wallet_iter = stmt.query_map([], |row| {
            Ok(WalletCacheEntry {
                address: row.get(0)?,
                balance: row.get(1)?,
                created_at: row.get(2)?,
                transactions: row.get(3)?,
                staking: row.get(4)?,
                staking_amount: row.get(5)?,
                cached_at: row.get(6)?,
            })
        })?;

        let mut wallets = Vec::new();
        for wallet in wallet_iter {
            wallets.push(wallet?);
        }

        Ok(wallets)
    }

    /// Delete a cached wallet
    pub fn delete_cached_wallet(&self, address: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "DELETE FROM wallet_cache WHERE address = ?",
            params![address],
        )?;

        Ok(())
    }

    // ========== NFT Cache ==========

    /// Cache an NFT
    pub fn cache_nft(&self, nft: &NFTCacheEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT OR REPLACE INTO nft_cache (
                id, name, description, owner, created_at,
                metadata, nft_type, computational_status, cached_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                nft.id,
                nft.name,
                nft.description,
                nft.owner,
                nft.created_at,
                nft.metadata,
                nft.nft_type,
                nft.computational_status,
                nft.cached_at
            ],
        )?;

        Ok(())
    }

    /// Get a cached NFT
    pub fn get_cached_nft(&self, id: &str) -> Result<NFTCacheEntry> {
        let conn = self.conn.lock().unwrap();

        let nft = conn.query_row(
            "SELECT
                id, name, description, owner, created_at,
                metadata, nft_type, computational_status, cached_at
            FROM nft_cache WHERE id = ?",
            params![id],
            |row| {
                Ok(NFTCacheEntry {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    owner: row.get(3)?,
                    created_at: row.get(4)?,
                    metadata: row.get(5)?,
                    nft_type: row.get(6)?,
                    computational_status: row.get(7)?,
                    cached_at: row.get(8)?,
                })
            },
        )?;

        Ok(nft)
    }

    /// Get NFTs by owner
    pub fn get_cached_nfts_by_owner(
        &self,
        owner: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<NFTCacheEntry>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT
                id, name, description, owner, created_at,
                metadata, nft_type, computational_status, cached_at
            FROM nft_cache
            WHERE owner = ?
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?",
        )?;

        let nft_iter = stmt.query_map(params![owner, limit as i64, offset as i64], |row| {
            Ok(NFTCacheEntry {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                owner: row.get(3)?,
                created_at: row.get(4)?,
                metadata: row.get(5)?,
                nft_type: row.get(6)?,
                computational_status: row.get(7)?,
                cached_at: row.get(8)?,
            })
        })?;

        let mut nfts = Vec::new();
        for nft in nft_iter {
            nfts.push(nft?);
        }

        Ok(nfts)
    }

    /// Delete a cached NFT
    pub fn delete_cached_nft(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute("DELETE FROM nft_cache WHERE id = ?", params![id])?;

        Ok(())
    }

    // ========== Command History ==========

    /// Add a command to history
    pub fn add_command_to_history(
        &self,
        command: &str,
        args: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();

        let timestamp = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO command_history (
                command, args, timestamp, status, error
            ) VALUES (?, ?, ?, ?, ?)",
            params![command, args, timestamp, status, error],
        )?;

        let id = conn.last_insert_rowid();

        Ok(id)
    }

    /// Get command history
    pub fn get_command_history(&self, limit: usize) -> Result<Vec<CommandHistoryEntry>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, command, args, timestamp, status, error
            FROM command_history
            ORDER BY id DESC
            LIMIT ?",
        )?;

        let history_iter = stmt.query_map(params![limit as i64], |row| {
            Ok(CommandHistoryEntry {
                id: row.get(0)?,
                command: row.get(1)?,
                args: row.get(2)?,
                timestamp: row.get(3)?,
                status: row.get(4)?,
                error: row.get(5)?,
            })
        })?;

        let mut history = Vec::new();
        for entry in history_iter {
            history.push(entry?);
        }

        Ok(history)
    }

    /// Clear command history
    pub fn clear_command_history(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute("DELETE FROM command_history", [])?;

        Ok(())
    }

    // ========== Weather Queue ==========

    /// Enqueue a weather observation for later sync
    pub fn enqueue_weather(
        &self,
        nonce: &str,
        station_id: &str,
        payload: &str,
        signature: &str,
        created_at: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO weather_queue (nonce, station_id, payload, signature, created_at, status)
             VALUES (?, ?, ?, ?, ?, 'pending')",
            params![nonce, station_id, payload, signature, created_at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get pending weather entries up to a limit
    pub fn get_pending_weather(&self, limit: usize) -> Result<Vec<WeatherQueueEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, nonce, station_id, payload, signature, created_at, status, last_error, sync_attempts
             FROM weather_queue
             WHERE status = 'pending'
             ORDER BY id ASC
             LIMIT ?",
        )?;
        let iter = stmt.query_map(params![limit as i64], |row| {
            Ok(WeatherQueueEntry {
                id: row.get(0)?,
                nonce: row.get(1)?,
                station_id: row.get(2)?,
                payload: row.get(3)?,
                signature: row.get(4)?,
                created_at: row.get(5)?,
                status: row.get(6)?,
                last_error: row.get(7)?,
                sync_attempts: row.get(8)?,
            })
        })?;
        let mut entries = Vec::new();
        for entry in iter {
            entries.push(entry?);
        }
        Ok(entries)
    }

    /// Mark a weather queue entry as synced
    pub fn mark_weather_synced(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE weather_queue SET status = 'synced' WHERE id = ?",
            params![id],
        )?;
        Ok(())
    }

    /// Mark a weather queue entry as conflict (e.g., duplicate nonce on server)
    pub fn mark_weather_conflict(&self, id: i64, reason: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE weather_queue SET status = 'conflict', last_error = ? WHERE id = ?",
            params![reason, id],
        )?;
        Ok(())
    }

    /// Mark a weather queue entry as error and increment sync_attempts
    pub fn mark_weather_error(&self, id: i64, error: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE weather_queue SET status = 'pending', last_error = ?, sync_attempts = sync_attempts + 1 WHERE id = ?",
            params![error, id],
        )?;
        Ok(())
    }

    /// Purge weather queue entries by status and/or age
    pub fn purge_weather_queue(
        &self,
        status: Option<&str>,
        older_than_days: Option<u32>,
    ) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(s) = status {
            conditions.push("status = ?");
            param_values.push(Box::new(s.to_string()));
        }
        if let Some(days) = older_than_days {
            conditions.push("created_at < datetime('now', ?)");
            param_values.push(Box::new(format!("-{days} days")));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        let sql = format!("DELETE FROM weather_queue{where_clause}");
        let params: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let count = conn.execute(&sql, params.as_slice())?;
        Ok(count as u64)
    }

    /// Get weather queue statistics
    pub fn get_weather_queue_stats(&self) -> Result<serde_json::Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT status, COUNT(*) FROM weather_queue GROUP BY status")?;
        let iter = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        let mut stats = serde_json::Map::new();
        let mut total: i64 = 0;
        for row in iter {
            let (status, count) = row?;
            total += count;
            stats.insert(status, serde_json::Value::Number(count.into()));
        }
        stats.insert("total".to_string(), serde_json::Value::Number(total.into()));

        Ok(serde_json::Value::Object(stats))
    }

    /// Get a single weather queue entry by ID
    pub fn get_weather_queue_entry(&self, id: i64) -> Result<WeatherQueueEntry> {
        let conn = self.conn.lock().unwrap();
        let entry = conn.query_row(
            "SELECT id, nonce, station_id, payload, signature, created_at, status, last_error, sync_attempts
             FROM weather_queue WHERE id = ?",
            params![id],
            |row| {
                Ok(WeatherQueueEntry {
                    id: row.get(0)?,
                    nonce: row.get(1)?,
                    station_id: row.get(2)?,
                    payload: row.get(3)?,
                    signature: row.get(4)?,
                    created_at: row.get(5)?,
                    status: row.get(6)?,
                    last_error: row.get(7)?,
                    sync_attempts: row.get(8)?,
                })
            },
        )?;
        Ok(entry)
    }
}
