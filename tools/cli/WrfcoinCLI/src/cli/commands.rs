use crate::commands::wrf_amount_to_units;
use crate::crypto::KeyManager;
use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
#[allow(unused_imports)]
use log::{error, info};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use wrfcoin_cli::errors::{CliError, CliErrorCode};
use wrfcoin_cli::{ApiClient, ApiConfig};
use wrfcoin_shared_protocol::blockchain::transaction::TransparentTransferPayload;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Execution context for CLI formatting flags
#[derive(Clone, Copy)]
pub struct ExecutionContext {
    pub json: bool,
    pub quiet: bool,
    pub why: bool,
    pub output_format: wrfcoin_cli::output::OutputFormat,
    #[allow(dead_code)]
    pub non_interactive: bool,
}

impl ExecutionContext {
    pub fn new(
        json: bool,
        quiet: bool,
        why: bool,
        output_format: wrfcoin_cli::output::OutputFormat,
        non_interactive: bool,
    ) -> Self {
        Self {
            json,
            quiet,
            why,
            output_format,
            non_interactive,
        }
    }
}
// Transaction definition (to avoid import issues)
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Transaction {
    pub id: String,
    pub block_height: Option<u64>,
    pub timestamp: String,
    pub from: String,
    pub to: String,
    pub amount: f64,
    pub fee: f64,
    pub status: String,
    pub tx_type: String,
    pub hash: String,
}

// Temporary NodeCommands placeholder - this would normally be imported
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum NodeCommands {
    Info,
}

// Temporary Database trait stub
#[allow(dead_code)]
pub trait Database {
    // Placeholder methods would go here
}

// Temporary Block struct stub
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Block {
    pub hash: String,
    pub height: u64,
    pub timestamp: String,
    pub previous_hash: String,
    pub block_type: String,
    pub transactions: Vec<Transaction>,
    pub miner: String,
    pub reward: f64,
    pub difficulty: f64,
    pub nonce: u64,
    pub merkle_root: String,
    pub size: u64,
}

/// Temporary Blockchain struct stub
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Blockchain {
    pub tip: String,
    pub height: u64,
    pub difficulty: f64,
    pub blocks: Vec<Block>,
    pub target_block_time: u64,
    pub chain_id: String,
    pub pending_transactions: Vec<Transaction>,
}

#[allow(dead_code)]
impl Blockchain {
    /// Get latest block height
    pub fn get_latest_block_height(&self) -> u64 {
        self.height
    }

    /// Get total transaction count
    pub fn get_total_transactions(&self) -> u64 {
        0 // Placeholder
    }

    /// Get current difficulty
    pub fn get_difficulty(&self) -> f64 {
        self.difficulty
    }

    /// Get block by hash
    pub fn get_block(&self, _hash: &str) -> Option<Block> {
        None // Placeholder
    }

    /// Get block by height
    pub fn get_block_by_height(&self, _height: u64) -> Option<Block> {
        None // Placeholder
    }

    /// Get latest blocks with limit
    pub fn get_latest_blocks(&self, _limit: u64) -> Vec<Block> {
        vec![] // Placeholder
    }

    /// Get pending transactions
    pub fn get_pending_transactions(&self) -> Vec<Transaction> {
        self.pending_transactions.clone()
    }
}

/// Temporary ComputationalNftManager stub
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ComputationalNftManager {
    // Placeholder fields would go here
}

#[allow(dead_code)]
impl ComputationalNftManager {
    /// List all computational NFTs
    pub fn list_computational_nfts(&self) -> Vec<String> {
        vec![] // Placeholder
    }

    /// Get computational NFT by ID
    pub fn get_computational_nft(&self, _id: &str) -> Option<String> {
        None // Placeholder
    }

    /// Create a new computational NFT
    pub fn create_computational_nft(
        &mut self,
        _creator: &str,
        _title: &str,
        _description: &str,
        _computation_type: &str,
        _input_data: &str,
        _reward: u64,
    ) -> anyhow::Result<String> {
        Ok("nft_placeholder".to_string())
    }

    /// Submit computation result
    pub fn submit_computation_result(
        &mut self,
        _id: &str,
        _executor: &str,
    ) -> anyhow::Result<String> {
        Ok("tx_placeholder".to_string())
    }

    /// Get available categories
    pub fn get_categories(&self) -> Vec<String> {
        vec![] // Placeholder
    }

    /// Add a new category
    pub fn add_category(&mut self, _category: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct ManagedAddress {
    address: String,
    key_path: String,
    created_at: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct WalletState {
    version: String,
    addresses: Vec<ManagedAddress>,
    primary_address: Option<String>,
    balances: HashMap<String, u64>,
    transaction_history: Vec<Transaction>,
    legacy_addresses: Vec<String>,
}

impl Default for WalletState {
    fn default() -> Self {
        Self {
            version: "2".to_string(),
            addresses: Vec::new(),
            primary_address: None,
            balances: HashMap::new(),
            transaction_history: Vec::new(),
            legacy_addresses: Vec::new(),
        }
    }
}

fn malformed_key_file_warning(path: &Path, error: &anyhow::Error) -> String {
    format!(
        "warning: skipping malformed wallet key file {}: {error}",
        path.display()
    )
}

/// #2735: the v2 wallet-state shape as written before the #505 f64 -> u64
/// balance migration — structured addresses, WRF-denominated float balances.
#[derive(Clone, Debug, serde::Deserialize)]
struct WalletStateV2Float {
    #[allow(dead_code)]
    version: String,
    addresses: Vec<ManagedAddress>,
    #[serde(default)]
    primary_address: Option<String>,
    #[serde(default)]
    balances: HashMap<String, f64>,
    #[serde(default)]
    transaction_history: Vec<Transaction>,
    #[serde(default)]
    legacy_addresses: Vec<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct LegacyWalletState {
    addresses: Option<Vec<String>>,
    primary_address: Option<String>,
    balances: Option<HashMap<String, f64>>,
    transaction_history: Option<Vec<Transaction>>,
}

fn legacy_balances_to_units(balances: HashMap<String, f64>) -> Result<HashMap<String, u64>> {
    balances
        .into_iter()
        .map(|(address, balance)| Ok((address, wrf_amount_to_units(balance)?)))
        .collect()
}

/// Persistent wallet manager for CLI operations.
#[derive(Clone, Debug)]
pub struct Wallet {
    base_dir: PathBuf,
    state_path: PathBuf,
    state: WalletState,
}

impl Wallet {
    /// #2735: this replaces the old `impl Default`, which swallowed the init
    /// error (logging it, then continuing with a fallback wallet rooted at
    /// `.`). That fallback then wrote REAL key material into `./keys/` of
    /// whatever cwd the command ran from and finally failed persisting a
    /// parentless relative `wallet-state.json` — surfacing as the bare,
    /// misleading "No such file or directory (os error 2)" after the real
    /// error had already scrolled by. Init failures now propagate, carrying
    /// the failing file's path.
    pub fn try_default() -> Result<Self> {
        Self::load_or_initialize()
    }

    fn load_or_initialize() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Home directory not found"))?;
        Self::load_or_initialize_at(home.join(".wrfcoin"))
    }

    /// Base-dir-parameterized init, so tests can drive it against a scratch
    /// directory instead of the process home (#2735).
    fn load_or_initialize_at(base_dir: PathBuf) -> Result<Self> {
        let wallet_dir = base_dir.join("wallet");
        if !wallet_dir.exists() {
            fs::create_dir_all(&wallet_dir)?;
        }
        set_dir_permissions_owner_only(&wallet_dir)?;

        let state_path = wallet_dir.join("wallet-state.json");
        let mut state = if state_path.exists() {
            Self::read_state(&state_path)?
        } else {
            WalletState::default()
        };

        Self::migrate_legacy_wallet(&base_dir, &mut state)?;

        let mut wallet = Self {
            base_dir,
            state_path,
            state,
        };
        wallet.sync_with_key_store()?;
        if wallet.state.addresses.is_empty() {
            let _ = wallet.generate_new_address()?;
        }
        wallet.persist_state()?;
        Ok(wallet)
    }

    fn key_manager(&self) -> Result<KeyManager> {
        KeyManager::new(&self.base_dir)
    }

    fn read_state(path: &Path) -> Result<WalletState> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read wallet state file {}", path.display()))?;
        // #2735: keep the PRIMARY parse error. The old code discarded it and
        // surfaced the legacy fallback's error instead, which is how a float-
        // balance issue got reported as "invalid type: map, expected a string
        // at line 4 column 4" with no file name.
        let primary_err = match serde_json::from_str::<WalletState>(&raw) {
            Ok(state) => return Ok(state),
            Err(err) => err,
        };
        // #2735: v2-with-float-balances — written by CLI builds that already
        // used structured `addresses` but predate the #505 f64 -> u64 balance
        // migration (observed in the wild: Feb 2026). Neither the current nor
        // the string-address legacy parser accepts it, which made every
        // wallet subcommand unusable against such a store.
        if let Ok(v2f) = serde_json::from_str::<WalletStateV2Float>(&raw) {
            let balances = legacy_balances_to_units(v2f.balances).with_context(|| {
                format!(
                    "failed to convert float balances to base units in {}",
                    path.display()
                )
            })?;
            return Ok(WalletState {
                version: "2".to_string(),
                addresses: v2f.addresses,
                primary_address: v2f.primary_address,
                balances,
                transaction_history: v2f.transaction_history,
                legacy_addresses: v2f.legacy_addresses,
            });
        }
        if let Ok(legacy) = serde_json::from_str::<LegacyWalletState>(&raw) {
            let balances = legacy
                .balances
                .map(legacy_balances_to_units)
                .transpose()
                .with_context(|| {
                    format!(
                        "failed to convert legacy balances to base units in {}",
                        path.display()
                    )
                })?
                .unwrap_or_default();
            return Ok(WalletState {
                version: "2".to_string(),
                addresses: Vec::new(),
                primary_address: legacy.primary_address,
                balances,
                transaction_history: legacy.transaction_history.unwrap_or_default(),
                legacy_addresses: legacy.addresses.unwrap_or_default(),
            });
        }
        Err(anyhow!(
            "failed to parse wallet state file {}: {primary_err} (the file is also not \
             readable as a v2-float or legacy wallet state)",
            path.display()
        ))
    }

    fn migrate_legacy_wallet(base_dir: &Path, state: &mut WalletState) -> Result<()> {
        let legacy_path = base_dir.join("wallet.json");
        if !legacy_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&legacy_path).with_context(|| {
            format!(
                "failed to read legacy wallet file {}",
                legacy_path.display()
            )
        })?;
        let legacy: LegacyWalletState = serde_json::from_str(&raw).with_context(|| {
            format!(
                "failed to parse legacy wallet file {}",
                legacy_path.display()
            )
        })?;

        if let Some(addresses) = legacy.addresses {
            for addr in addresses {
                if !state.legacy_addresses.contains(&addr) {
                    state.legacy_addresses.push(addr);
                }
            }
        }
        if state.primary_address.is_none() {
            state.primary_address = legacy.primary_address;
        }
        if state.balances.is_empty() {
            state.balances = legacy
                .balances
                .map(legacy_balances_to_units)
                .transpose()?
                .unwrap_or_default();
        }
        if state.transaction_history.is_empty() {
            state.transaction_history = legacy.transaction_history.unwrap_or_default();
        }
        Ok(())
    }

    fn sync_with_key_store(&mut self) -> Result<()> {
        let manager = self.key_manager()?;
        let mut key_pairs = manager.list_key_pairs()?;
        key_pairs.sort_by(|(_, left), (_, right)| left.cmp(right));

        // A migrated wallet intentionally retains its legacy key file so the
        // backup index is usable. Group files by the address derived from the
        // key material, then prefer the canonical filename. This makes index
        // reconstruction independent of read_dir order and collapses a
        // restored legacy index back to one canonical managed address.
        let mut groups: BTreeMap<String, Vec<(String, PathBuf)>> = BTreeMap::new();
        for (address, path) in key_pairs {
            let key = match crate::crypto::WrfKeyPair::load_from_file(&path) {
                Ok(key) => key,
                Err(error) => {
                    eprintln!("{}", malformed_key_file_warning(&path, &error));
                    continue;
                },
            };
            let canonical = match key.canonical_address() {
                Ok(canonical) => canonical,
                Err(error) => {
                    eprintln!("{}", malformed_key_file_warning(&path, &error));
                    continue;
                },
            };
            groups.entry(canonical).or_default().push((address, path));
        }

        for (canonical, candidates) in groups {
            let (selected_address, selected_path) = candidates
                .iter()
                .find(|(address, _)| address == &canonical)
                .unwrap_or(&candidates[0]);
            let aliases: HashSet<&str> = candidates
                .iter()
                .map(|(address, _)| address.as_str())
                .chain(std::iter::once(canonical.as_str()))
                .collect();
            let first_index = self
                .state
                .addresses
                .iter()
                .position(|entry| aliases.contains(entry.address.as_str()));
            let created_at = first_index
                .map(|index| self.state.addresses[index].created_at.clone())
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
            let was_primary = self
                .state
                .primary_address
                .as_deref()
                .is_some_and(|address| aliases.contains(address));
            self.state
                .addresses
                .retain(|entry| !aliases.contains(entry.address.as_str()));

            let managed = ManagedAddress {
                address: selected_address.clone(),
                key_path: selected_path.display().to_string(),
                created_at,
            };
            let insertion_index = first_index
                .unwrap_or(self.state.addresses.len())
                .min(self.state.addresses.len());
            self.state.addresses.insert(insertion_index, managed);

            let balance = aliases
                .iter()
                .filter_map(|address| self.state.balances.remove(*address))
                .max()
                .unwrap_or(0);
            self.state
                .balances
                .insert(selected_address.clone(), balance);
            if was_primary {
                self.state.primary_address = Some(selected_address.clone());
            }
        }

        if self.state.primary_address.is_none() && !self.state.addresses.is_empty() {
            self.state.primary_address = Some(self.state.addresses[0].address.clone());
        }
        Ok(())
    }

    fn persist_state(&self) -> Result<()> {
        let payload = serde_json::to_string_pretty(&self.state)?;
        write_secure_file_atomic(&self.state_path, payload.as_bytes())
    }

    /// Generate a new address backed by real persisted key material.
    pub fn generate_new_address(&mut self) -> Result<String> {
        let manager = self.key_manager()?;
        let key_pair = manager.generate_key_pair(None)?;
        let address = key_pair.address.clone();
        self.register_address(&address)?;
        Ok(address)
    }

    /// Generate a new address with BIP-39 mnemonic. Returns `(address, mnemonic)`.
    pub fn generate_new_address_with_mnemonic(&mut self) -> Result<(String, String)> {
        let manager = self.key_manager()?;
        let (key_pair, mnemonic) = manager.generate_key_pair_with_mnemonic(None)?;
        let address = key_pair.address.clone();
        self.register_address(&address)?;
        Ok((address, mnemonic))
    }

    /// Recover a wallet address from a BIP-39 mnemonic phrase.
    pub fn recover_from_mnemonic(&mut self, mnemonic: &str) -> Result<String> {
        let manager = self.key_manager()?;
        let key_pair = manager.recover_key_pair(mnemonic, None)?;
        let address = key_pair.address.clone();
        self.register_address(&address)?;
        Ok(address)
    }

    fn register_address(&mut self, address: &str) -> Result<()> {
        let key_path = self.base_dir.join("keys").join(format!("{}.json", address));
        if !self
            .state
            .addresses
            .iter()
            .any(|entry| entry.address == address)
        {
            self.state.addresses.push(ManagedAddress {
                address: address.to_string(),
                key_path: key_path.display().to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
            });
        }
        self.state
            .balances
            .entry(address.to_string())
            .or_insert(0u64);
        if self.state.primary_address.is_none() {
            self.state.primary_address = Some(address.to_string());
        }
        self.persist_state()?;
        Ok(())
    }

    /// Get all wallet addresses.
    pub fn get_addresses(&self) -> Vec<String> {
        self.state
            .addresses
            .iter()
            .map(|entry| entry.address.clone())
            .collect()
    }

    /// Get the primary address.
    pub fn get_primary_address(&self) -> Option<String> {
        self.state.primary_address.clone()
    }

    /// Return the explicit legacy-to-chain address plan and, when requested,
    /// apply it only after preserving the old wallet index.
    pub fn migrate_addresses(
        &mut self,
        dry_run: bool,
    ) -> Result<(Vec<(String, String)>, Option<String>)> {
        let mut mappings = Vec::new();
        for entry in &self.state.addresses {
            let key = crate::crypto::WrfKeyPair::load_from_file(Path::new(&entry.key_path))?;
            let canonical = key.canonical_address()?;
            if entry.address != canonical {
                mappings.push((entry.address.clone(), canonical));
            }
        }
        if dry_run || mappings.is_empty() {
            return Ok((mappings, None));
        }

        let backup = self.state_path.with_file_name(format!(
            "wallet-state.json.backup-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
        ));
        fs::copy(&self.state_path, &backup)
            .with_context(|| format!("failed to back up wallet index to {}", backup.display()))?;
        set_file_permissions_owner_only(&backup)?;

        for entry in &mut self.state.addresses {
            let old_address = entry.address.clone();
            let old_path = PathBuf::from(&entry.key_path);
            let mut key = crate::crypto::WrfKeyPair::load_from_file(&old_path)?;
            let canonical = key.canonical_address()?;
            if old_address == canonical {
                continue;
            }
            key.set_canonical_address()?;
            let new_path = old_path.with_file_name(format!("{canonical}.json"));
            key.save_to_file(&new_path)?;
            // Keep the legacy key file alongside the timestamped old index.
            // If a later write fails, that index must still point at usable
            // key material and the migration remains recoverable.
            entry.address = canonical.clone();
            entry.key_path = new_path.display().to_string();
            if self.state.primary_address.as_deref() == Some(&old_address) {
                self.state.primary_address = Some(canonical.clone());
            }
            if let Some(value) = self.state.balances.remove(&old_address) {
                let canonical_balance = self.state.balances.entry(canonical).or_insert(0);
                *canonical_balance = (*canonical_balance).max(value);
            }
        }
        let mut seen = HashSet::new();
        self.state
            .addresses
            .retain(|entry| seen.insert(entry.address.clone()));
        self.persist_state()?;
        Ok((mappings, Some(backup.display().to_string())))
    }

    /// Get balance for a known wallet address.
    pub fn get_balance(&self, address: &str) -> Result<u64> {
        if !self.owns_address(address) {
            return Err(anyhow!("Address not found in wallet: {}", address));
        }
        Ok(*self.state.balances.get(address).unwrap_or(&0u64))
    }

    /// Set the primary address.
    pub fn set_primary_address(&mut self, address: &str) -> Result<()> {
        if !self.owns_address(address) {
            return Err(anyhow!("Address not found in wallet: {}", address));
        }
        self.state.primary_address = Some(address.to_string());
        self.persist_state()?;
        Ok(())
    }

    /// Build a chain-valid, client-authorized transparent-transfer payload.
    pub fn sign_transparent_transfer(
        &self,
        to: &str,
        amount: u64,
        nonce: u64,
    ) -> Result<(String, String, String)> {
        let from = self.primary_signing_address()?;
        let key_pair = self.key_manager()?.load_key_pair(&from)?;
        let payload = TransparentTransferPayload {
            from: from.clone(),
            to: to.to_string(),
            amount,
            transparent_opt_out: true,
            opt_out_audit_tag: None,
            auth_pubkey: None,
            auth_signature: None,
            authority: None,
            authority_signature: None,
            passkey_auth: None,
            sponsorship: None,
            session_key_auth: None,
        };
        let signature = key_pair.sign_hex(&payload.authorization_message(nonce))?;
        Ok((from, key_pair.public_key_hex()?, signature))
    }

    /// Sign `message` with the primary address's phrase key. Returns
    /// `(address, ed25519 public key hex, signature hex)`. Used for
    /// security-key actions (`security_key_spend`); the key never leaves
    /// this process.
    pub fn sign_with_primary(&self, message: &[u8]) -> Result<(String, String, String)> {
        let from = self.primary_signing_address()?;
        let key_pair = self.key_manager()?.load_key_pair(&from)?;
        let signature = key_pair.sign_hex(message)?;
        Ok((from, key_pair.public_key_hex()?, signature))
    }

    /// Validate the primary key before any node request. This ensures a
    /// legacy wallet receives the migration hint instead of an unrelated
    /// address-route 400 response.
    pub(crate) fn primary_signing_address(&self) -> Result<String> {
        let from = self
            .state
            .primary_address
            .clone()
            .ok_or_else(|| anyhow!("No primary address set"))?;
        if !self.owns_address(&from) {
            return Err(anyhow!("Primary address not managed by local wallet"));
        }
        let key_pair = self.key_manager()?.load_key_pair(&from)?;
        let canonical = key_pair.canonical_address()?;
        if from != canonical || key_pair.address != canonical {
            return Err(anyhow!(
                "Legacy wallet address {from} is receive-only and cannot sign; run `wrfcoin wallet migrate-addresses --dry-run`, then confirm with `--yes`"
            ));
        }
        Ok(from)
    }

    /// Get transaction history.
    pub fn get_transaction_history(&self) -> Vec<Transaction> {
        self.state.transaction_history.clone()
    }

    /// Check if wallet owns an address.
    pub fn owns_address(&self, address: &str) -> bool {
        self.state
            .addresses
            .iter()
            .any(|entry| entry.address == address)
    }
}

fn write_secure_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
        set_dir_permissions_owner_only(parent)?;
    }
    fs::write(path, bytes)?;
    set_file_permissions_owner_only(path)?;
    Ok(())
}

fn write_secure_file_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        set_dir_permissions_owner_only(parent)?;
    }
    let temp_path = path.with_extension(format!("tmp-{}", std::process::id()));
    write_secure_file(&temp_path, bytes)?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error).with_context(|| {
            format!(
                "failed to atomically replace wallet state {}",
                path.display()
            )
        });
    }
    Ok(())
}

fn set_dir_permissions_owner_only(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn set_file_permissions_owner_only(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

/// Wallet CLI commands
#[derive(Subcommand)]
pub enum WalletCommands {
    /// Generate a new address
    NewAddress,

    /// Recover wallet from BIP-39 mnemonic phrase
    Recover {
        /// BIP-39 mnemonic phrase (24 words, quoted)
        mnemonic: String,
    },

    /// List all addresses in the wallet
    ListAddresses,

    /// Show wallet balance
    Balance {
        /// Address to check (default: primary address)
        #[clap(short, long)]
        address: Option<String>,
    },

    /// Set primary address
    SetPrimary {
        /// Address to set as primary
        address: String,
    },

    /// Send tokens to another address
    Send {
        /// Recipient address
        #[clap(short, long)]
        to: String,

        /// Amount to send
        #[clap(short, long)]
        amount: f64,

        /// Account nonce (required until the node exposes it on the address route)
        #[clap(long)]
        nonce: Option<u64>,
    },

    /// Explicitly migrate legacy Wrf addresses to chain-canonical addresses
    MigrateAddresses {
        #[clap(long)]
        dry_run: bool,
        #[clap(long)]
        yes: bool,
    },

    /// Show transaction history
    History,
}

/// Blockchain CLI commands
#[derive(Subcommand)]
pub enum ChainCommands {
    /// Show information about the blockchain
    Info,

    /// Show a specific block by hash or height
    Block {
        /// Block hash (if not specified, height will be used)
        #[clap(short, long)]
        hash: Option<String>,

        /// Block height (default: latest block)
        #[clap(short, long)]
        height: Option<u64>,
    },

    /// List the latest blocks
    ListBlocks {
        /// Number of blocks to show
        #[clap(short, long, default_value = "10")]
        limit: usize,
    },

    /// Show pending transactions
    PendingTransactions,

    /// Export blockchain data (for backup)
    Export {
        /// Output file path
        #[clap(short, long)]
        output: String,
    },

    /// Import blockchain data
    Import {
        /// Input file path
        #[clap(short, long)]
        input: String,
    },
}

/// NFT CLI commands
#[derive(Subcommand)]
pub enum NftCommands {
    /// List all computational NFTs
    List,

    /// Show details for a specific NFT
    Show {
        /// NFT ID
        id: String,
    },

    /// Create a new computational NFT
    Create {
        /// Creator's address (default: primary address)
        #[clap(short, long)]
        creator: Option<String>,

        /// NFT title
        #[clap(short, long)]
        title: String,

        /// NFT description
        #[clap(short, long)]
        description: String,

        /// Computation type
        #[clap(short, long)]
        computation_type: String,

        /// Input data for computation
        #[clap(short, long)]
        input_data: String,

        /// Reward amount
        #[clap(short, long)]
        reward: u64,
    },

    /// Submit a computation result
    Submit {
        /// NFT ID
        #[clap(short, long)]
        id: String,

        /// Executor's address (default: primary address)
        #[clap(short, long)]
        executor: Option<String>,

        /// Computation result
        #[clap(short, long)]
        result: String,
    },

    /// List available computation categories
    Categories,

    /// Add a new computation category
    AddCategory {
        /// Category name
        name: String,
    },
}

/// Handles wallet commands. Returns structured JSON data for the emit layer.
pub async fn handle_wallet_command(
    command: &WalletCommands,
    wallet: Arc<RwLock<Wallet>>,
) -> Result<Value> {
    match command {
        WalletCommands::NewAddress => {
            let mut wallet = wallet.write().await;
            let (address, mnemonic) = wallet.generate_new_address_with_mnemonic()?;
            Ok(json!({
                "summary": "Generated new wallet address with BIP-39 mnemonic",
                "address": address,
                "mnemonic": mnemonic,
            }))
        },
        WalletCommands::Recover { mnemonic } => {
            let mut wallet = wallet.write().await;
            let address = wallet.recover_from_mnemonic(mnemonic)?;
            Ok(json!({
                "summary": "Recovered wallet from mnemonic",
                "address": address,
            }))
        },
        WalletCommands::ListAddresses => {
            let wallet = wallet.read().await;
            let addresses = wallet.get_addresses();
            let entries: Vec<Value> = addresses
                .iter()
                .map(|addr| {
                    json!({
                        "address": addr,
                        "primary": wallet.get_primary_address() == Some(addr.clone()),
                    })
                })
                .collect();
            Ok(json!({
                "summary": if addresses.is_empty() { "No wallet addresses configured" } else { "Wallet addresses retrieved" },
                "addresses": entries,
            }))
        },
        WalletCommands::Balance { address } => {
            let wallet = wallet.read().await;
            let addr = match address {
                Some(a) => a.clone(),
                None => wallet
                    .get_primary_address()
                    .ok_or_else(|| anyhow!("No primary address configured"))?,
            };
            if !wallet.owns_address(&addr) {
                return Err(anyhow!("Address not found in wallet: {addr}"));
            }
            drop(wallet);
            let account = ApiClient::new(ApiConfig::default())?
                .get_address(&addr)
                .await?;
            Ok(json!({
                "summary": format!("On-chain balance for {}: {} base units", addr, account.balance),
                "address": addr,
                "balance": account.balance,
                "spendable_balance": account.spendable_balance,
                "stake_locked": account.stake_locked,
                "earned_balance_total": account.earned_balance_total,
                "nonce": account.nonce,
            }))
        },
        WalletCommands::SetPrimary { address } => {
            let mut wallet = wallet.write().await;
            wallet.set_primary_address(address)?;
            Ok(json!({
                "summary": "Primary wallet address updated",
                "address": address,
            }))
        },
        WalletCommands::Send { to, amount, nonce } => {
            let wallet_guard = wallet.read().await;
            let from = wallet_guard.primary_signing_address()?;
            let amount_units = wrf_amount_to_units(*amount)?;
            drop(wallet_guard);
            let client = ApiClient::new(ApiConfig::default())?;
            let account = client.get_address(&from).await?;
            let nonce = match (account.nonce.as_deref(), nonce) {
                (Some(raw), _) => raw
                    .parse::<u64>()
                    .context("node returned an invalid account nonce")?,
                (None, Some(value)) => *value,
                (None, None) => {
                    return Err(anyhow!(
                        "The node does not expose an account nonce yet; pass `--nonce <u64>` explicitly"
                    ));
                },
            };
            let (from, auth_pubkey, auth_signature) = wallet
                .read()
                .await
                .sign_transparent_transfer(to, amount_units, nonce)?;
            // Burned transfer fee: declared when the node reports one in
            // force; otherwise omitted, and the node declares its own.
            let fee = client.get_transfer_fee().await;
            let fee_units = fee.as_ref().ok().copied().flatten();
            // `security_key_spend`: a protected account also needs a touch +
            // PIN on its registered key (nothing happens for other accounts).
            let security_key_auth = crate::commands::security_key::spend_assertion_if_required(
                &client, &from, to, amount_units, nonce,
            )
            .await?;
            let accepted = client
                .submit_transparent_transfer_with_security_key(
                    &from,
                    to,
                    amount_units,
                    nonce,
                    &auth_pubkey,
                    &auth_signature,
                    fee_units,
                    security_key_auth.as_ref(),
                )
                .await?;
            Ok(json!({
                "summary": format!(
                    "Submitted {} WRF to {} (tx_id {}) — not yet confirmed",
                    wrfcoin_cli::output::format_wrf_units(amount_units),
                    to,
                    accepted.tx_id,
                ),
                "from": from,
                "to": to,
                "amount": wrfcoin_cli::output::format_wrf_units(amount_units),
                "network_fee": crate::commands::send_interactive::network_fee_label(&fee),
                "status": "submitted",
                "tx_id": accepted.tx_id,
            }))
        },
        WalletCommands::History { .. } => {
            let wallet = wallet.read().await;
            let address = wallet
                .get_primary_address()
                .ok_or_else(|| anyhow!("No primary address configured"))?;
            drop(wallet);
            let history = ApiClient::new(ApiConfig::default())?
                .get_address_transactions(&address)
                .await
                .map_err(|err| {
                    CliError::new(
                        CliErrorCode::NotFound,
                        format!("On-chain wallet history is not available from this node: {err}"),
                    )
                })?;
            Ok(json!({
                "summary": "On-chain transaction history retrieved",
                "address": address,
                "history": history,
            }))
        },
        WalletCommands::MigrateAddresses { dry_run, yes } => {
            if !dry_run && !yes {
                return Err(anyhow!(
                    "Migration rewrites the wallet index; inspect `--dry-run` then repeat with `--yes`"
                ));
            }
            let mut wallet = wallet.write().await;
            let (mappings, backup) = wallet.migrate_addresses(*dry_run)?;
            Ok(json!({
                "summary": if *dry_run { "Address migration dry run; no files changed" } else { "Wallet addresses migrated" },
                "dry_run": dry_run,
                "mappings": mappings.into_iter().map(|(legacy, chain)| json!({"legacy": legacy, "chain": chain})).collect::<Vec<_>>(),
                "backup": backup,
            }))
        },
    }
}

/// Handles blockchain commands
#[allow(dead_code)]
pub async fn handle_chain_command(
    command: &ChainCommands,
    blockchain: Arc<RwLock<Blockchain>>,
    _db: Arc<dyn Database + Send + Sync>,
) -> Result<()> {
    match command {
        ChainCommands::Info => {
            let chain = blockchain.read().await;

            if let Some(tip_block) = chain.get_block(&chain.tip) {
                info!("Blockchain information:");
                info!("  Network: Wrfcoin");
                info!("  Height: {}", chain.height);
                info!("  Tip: {}", chain.tip);
                info!("  Difficulty: {}", chain.difficulty);
                info!("  Block count: {}", chain.blocks.len());
                info!("  Block time target: {} seconds", chain.target_block_time);
                info!("  Chain ID: {}", chain.chain_id);
                info!(
                    "  Pending transactions: {}",
                    chain.pending_transactions.len()
                );

                // Latest block info
                info!("Latest block:");
                info!("  Hash: {}", tip_block.hash);
                info!("  Height: {}", tip_block.height);
                info!("  Type: {:?}", tip_block.block_type);
                info!("  Timestamp: {}", tip_block.timestamp);
                info!("  Transactions: {}", tip_block.transactions.len());
                info!("  Miner: {}", tip_block.miner);
                info!("  Reward: {} WRF", tip_block.reward);
            } else {
                return Err(CliError::new(
                    CliErrorCode::BlockchainError,
                    "Failed to get tip block",
                )
                .into());
            }
        },
        ChainCommands::Block { hash, height } => {
            let chain = blockchain.read().await;

            let block = match (hash, height) {
                (Some(h), _) => Some(
                    chain
                        .get_block(h)
                        .ok_or_else(|| CliError::new(CliErrorCode::NotFound, "Block not found"))?,
                ),
                (None, Some(h)) => Some(
                    chain
                        .get_block_by_height(*h)
                        .ok_or_else(|| CliError::new(CliErrorCode::NotFound, "Block not found"))?,
                ),
                (None, None) => {
                    // Get the latest block
                    chain.get_block(&chain.tip)
                },
            };

            match block {
                Some(block) => {
                    info!("Block information:");
                    info!("  Hash: {}", block.hash);
                    info!("  Height: {}", block.height);
                    info!("  Previous hash: {}", block.previous_hash);
                    info!("  Timestamp: {}", block.timestamp);
                    info!("  Difficulty: {}", block.difficulty);
                    info!("  Nonce: {}", block.nonce);
                    info!("  Merkle root: {}", block.merkle_root);
                    info!("  Type: {:?}", block.block_type);
                    info!("  Size: {} transactions", block.size);
                    info!("  Miner: {}", block.miner);
                    info!("  Reward: {} WRF", block.reward);

                    if !block.transactions.is_empty() {
                        info!("  Transactions:");
                        for (i, tx) in block.transactions.iter().enumerate() {
                            info!("    {}: {} ({})", i + 1, tx.id, tx.hash);
                        }
                    } else {
                        info!("  Transactions: None (Genesis block)");
                    }
                },
                None => {
                    if let Some(h) = hash {
                        return Err(CliError::new(
                            CliErrorCode::NotFound,
                            format!("Block not found with hash: {}", h),
                        )
                        .into());
                    } else if let Some(h) = height {
                        return Err(CliError::new(
                            CliErrorCode::NotFound,
                            format!("Block not found with height: {}", h),
                        )
                        .into());
                    } else {
                        return Err(CliError::new(
                            CliErrorCode::NotFound,
                            "No blocks found in blockchain",
                        )
                        .into());
                    }
                },
            }
        },
        ChainCommands::ListBlocks { limit } => {
            let chain = blockchain.read().await;
            let blocks = chain.get_latest_blocks(*limit as u64);

            if blocks.is_empty() {
                info!("No blocks found");
            } else {
                info!("Latest {} blocks:", blocks.len());
                for (i, block) in blocks.iter().enumerate() {
                    info!(
                        "  {} - Height: {}, Hash: {}",
                        i + 1,
                        block.height,
                        block.hash
                    );
                    info!(
                        "      Type: {:?}, Time: {}, Txs: {}",
                        block.block_type,
                        block.timestamp,
                        block.transactions.len()
                    );
                }
            }
        },
        ChainCommands::PendingTransactions => {
            let chain = blockchain.read().await;
            let pending = chain.get_pending_transactions();

            if pending.is_empty() {
                info!("No pending transactions");
            } else {
                info!("Pending transactions:");
                for (i, tx) in pending.iter().enumerate() {
                    info!("  {} - ID: {}", i + 1, tx.id);
                    info!("      Type: {:?}", tx.tx_type);
                    info!("      From: {}", tx.from);
                    info!("      To: {}", tx.to);
                    info!("      Amount: {} WRF", tx.amount);
                    info!("      Fee: {} WRF", tx.fee);
                }
            }
        },
        ChainCommands::Export { output } => {
            // Create a directory for the output if it doesn't exist
            if let Some(parent) = Path::new(output).parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }

            let chain = blockchain.read().await;

            // In a real implementation, we would serialize the actual blockchain data
            // For now, we'll create a sample JSON file
            let mut blocks_json = Vec::new();
            for block in &chain.blocks {
                blocks_json.push(format!(
                    r#"{{
                        "hash": "{}",
                        "height": {},
                        "timestamp": "{}",
                        "previous_hash": "{}",
                        "difficulty": {}
                    }}"#,
                    block.hash,
                    block.height,
                    block.timestamp,
                    block.previous_hash,
                    block.difficulty
                ));
            }

            let blockchain_json = format!(
                r#"{{
                    "blockchain": {{
                        "name": "Wrfcoin",
                        "height": {},
                        "difficulty": {},
                        "chain_id": "{}",
                        "blocks": [
                            {}
                        ]
                    }}
                }}"#,
                chain.height,
                chain.difficulty,
                chain.chain_id,
                blocks_json.join(",\n")
            );

            fs::write(output, blockchain_json)?;
            info!("Exported blockchain to: {}", output);
            info!("Exported {} blocks", chain.blocks.len());
        },
        ChainCommands::Import { input } => {
            if !Path::new(input).exists() {
                return Err(anyhow!("Input file does not exist: {}", input));
            }

            // Read the file
            let data = fs::read_to_string(input)?;

            // In a real implementation, we would parse and validate the imported data
            // and update the blockchain accordingly
            info!("Importing blockchain from: {}", input);
            info!("Read {} bytes of blockchain data", data.len());
            info!("Blockchain data imported successfully");

            // This would normally be followed by validation and application to the blockchain
            info!(
                "Note: In a full implementation, the imported data would be validated and applied to the blockchain"
            );
        },
    }

    Ok(())
}

/// Handles NFT commands
#[allow(dead_code)]
pub async fn handle_nft_command(
    command: &NftCommands,
    nft_manager: Arc<RwLock<ComputationalNftManager>>,
    _blockchain: Arc<RwLock<Blockchain>>,
    wallet: Arc<RwLock<Wallet>>,
) -> Result<()> {
    match command {
        NftCommands::List => {
            let nft_manager = nft_manager.read().await;
            let nfts = nft_manager.list_computational_nfts();

            if nfts.is_empty() {
                info!("No computational NFTs found");
            } else {
                info!("Computational NFTs:");
                for (i, nft_id) in nfts.iter().enumerate() {
                    info!("  {}: {}", i + 1, nft_id);
                    info!(
                        "      Note: Full NFT details would be available in a complete implementation"
                    );
                }
            }
        },
        NftCommands::Show { id } => {
            let nft_manager = nft_manager.read().await;
            match nft_manager.get_computational_nft(id) {
                Some(nft_data) => {
                    info!("NFT Information:");
                    info!("  ID: {}", id);
                    info!("  Data: {}", nft_data);
                    info!(
                        "  Note: Full NFT details would be available in a complete implementation"
                    );
                },
                None => {
                    return Err(anyhow!("NFT not found with ID: {}", id));
                },
            }
        },
        NftCommands::Create {
            creator,
            title,
            description,
            computation_type,
            input_data,
            reward,
        } => {
            // Get creator address (use primary if not specified)
            let creator_addr = match creator {
                Some(c) => c.clone(),
                None => {
                    let wallet_instance = wallet.read().await;
                    match wallet_instance.get_primary_address() {
                        Some(addr) => addr.clone(),
                        None => return Err(anyhow!("No primary address set")),
                    }
                },
            };

            // Check if the creator address is in our wallet
            let wallet_instance = wallet.read().await;
            if !wallet_instance.owns_address(&creator_addr) {
                return Err(anyhow!(
                    "Creator address not found in wallet: {}",
                    creator_addr
                ));
            }

            // Drop the wallet lock before acquiring the NFT manager lock
            drop(wallet_instance);

            let mut nft_manager = nft_manager.write().await;
            let nft_id = nft_manager.create_computational_nft(
                &creator_addr,
                title,
                description,
                computation_type,
                input_data,
                *reward,
            )?;

            info!("Created new computational NFT:");
            info!("  Title: {}", title);
            info!("  ID: {}", nft_id);
            info!("  Creator: {}", creator_addr);
            info!("  Computation type: {}", computation_type);
            info!("  Reward: {} WRF", reward);
        },
        NftCommands::Submit {
            id,
            executor,
            result,
        } => {
            // Get executor address (use primary if not specified)
            let executor_addr = match executor {
                Some(e) => e.clone(),
                None => {
                    let wallet_instance = wallet.read().await;
                    match wallet_instance.get_primary_address() {
                        Some(addr) => addr.clone(),
                        None => return Err(anyhow!("No primary address set")),
                    }
                },
            };

            // Check if the executor address is in our wallet
            let wallet_instance = wallet.read().await;
            if !wallet_instance.owns_address(&executor_addr) {
                return Err(anyhow!(
                    "Executor address not found in wallet: {}",
                    executor_addr
                ));
            }

            // Drop the wallet lock before acquiring the NFT manager lock
            drop(wallet_instance);

            // Check if the NFT exists
            let nft_manager_read = nft_manager.read().await;
            let nft_exists = match nft_manager_read.get_computational_nft(id) {
                Some(_) => true,
                None => false,
            };

            if !nft_exists {
                return Err(anyhow!("NFT not found with ID: {}", id));
            }

            // Drop the read lock before acquiring the write lock
            drop(nft_manager_read);

            let mut nft_manager = nft_manager.write().await;
            let tx_id = nft_manager.submit_computation_result(id, &executor_addr)?;

            info!("Submitted computation result:");
            info!("  NFT ID: {}", id);
            info!("  Executor: {}", executor_addr);
            info!("  Result length: {} bytes", result.len());
            info!("  Transaction ID: {}", tx_id);
        },
        NftCommands::Categories => {
            let nft_manager = nft_manager.read().await;
            let categories = nft_manager.get_categories();

            if categories.is_empty() {
                info!("No computation categories defined");
            } else {
                info!("Available computation categories:");
                for (i, category) in categories.iter().enumerate() {
                    info!("  {}: {}", i + 1, category);
                }
            }
        },
        NftCommands::AddCategory { name } => {
            let mut nft_manager = nft_manager.write().await;
            match nft_manager.add_category(name) {
                Ok(_) => {
                    info!("Added new computation category: {}", name);
                },
                Err(e) => {
                    return Err(anyhow!("Failed to add category: {}", e));
                },
            }
        },
    }

    Ok(())
}

/// Handles node commands
#[allow(dead_code)]
pub async fn handle_node_command(
    command: &NodeCommands,
    node_stats: HashMap<String, String>,
) -> Result<()> {
    match command {
        NodeCommands::Info => {
            info!("Node Information:");

            // Display node information from stats
            if let Some(node_id) = node_stats.get("node_id") {
                info!("  Node ID: {}", node_id);
            }

            if let Some(network) = node_stats.get("network") {
                info!("  Network: {}", network);
            }

            if let Some(version) = node_stats.get("version") {
                info!("  Version: {}", version);
            }

            if let Some(running) = node_stats.get("is_running") {
                info!("  Running: {}", running);

                if running == "true" {
                    if let Some(uptime) = node_stats.get("uptime_seconds") {
                        let uptime_secs = uptime.parse::<u64>().unwrap_or(0);
                        let hours = uptime_secs / 3600;
                        let minutes = (uptime_secs % 3600) / 60;
                        let seconds = uptime_secs % 60;

                        info!("  Uptime: {}h {}m {}s", hours, minutes, seconds);
                    }

                    if let Some(height) = node_stats.get("blockchain_height") {
                        info!("  Blockchain Height: {}", height);
                    }

                    if let Some(difficulty) = node_stats.get("blockchain_difficulty") {
                        info!("  Current Difficulty: {}", difficulty);
                    }

                    if let Some(pending) = node_stats.get("pending_transactions") {
                        info!("  Pending Transactions: {}", pending);
                    }

                    if let Some(peers) = node_stats.get("connected_peers") {
                        info!("  Connected Peers: {}", peers);
                    }

                    if let Some(is_mining) = node_stats.get("is_mining") {
                        info!("  Mining: {}", is_mining);

                        if is_mining == "true" {
                            if let Some(hashrate) = node_stats.get("hashrate") {
                                info!("  Hashrate: {} H/s", hashrate);
                            }

                            if let Some(blocks_mined) = node_stats.get("blocks_mined") {
                                info!("  Blocks Mined: {}", blocks_mined);
                            }
                        }
                    }
                }
            }
        },
    }

    Ok(())
}

#[cfg(test)]
mod chain_error_tests {
    use super::*;
    use wrfcoin_cli::errors::CliErrorCode;

    struct EmptyDatabase;

    impl Database for EmptyDatabase {}

    fn empty_blockchain() -> Blockchain {
        Blockchain {
            tip: "missing-tip".to_string(),
            height: 0,
            difficulty: 0.0,
            blocks: Vec::new(),
            target_block_time: 12,
            chain_id: "test".to_string(),
            pending_transactions: Vec::new(),
        }
    }

    #[tokio::test]
    async fn missing_tip_is_a_blockchain_error() {
        let err = handle_chain_command(
            &ChainCommands::Info,
            Arc::new(RwLock::new(empty_blockchain())),
            Arc::new(EmptyDatabase),
        )
        .await
        .expect_err("missing tip must be classified as a blockchain error");

        let code = CliErrorCode::from_error(&err);
        assert_eq!(code, CliErrorCode::BlockchainError);
        assert_eq!(code.exit_code(), 5);
    }

    #[tokio::test]
    async fn missing_block_is_not_found_not_user_error() {
        let err = handle_chain_command(
            &ChainCommands::Block {
                hash: Some("missing-hash".to_string()),
                height: None,
            },
            Arc::new(RwLock::new(empty_blockchain())),
            Arc::new(EmptyDatabase),
        )
        .await
        .expect_err("missing block must be classified as not found");

        let code = CliErrorCode::from_error(&err);
        assert_eq!(code, CliErrorCode::NotFound);
        assert_eq!(code.exit_code(), 7);
    }
}

#[cfg(test)]
mod wallet_state_2735_tests {
    use super::*;

    /// Byte-shape of the real `~/.wrfcoin/wallet/wallet-state.json` that #2735
    /// was filed against: v2 structured addresses (maps, not strings) AND
    /// pre-#505 float balances. The current parser wants u64 balances, the
    /// legacy parser wants string addresses — this file satisfies neither.
    const HYBRID_V2_FLOAT: &str = r#"{
  "version": "2",
  "addresses": [
    {
      "address": "Wrf9f685213cbba7448ba10b8f4c6eb15d567d9b702",
      "key_path": "/home/user/.wrfcoin/keys/Wrf9f685213cbba7448ba10b8f4c6eb15d567d9b702.json",
      "created_at": "2026-02-21T17:37:14.762491864+00:00"
    },
    {
      "address": "Wrf1d4ff940f8f624ed2b1b6b4b00014a129d9a3c50",
      "key_path": "/home/user/.wrfcoin/keys/Wrf1d4ff940f8f624ed2b1b6b4b00014a129d9a3c50.json",
      "created_at": "2026-02-21T17:37:14.763799593+00:00"
    }
  ],
  "primary_address": "Wrf9f685213cbba7448ba10b8f4c6eb15d567d9b702",
  "balances": {
    "Wrf9f685213cbba7448ba10b8f4c6eb15d567d9b702": 0.0,
    "Wrf1d4ff940f8f624ed2b1b6b4b00014a129d9a3c50": 1.5
  },
  "transaction_history": [],
  "legacy_addresses": []
}"#;

    fn scratch_file(name: &str, contents: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wrfcoin-2735-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wallet-state.json");
        fs::write(&path, contents).unwrap();
        path
    }

    /// **The #2735 regression test.** A wallet-state written by the Feb-2026
    /// CLI (v2 addresses + float balances) must load, keeping every address.
    /// RED on pre-fix code: the primary parse rejects the float balances, the
    /// legacy fallback rejects the structured addresses, and the command dies
    /// with "invalid type: map, expected a string at line 4 column 4".
    #[test]
    fn v2_wallet_state_with_float_balances_loads_2735() {
        let path = scratch_file("hybrid", HYBRID_V2_FLOAT);

        let state = Wallet::read_state(&path)
            .expect("a v2 wallet-state with pre-#505 float balances must remain readable (#2735)");

        assert_eq!(
            state.addresses.len(),
            2,
            "both managed addresses must survive"
        );
        assert_eq!(
            state.addresses[0].address,
            "Wrf9f685213cbba7448ba10b8f4c6eb15d567d9b702"
        );
        assert_eq!(
            state.primary_address.as_deref(),
            Some("Wrf9f685213cbba7448ba10b8f4c6eb15d567d9b702")
        );
        // 1.5 WRF -> 150_000_000 base units under WRF_UNITS_PER_COIN = 10^8.
        assert_eq!(
            state
                .balances
                .get("Wrf1d4ff940f8f624ed2b1b6b4b00014a129d9a3c50")
                .copied(),
            Some(150_000_000)
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    /// #2735 defect (b): an unreadable wallet-state must fail wallet INIT with
    /// the file named — not fall back to a cwd-rooted wallet that scatters key
    /// material into ./keys and then dies with a bare ENOENT.
    #[test]
    fn init_failure_propagates_with_file_named_and_writes_nothing_2735() {
        let base = std::env::temp_dir().join(format!("wrfcoin-2735-init-{}", std::process::id()));
        let wallet_dir = base.join("wallet");
        fs::create_dir_all(&wallet_dir).unwrap();
        fs::write(
            wallet_dir.join("wallet-state.json"),
            r#"{"version": 7, "addresses": 3}"#,
        )
        .unwrap();

        let err = Wallet::load_or_initialize_at(base.clone())
            .expect_err("corrupt wallet-state must fail init, not fall back")
            .to_string();

        assert!(
            err.contains("wallet-state.json"),
            "init error must name the failing file (#2735), got: {err}"
        );
        assert!(
            !base.join("keys").exists(),
            "a failed init must not generate key material"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// #2735 defect (a): a wallet-state that parses as NOTHING must produce an
    /// error naming the FILE. "line 4 column 4" across six-plus JSON files
    /// under ~/.wrfcoin is what made the original bisect inconclusive.
    #[test]
    fn unparseable_wallet_state_error_names_the_file_2735() {
        let path = scratch_file("garbage", r#"{"version": 7, "addresses": 3}"#);

        let err = Wallet::read_state(&path)
            .expect_err("garbage wallet-state must not parse")
            .to_string();

        assert!(
            err.contains("wallet-state.json"),
            "the error must name the file it failed to parse (#2735), got: {err}"
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}

#[cfg(test)]
mod wallet_address_migration_tests {
    use super::*;
    use crate::crypto::WrfKeyPair;

    const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    fn legacy_wallet(name: &str) -> (Wallet, PathBuf, PathBuf, String, String) {
        let base = std::env::temp_dir().join(format!(
            "wrfcoin-wallet-migration-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("wallet")).unwrap();
        fs::create_dir_all(base.join("keys")).unwrap();

        let mut key = WrfKeyPair::from_mnemonic(PHRASE).unwrap();
        let canonical = key.canonical_address().unwrap();
        let legacy = key.legacy_address().unwrap();
        key.address = legacy.clone();
        let legacy_path = base.join("keys").join(format!("{legacy}.json"));
        key.save_to_file(&legacy_path).unwrap();
        let state_path = base.join("wallet/wallet-state.json");
        let wallet = Wallet {
            base_dir: base.clone(),
            state_path,
            state: WalletState {
                addresses: vec![ManagedAddress {
                    address: legacy.clone(),
                    key_path: legacy_path.display().to_string(),
                    created_at: "2026-09-18T00:00:00Z".to_string(),
                }],
                primary_address: Some(legacy.clone()),
                ..WalletState::default()
            },
        };
        wallet.persist_state().unwrap();
        (wallet, base, legacy_path, legacy, canonical)
    }

    #[test]
    fn legacy_wallet_refuses_to_sign_until_migrated() {
        let (wallet, base, _, legacy, _) = legacy_wallet("refuse-sign");
        let error = wallet
            .sign_transparent_transfer("0x2222222222222222222222222222222222222222", 1, 0)
            .expect_err("legacy wallet must not sign")
            .to_string();
        assert!(error.contains(&legacy));
        assert!(error.contains("migrate-addresses"));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn migrate_addresses_dry_run_changes_nothing() {
        let (mut wallet, base, legacy_path, legacy, canonical) = legacy_wallet("dry-run");
        let before = fs::read(&wallet.state_path).unwrap();
        let (mappings, backup) = wallet.migrate_addresses(true).unwrap();
        assert_eq!(mappings, vec![(legacy, canonical.clone())]);
        assert!(backup.is_none());
        assert_eq!(fs::read(&wallet.state_path).unwrap(), before);
        assert!(legacy_path.exists());
        assert!(!base.join("keys").join(format!("{canonical}.json")).exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn migrate_addresses_writes_backup_and_shows_both() {
        let (mut wallet, base, legacy_path, legacy, canonical) = legacy_wallet("apply");
        let old_index = fs::read(&wallet.state_path).unwrap();
        let (mappings, backup) = wallet.migrate_addresses(false).unwrap();
        assert_eq!(mappings, vec![(legacy.clone(), canonical.clone())]);
        let backup = PathBuf::from(backup.expect("backup path"));
        assert_eq!(fs::read(&backup).unwrap(), old_index);
        assert!(
            legacy_path.exists(),
            "backup index lost its legacy key file"
        );
        assert!(base.join("keys").join(format!("{canonical}.json")).exists());
        assert_eq!(
            wallet.state.primary_address.as_deref(),
            Some(canonical.as_str())
        );
        assert_eq!(wallet.state.addresses[0].address, canonical);
        assert_eq!(
            wallet.state.addresses[0].key_path,
            base.join("keys")
                .join(format!("{}.json", wallet.state.addresses[0].address))
                .display()
                .to_string()
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn migrated_wallet_reload_is_idempotent_and_duplicate_free() {
        let (mut wallet, base, _, legacy, canonical) = legacy_wallet("reload-idempotent");
        let (mappings, _) = wallet.migrate_addresses(false).unwrap();
        assert_eq!(mappings, vec![(legacy, canonical.clone())]);

        let mut reloaded = Wallet::load_or_initialize_at(base.clone()).unwrap();
        let addresses = reloaded.get_addresses();
        assert_eq!(addresses, vec![canonical]);
        let (second_mappings, second_backup) = reloaded.migrate_addresses(true).unwrap();
        assert!(second_mappings.is_empty());
        assert!(second_backup.is_none());
        assert_eq!(reloaded.get_addresses().len(), 1);

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn restored_migration_backup_remigrates_without_duplicate_canonical_address() {
        let (mut wallet, base, _, legacy, canonical) = legacy_wallet("restore-backup");
        let (mappings, backup) = wallet.migrate_addresses(false).unwrap();
        assert_eq!(mappings, vec![(legacy, canonical.clone())]);
        fs::copy(backup.unwrap(), &wallet.state_path).unwrap();

        let mut restored = Wallet::load_or_initialize_at(base.clone()).unwrap();
        let _ = restored.migrate_addresses(false).unwrap();
        let reloaded = Wallet::load_or_initialize_at(base.clone()).unwrap();
        assert_eq!(reloaded.get_addresses(), vec![canonical.clone()]);
        assert_eq!(
            reloaded.get_primary_address().as_deref(),
            Some(canonical.as_str())
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn lost_wallet_index_rebuild_prefers_canonical_key_file() {
        let (mut wallet, base, _, _, canonical) = legacy_wallet("lost-index");
        wallet.migrate_addresses(false).unwrap();
        fs::remove_file(&wallet.state_path).unwrap();

        let rebuilt = Wallet::load_or_initialize_at(base.clone()).unwrap();
        assert_eq!(rebuilt.get_addresses(), vec![canonical.clone()]);
        assert_eq!(
            rebuilt.get_primary_address().as_deref(),
            Some(canonical.as_str())
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn malformed_stray_key_file_is_skipped_until_requested() {
        let (_wallet, base, _, legacy, _) = legacy_wallet("malformed-stray");
        let stray = base.join("keys/notes.json");
        fs::write(&stray, "{}").unwrap();

        let warning_error = WrfKeyPair::load_from_file(&stray).unwrap_err();
        let warning = malformed_key_file_warning(&stray, &warning_error);
        assert!(warning.contains("notes.json"));

        let loaded = Wallet::load_or_initialize_at(base.clone()).unwrap();
        assert_eq!(loaded.get_addresses(), vec![legacy]);
        assert!(
            loaded
                .key_manager()
                .unwrap()
                .load_key_pair("notes")
                .is_err()
        );

        let _ = fs::remove_dir_all(base);
    }
}
