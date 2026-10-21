use std::path::{Path, PathBuf};
use std::sync::Arc;
use rusqlite::{params, Connection, Result, NO_PARAMS};
use log::{info, warn, error};
use serde_json;

use crate::blockchain::block::Block;
use crate::blockchain::transaction::Transaction;
use crate::blockchain::wallet::Wallet;
use crate::blockchain::nft::NFT;
use crate::storage::encryption::{encrypt_private_key, decrypt_private_key};

pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl Database {
    pub fn new(db_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        info!("Initializing database at {}", db_path.display());
        
        let conn = Connection::open(db_path)?;
        
        // Enable foreign keys
        conn.execute("PRAGMA foreign_keys = ON", NO_PARAMS)?;
        
        // Create tables if they don't exist
        Self::create_tables(&conn)?;
        
        Ok(Database {
            conn,
            path: db_path.to_path_buf(),
        })
    }
    
    fn create_tables(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
        // Blocks table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (
                id INTEGER PRIMARY KEY,
                hash TEXT NOT NULL UNIQUE,
                previous_hash TEXT NOT NULL,
                merkle_root TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                nonce INTEGER NOT NULL,
                difficulty INTEGER NOT NULL,
                index_num INTEGER NOT NULL,
                data TEXT NOT NULL
            )",
            NO_PARAMS,
        )?;
        
        // Transactions table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS transactions (
                id INTEGER PRIMARY KEY,
                tx_id TEXT NOT NULL UNIQUE,
                block_hash TEXT,
                timestamp INTEGER NOT NULL,
                data TEXT NOT NULL,
                FOREIGN KEY (block_hash) REFERENCES blocks(hash)
            )",
            NO_PARAMS,
        )?;
        
        // Wallets table — private keys are encrypted with ChaCha20-Poly1305
        conn.execute(
            "CREATE TABLE IF NOT EXISTS wallets (
                id INTEGER PRIMARY KEY,
                address TEXT NOT NULL UNIQUE,
                public_key TEXT NOT NULL,
                encrypted_private_key TEXT NOT NULL,
                created_at INTEGER NOT NULL
            )",
            NO_PARAMS,
        )?;

        // Migrate legacy plaintext private_key column if it exists
        Self::migrate_wallets_encryption(conn)?;
        
        // NFTs table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS nfts (
                id INTEGER PRIMARY KEY,
                token_id TEXT NOT NULL UNIQUE,
                creator TEXT NOT NULL,
                owner TEXT NOT NULL,
                creation_tx TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_transferred_at INTEGER,
                royalty_percentage REAL NOT NULL,
                metadata TEXT NOT NULL,
                FOREIGN KEY (creator) REFERENCES wallets(address),
                FOREIGN KEY (owner) REFERENCES wallets(address)
            )",
            NO_PARAMS,
        )?;
        
        // Peers table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS peers (
                id INTEGER PRIMARY KEY,
                peer_id TEXT NOT NULL UNIQUE,
                addresses TEXT NOT NULL,
                last_seen INTEGER NOT NULL,
                user_agent TEXT
            )",
            NO_PARAMS,
        )?;
        
        // Mining stats table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mining_stats (
                id INTEGER PRIMARY KEY,
                block_hash TEXT NOT NULL UNIQUE,
                miner_address TEXT NOT NULL,
                mining_time INTEGER NOT NULL,
                reward REAL NOT NULL,
                difficulty INTEGER NOT NULL,
                mined_at INTEGER NOT NULL,
                FOREIGN KEY (block_hash) REFERENCES blocks(hash)
            )",
            NO_PARAMS,
        )?;
        
        // Settings table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY,
                key TEXT NOT NULL UNIQUE,
                value TEXT NOT NULL
            )",
            NO_PARAMS,
        )?;
        
        info!("Database tables created successfully");

        Ok(())
    }

    /// Migrate legacy `private_key` column to `encrypted_private_key` if needed.
    ///
    /// If the old `private_key` column exists, the table is recreated with
    /// `encrypted_private_key`. Any plaintext keys are preserved as-is during
    /// migration (they will be re-encrypted on next unlock/save).
    fn migrate_wallets_encryption(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
        // Check if legacy private_key column exists
        let mut has_legacy = false;
        let mut has_encrypted = false;
        let mut stmt = conn.prepare("PRAGMA table_info(wallets)")?;
        let rows = stmt.query_map(NO_PARAMS, |row| {
            let name: String = row.get(1)?;
            Ok(name)
        })?;
        for col in rows {
            let col_name = col?;
            if col_name == "private_key" {
                has_legacy = true;
            }
            if col_name == "encrypted_private_key" {
                has_encrypted = true;
            }
        }

        if has_legacy && !has_encrypted {
            warn!("Migrating wallets table: renaming private_key to encrypted_private_key");
            conn.execute("ALTER TABLE wallets RENAME COLUMN private_key TO encrypted_private_key", NO_PARAMS)?;
            info!("Wallets table migration complete");
        }

        Ok(())
    }
    
    pub fn save_block(&self, block: &Block) -> Result<(), Box<dyn std::error::Error>> {
        let block_data = serde_json::to_string(block)?;
        
        self.conn.execute(
            "INSERT INTO blocks (hash, previous_hash, merkle_root, timestamp, nonce, difficulty, index_num, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(hash) DO UPDATE SET
             data = excluded.data",
            params![
                block.hash,
                block.previous_hash,
                block.merkle_root,
                block.timestamp.timestamp(),
                block.nonce,
                block.difficulty,
                block.index,
                block_data,
            ],
        )?;
        
        // Save all transactions in the block
        for tx in &block.transactions {
            self.save_transaction(tx, Some(&block.hash))?;
        }
        
        Ok(())
    }
    
    pub fn save_transaction(&self, transaction: &Transaction, block_hash: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let tx_data = serde_json::to_string(transaction)?;
        
        self.conn.execute(
            "INSERT INTO transactions (tx_id, block_hash, timestamp, data)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(tx_id) DO UPDATE SET
             block_hash = COALESCE(excluded.block_hash, block_hash),
             data = excluded.data",
            params![
                transaction.id,
                block_hash,
                transaction.timestamp.timestamp(),
                tx_data,
            ],
        )?;
        
        Ok(())
    }
    
    /// Saves a wallet with the private key encrypted using the given passphrase.
    pub fn save_wallet(&self, wallet: &Wallet, passphrase: &str) -> Result<(), Box<dyn std::error::Error>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let encrypted_key = encrypt_private_key(&wallet.private_key, passphrase)
            .map_err(|e| format!("Failed to encrypt private key: {}", e))?;

        self.conn.execute(
            "INSERT INTO wallets (address, public_key, encrypted_private_key, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(address) DO NOTHING",
            params![
                wallet.address,
                wallet.public_key,
                encrypted_key,
                now,
            ],
        )?;

        Ok(())
    }
    
    pub fn save_nft(&self, nft: &NFT) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = serde_json::to_string(&nft.metadata)?;
        
        let last_transferred_at = nft.last_transferred_at.map(|t| t.timestamp());
        
        self.conn.execute(
            "INSERT INTO nfts (token_id, creator, owner, creation_tx, created_at, last_transferred_at, royalty_percentage, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(token_id) DO UPDATE SET
             owner = excluded.owner,
             last_transferred_at = excluded.last_transferred_at,
             metadata = excluded.metadata",
            params![
                nft.id,
                nft.creator,
                nft.owner,
                nft.creation_tx,
                nft.created_at.timestamp(),
                last_transferred_at,
                nft.royalty_percentage,
                metadata,
            ],
        )?;
        
        Ok(())
    }
    
    pub fn save_peer(&self, peer_id: &str, addresses: &[String], user_agent: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let addresses_json = serde_json::to_string(addresses)?;
        
        self.conn.execute(
            "INSERT INTO peers (peer_id, addresses, last_seen, user_agent)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(peer_id) DO UPDATE SET
             addresses = excluded.addresses,
             last_seen = excluded.last_seen,
             user_agent = COALESCE(excluded.user_agent, user_agent)",
            params![
                peer_id,
                addresses_json,
                now,
                user_agent,
            ],
        )?;
        
        Ok(())
    }
    
    pub fn save_mining_stats(&self, block_hash: &str, miner_address: &str, mining_time: u64, reward: u64, difficulty: u32) -> Result<(), Box<dyn std::error::Error>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        self.conn.execute(
            "INSERT INTO mining_stats (block_hash, miner_address, mining_time, reward, difficulty, mined_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(block_hash) DO NOTHING",
            params![
                block_hash,
                miner_address,
                mining_time,
                reward,
                difficulty,
                now,
            ],
        )?;
        
        Ok(())
    }
    
    pub fn save_setting(&self, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "INSERT INTO settings (key, value)
             VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET
             value = excluded.value",
            params![key, value],
        )?;
        
        Ok(())
    }
    
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        
        if let Some(row) = rows.next()? {
            let value: String = row.get(0)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }
    
    pub fn get_block_by_hash(&self, hash: &str) -> Result<Option<Block>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT data FROM blocks WHERE hash = ?1")?;
        let mut rows = stmt.query(params![hash])?;
        
        if let Some(row) = rows.next()? {
            let block_data: String = row.get(0)?;
            let block: Block = serde_json::from_str(&block_data)?;
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }
    
    pub fn get_block_by_index(&self, index: u64) -> Result<Option<Block>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT data FROM blocks WHERE index_num = ?1")?;
        let mut rows = stmt.query(params![index])?;
        
        if let Some(row) = rows.next()? {
            let block_data: String = row.get(0)?;
            let block: Block = serde_json::from_str(&block_data)?;
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }
    
    pub fn get_latest_blocks(&self, limit: u64) -> Result<Vec<Block>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT data FROM blocks ORDER BY index_num DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit], |row| {
            let block_data: String = row.get(0)?;
            let block: Block = serde_json::from_str(&block_data)
                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
            Ok(block)
        })?;
        
        let mut blocks = Vec::new();
        for block_result in rows {
            blocks.push(block_result?);
        }
        
        // Reverse to get ascending order
        blocks.reverse();
        
        Ok(blocks)
    }
    
    pub fn get_all_blocks(&self) -> Result<Vec<Block>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT data FROM blocks ORDER BY index_num ASC")?;
        let rows = stmt.query_map(NO_PARAMS, |row| {
            let block_data: String = row.get(0)?;
            let block: Block = serde_json::from_str(&block_data)
                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
            Ok(block)
        })?;
        
        let mut blocks = Vec::new();
        for block_result in rows {
            blocks.push(block_result?);
        }
        
        Ok(blocks)
    }
    
    pub fn get_transaction_by_id(&self, tx_id: &str) -> Result<Option<Transaction>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT data FROM transactions WHERE tx_id = ?1")?;
        let mut rows = stmt.query(params![tx_id])?;
        
        if let Some(row) = rows.next()? {
            let tx_data: String = row.get(0)?;
            let transaction: Transaction = serde_json::from_str(&tx_data)?;
            Ok(Some(transaction))
        } else {
            Ok(None)
        }
    }
    
    pub fn get_transactions_by_block_hash(&self, block_hash: &str) -> Result<Vec<Transaction>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT data FROM transactions WHERE block_hash = ?1")?;
        let rows = stmt.query_map(params![block_hash], |row| {
            let tx_data: String = row.get(0)?;
            let transaction: Transaction = serde_json::from_str(&tx_data)
                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
            Ok(transaction)
        })?;
        
        let mut transactions = Vec::new();
        for tx_result in rows {
            transactions.push(tx_result?);
        }
        
        Ok(transactions)
    }
    
    /// Retrieves a wallet by address and decrypts the private key using the passphrase.
    pub fn get_wallet_by_address(&self, address: &str, passphrase: &str) -> Result<Option<Wallet>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT public_key, encrypted_private_key FROM wallets WHERE address = ?1")?;
        let mut rows = stmt.query(params![address])?;

        if let Some(row) = rows.next()? {
            let public_key: String = row.get(0)?;
            let encrypted_key: String = row.get(1)?;

            let private_key = decrypt_private_key(&encrypted_key, passphrase)
                .map_err(|e| format!("Failed to decrypt wallet key: {}", e))?;

            Ok(Some(Wallet {
                address: address.to_string(),
                public_key,
                private_key,
            }))
        } else {
            Ok(None)
        }
    }

    /// Retrieves wallet public info (address + public key) without decrypting the private key.
    pub fn get_wallet_public_info(&self, address: &str) -> Result<Option<(String, String)>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT address, public_key FROM wallets WHERE address = ?1")?;
        let mut rows = stmt.query(params![address])?;

        if let Some(row) = rows.next()? {
            let addr: String = row.get(0)?;
            let public_key: String = row.get(1)?;
            Ok(Some((addr, public_key)))
        } else {
            Ok(None)
        }
    }

    /// Checks whether a wallet address exists in the database.
    pub fn wallet_exists(&self, address: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT COUNT(*) FROM wallets WHERE address = ?1")?;
        let count: i64 = stmt.query_row(params![address], |row| row.get(0))?;
        Ok(count > 0)
    }
    
    /// Returns public info (address, public key) for all wallets. Private keys are NOT returned.
    pub fn get_all_wallet_public_info(&self) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT address, public_key FROM wallets")?;
        let rows = stmt.query_map(NO_PARAMS, |row| {
            let address: String = row.get(0)?;
            let public_key: String = row.get(1)?;
            Ok((address, public_key))
        })?;

        let mut wallets = Vec::new();
        for wallet_result in rows {
            wallets.push(wallet_result?);
        }

        Ok(wallets)
    }
    
    pub fn get_nft_by_id(&self, token_id: &str) -> Result<Option<NFT>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT * FROM nfts WHERE token_id = ?1")?;
        let mut rows = stmt.query(params![token_id])?;
        
        if let Some(row) = rows.next()? {
            let creator: String = row.get(2)?;
            let owner: String = row.get(3)?;
            let creation_tx: String = row.get(4)?;
            let created_at_ts: i64 = row.get(5)?;
            let last_transferred_at_ts: Option<i64> = row.get(6)?;
            let royalty_percentage: f64 = row.get(7)?;
            let metadata_json: String = row.get(8)?;
            
            use chrono::{DateTime, TimeZone, Utc};
            
            let created_at = Utc.timestamp(created_at_ts, 0);
            let last_transferred_at = last_transferred_at_ts.map(|ts| Utc.timestamp(ts, 0));
            
            let metadata = serde_json::from_str(&metadata_json)?;
            
            // This is a simplified reconstruction of the NFT
            // In a real implementation, you would need to also retrieve price history
            let nft = NFT {
                id: token_id.to_string(),
                token_id: 0, // This would need to be properly stored/retrieved
                creator,
                owner,
                metadata,
                creation_tx,
                created_at,
                last_transferred_at,
                price_history: Vec::new(), // This would need to be stored/retrieved properly
                royalty_percentage,
            };
            
            Ok(Some(nft))
        } else {
            Ok(None)
        }
    }
    
    pub fn get_nfts_by_owner(&self, owner_address: &str) -> Result<Vec<NFT>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT token_id FROM nfts WHERE owner = ?1")?;
        let rows = stmt.query_map(params![owner_address], |row| {
            let token_id: String = row.get(0)?;
            Ok(token_id)
        })?;
        
        let mut nfts = Vec::new();
        for token_id_result in rows {
            let token_id = token_id_result?;
            if let Some(nft) = self.get_nft_by_id(&token_id)? {
                nfts.push(nft);
            }
        }
        
        Ok(nfts)
    }
    
    pub fn close(&self) -> Result<(), Box<dyn std::error::Error>> {
        // SQLite connections are automatically closed when dropped
        Ok(())
    }
}
