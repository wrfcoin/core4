use anyhow::{Context, Result, anyhow};
use crate::errors::{CliError, CliErrorCode};
use log::debug;
use serde::{Deserialize, Serialize};
use serde_json;
use shellexpand;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Network type
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Network {
    /// Mainnet network
    Mainnet,
    /// Testnet network
    Testnet,
    /// Local development network
    Local,
}

impl FromStr for Network {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mainnet" => Ok(Network::Mainnet),
            "testnet" => Ok(Network::Testnet),
            "local" => Ok(Network::Local),
            _ => Err(anyhow!("Invalid network: {}", s)),
        }
    }
}

impl ToString for Network {
    fn to_string(&self) -> String {
        match self {
            Network::Mainnet => "mainnet".to_string(),
            Network::Testnet => "testnet".to_string(),
            Network::Local => "local".to_string(),
        }
    }
}

/// CLI configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CliConfig {
    /// Configuration version
    pub version: String,
    /// Network configuration
    pub network: Network,
    /// API configuration
    pub api: ApiConfig,
    /// WebSocket configuration
    pub websocket: WebSocketConfig,
    /// Paths configuration
    pub paths: PathsConfig,
    /// Wallet configuration
    pub wallet: WalletConfig,
    /// CLI UX preferences
    #[serde(default)]
    pub preferences: PreferencesConfig,
}

/// API configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiConfig {
    /// API host
    pub host: String,
    /// API port
    pub port: u16,
    /// API protocol
    pub protocol: String,
    /// API timeout in seconds
    pub timeout: u64,
    /// Number of retry attempts
    pub retry_attempts: u32,
    /// Delay between retry attempts in seconds
    pub retry_delay: u64,
}

/// WebSocket configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebSocketConfig {
    /// WebSocket host
    pub host: String,
    /// WebSocket port
    pub port: u16,
    /// WebSocket protocol
    pub protocol: String,
    /// WebSocket path
    pub path: String,
    /// Heartbeat interval in seconds
    pub heartbeat_interval: u64,
    /// Reconnect interval in seconds
    pub reconnect_interval: u64,
    /// Maximum reconnect attempts
    pub max_reconnect_attempts: u32,
}

/// Paths configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PathsConfig {
    /// Base directory
    pub base_dir: String,
    /// Keys directory
    pub keys_dir: String,
    /// Data directory
    pub data_dir: String,
    /// Logs directory
    pub logs_dir: String,
}

/// Wallet configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Default wallet address
    pub default_address: Option<String>,
    /// Default transaction fee
    pub default_fee: f64,
    /// Whether to auto-save wallets
    pub auto_save: bool,
}

/// Preferred default output mode for CLI responses
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputPreference {
    Human,
    Json,
    Quiet,
}

/// API key lookup source
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeySource {
    Env,
    Config,
}

/// Where the active configuration was loaded from
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigSource {
    Global,
    ProjectLocal,
    ProjectProfile,
    Explicit,
}

/// Operator-facing CLI preference settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreferencesConfig {
    pub output_mode: OutputPreference,
    pub api_key_source: ApiKeySource,
}

impl Default for PreferencesConfig {
    fn default() -> Self {
        Self {
            output_mode: OutputPreference::Human,
            api_key_source: ApiKeySource::Env,
        }
    }
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            version: "0.1.0".to_string(),
            network: Network::Testnet,
            api: ApiConfig {
                host: "127.0.0.1".to_string(),
                port: 8556,
                protocol: "http".to_string(),
                timeout: 30,
                retry_attempts: 5,
                retry_delay: 2,
            },
            websocket: WebSocketConfig {
                host: "localhost".to_string(),
                port: 5001,
                protocol: "ws".to_string(),
                path: "/ws".to_string(),
                heartbeat_interval: 30,
                reconnect_interval: 3,
                max_reconnect_attempts: 5,
            },
            paths: PathsConfig {
                base_dir: "~/.wrfcoin".to_string(),
                keys_dir: "~/.wrfcoin/keys".to_string(),
                data_dir: "~/.wrfcoin/data".to_string(),
                logs_dir: "~/.wrfcoin/logs".to_string(),
            },
            wallet: WalletConfig {
                default_address: None,
                default_fee: 0.001,
                auto_save: true,
            },
            preferences: PreferencesConfig::default(),
        }
    }
}

impl CliConfig {
    /// Get the API URL
    pub fn api_url(&self) -> String {
        format!("{}://{}:{}", self.api.protocol, self.api.host, self.api.port)
    }

    /// Get the WebSocket URL
    pub fn websocket_url(&self) -> String {
        format!(
            "{}://{}:{}{}",
            self.websocket.protocol, self.websocket.host, self.websocket.port, self.websocket.path
        )
    }

    /// Base directory path
    pub fn base_dir_path(&self) -> PathBuf {
        PathBuf::from(shellexpand::tilde(&self.paths.base_dir).to_string())
    }

    /// Keys directory path
    pub fn keys_dir_path(&self) -> PathBuf {
        PathBuf::from(shellexpand::tilde(&self.paths.keys_dir).to_string())
    }

    /// Data directory path
    pub fn data_dir_path(&self) -> PathBuf {
        PathBuf::from(shellexpand::tilde(&self.paths.data_dir).to_string())
    }

    /// Logs directory path
    pub fn logs_dir_path(&self) -> PathBuf {
        PathBuf::from(shellexpand::tilde(&self.paths.logs_dir).to_string())
    }
}

/// Stores the available profiles in the CLI config file
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileStore {
    pub active_profile: String,
    pub profiles: BTreeMap<String, CliConfig>,
}

impl Default for ProfileStore {
    fn default() -> Self {
        let mut profiles = BTreeMap::new();
        profiles.insert("default".to_string(), CliConfig::default());

        Self {
            active_profile: "default".to_string(),
            profiles,
        }
    }
}

impl ProfileStore {
    pub fn active_config(&self) -> &CliConfig {
        self.profiles.get(&self.active_profile).unwrap_or_else(|| {
            self.profiles
                .values()
                .next()
                .expect("At least one profile exists")
        })
    }
}

/// Configuration manager
pub struct ConfigManager {
    store: ProfileStore,
    config_path: PathBuf,
    source: ConfigSource,
}

impl ConfigManager {
    /// Create a new configuration manager using the default path
    pub fn new() -> Result<Self> {
        let global_path = get_config_path()?;
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::resolve(&cwd, &global_path)
    }

    /// Create a new configuration manager with a specific path
    pub fn with_path(config_path: PathBuf) -> Result<Self> {
        if let Some(parent) = config_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
                debug!("Created config directory: {}", parent.display());
            }
            set_secure_dir_perms(parent)?;
        }

        let store = if config_path.exists() {
            Self::load_profile_store(&config_path)?
        } else {
            let default_store = ProfileStore::default();
            Self::save_profile_store(&config_path, &default_store)?;
            default_store
        };

        Ok(Self {
            store,
            config_path,
            source: ConfigSource::Explicit,
        })
    }

    /// Get the source from which the active configuration was loaded.
    pub fn config_source(&self) -> ConfigSource {
        self.source.clone()
    }

    /// Get the active CLI configuration
    pub fn get_config(&self) -> &CliConfig {
        self.store.active_config()
    }

    /// Get the configuration file path
    pub fn get_config_path(&self) -> &Path {
        &self.config_path
    }

    /// Get the name of the active profile
    pub fn active_profile(&self) -> &str {
        &self.store.active_profile
    }

    /// List all known profiles
    pub fn list_profiles(&self) -> Vec<String> {
        self.store.profiles.keys().cloned().collect()
    }

    /// Get a profile configuration by name.
    pub fn get_profile(&self, name: &str) -> Option<&CliConfig> {
        self.store.profiles.get(name)
    }

    /// Switch the active profile
    pub fn use_profile(&mut self, name: &str) -> Result<()> {
        if !self.store.profiles.contains_key(name) {
            return Err(
                CliError::new(
                    CliErrorCode::ConfigError,
                    format!("Profile '{}' does not exist", name),
                )
                .into(),
            );
        }

        self.store.active_profile = name.to_string();
        Self::save_profile_store(&self.config_path, &self.store)?;
        Ok(())
    }

    /// Add or update a profile
    pub fn add_profile(&mut self, name: String, config: CliConfig) -> Result<()> {
        self.store.profiles.insert(name, config);
        Self::save_profile_store(&self.config_path, &self.store)?;
        Ok(())
    }

    /// Update the active profile's configuration
    pub fn update_config<F>(&mut self, update_fn: F) -> Result<()>
    where
        F: FnOnce(&mut CliConfig),
    {
        let active = self.active_profile().to_string();
        let mut config = self.get_config().clone();
        update_fn(&mut config);
        self.store.profiles.insert(active, config);
        Self::save_profile_store(&self.config_path, &self.store)?;
        Ok(())
    }

    /// Reset the active profile to defaults
    pub fn reset_config(&mut self) -> Result<()> {
        let active = self.active_profile().to_string();
        self.store.profiles.insert(active, CliConfig::default());
        Self::save_profile_store(&self.config_path, &self.store)?;
        Ok(())
    }

    /// Create the required directories for the active profile
    pub fn create_directories(&self) -> Result<()> {
        let config = self.get_config();
        let base_dir = config.base_dir_path();
        if !base_dir.exists() {
            fs::create_dir_all(&base_dir)?;
            debug!("Created base directory: {}", base_dir.display());
        }

        let keys_dir = config.keys_dir_path();
        if !keys_dir.exists() {
            fs::create_dir_all(&keys_dir)?;
            debug!("Created keys directory: {}", keys_dir.display());
        }

        let data_dir = config.data_dir_path();
        if !data_dir.exists() {
            fs::create_dir_all(&data_dir)?;
            debug!("Created data directory: {}", data_dir.display());
        }

        let logs_dir = config.logs_dir_path();
        if !logs_dir.exists() {
            fs::create_dir_all(&logs_dir)?;
            debug!("Created logs directory: {}", logs_dir.display());
        }

        Ok(())
    }

    /// Get a copy of the active configuration
    pub fn snapshot(&self) -> CliConfig {
        self.get_config().clone()
    }

    /// Resolve configuration by checking project-local overrides first.
    ///
    /// Priority (highest to lowest):
    /// 1. `.wrfcoin/config.json` in `project_dir` -- full local ProfileStore
    /// 2. `.wrfcoin-profile` in `project_dir` -- profile name selecting from global
    /// 3. `global_path` -- the global `~/.wrfcoin/config.json`
    pub fn resolve(project_dir: &Path, global_path: &Path) -> Result<Self> {
        let local_config = project_dir.join(".wrfcoin").join("config.json");
        if local_config.exists() {
            let mut mgr = Self::with_path(local_config)?;
            mgr.source = ConfigSource::ProjectLocal;
            return Ok(mgr);
        }

        let profile_file = project_dir.join(".wrfcoin-profile");
        if profile_file.exists() {
            let name = fs::read_to_string(&profile_file)?.trim().to_string();
            if name.is_empty() {
                return Err(
                    CliError::new(CliErrorCode::ConfigError, ".wrfcoin-profile is empty").into(),
                );
            }
            let mut mgr = Self::with_path(global_path.to_path_buf())?;
            if !mgr.store.profiles.contains_key(&name) {
                return Err(CliError::new(
                    CliErrorCode::ConfigError,
                    format!(
                        "Profile '{}' from .wrfcoin-profile not found in global config",
                        name
                    ),
                )
                .into());
            }
            mgr.store.active_profile = name;
            mgr.source = ConfigSource::ProjectProfile;
            return Ok(mgr);
        }

        Self::with_path(global_path.to_path_buf())
    }

    fn load_profile_store(path: &Path) -> Result<ProfileStore> {
        debug!("Loading configuration from {}", path.display());
        let config_str = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;

        // #2735: keep the primary parse error so a fallback failure cannot
        // mask it, and name the file — a bare serde "line N column M" across
        // the several JSON files under ~/.wrfcoin is undiagnosable.
        let primary_err = match serde_json::from_str::<ProfileStore>(&config_str) {
            Ok(store) => return Ok(store),
            Err(err) => err,
        };

        let legacy: CliConfig = serde_json::from_str(&config_str).map_err(|legacy_err| {
            anyhow::anyhow!(
                "failed to parse config file {}: {primary_err} (also not readable as a \
                 legacy single-profile config: {legacy_err})",
                path.display()
            )
        })?;
        let mut store = ProfileStore::default();
        store.profiles.insert("default".to_string(), legacy);
        store.active_profile = "default".to_string();
        Ok(store)
    }

    fn save_profile_store(path: &Path, store: &ProfileStore) -> Result<()> {
        debug!("Saving configuration to {}", path.display());
        let config_str = serde_json::to_string_pretty(store)?;
        fs::write(path, config_str)?;
        set_secure_file_perms(path)?;
        Ok(())
    }
}

fn set_secure_dir_perms(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn set_secure_file_perms(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

/// Get the configuration path
fn get_config_path() -> Result<PathBuf> {
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("Home directory not found"))?;
    let config_path = home_dir.join(".wrfcoin").join("config.json");
    Ok(config_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn config_manager_reports_global_source() {
        let dir = std::env::temp_dir().join("wrfcoin-test-source-global");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let mgr = ConfigManager::with_path(path).unwrap();
        assert_eq!(mgr.config_source(), ConfigSource::Explicit);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn config_file_has_secure_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join("wrfcoin-test-perms");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let _mgr = ConfigManager::with_path(path.clone()).unwrap();

        let meta = fs::metadata(&path).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config.json should be owner-only rw");

        let dir_meta = fs::metadata(&dir).unwrap();
        let dir_mode = dir_meta.permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "config dir should be owner-only rwx");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn project_local_profile_file_overrides_active() {
        let global_dir = std::env::temp_dir().join("wrfcoin-test-local-profile-global");
        let project_dir = std::env::temp_dir().join("wrfcoin-test-local-profile-project");
        let _ = fs::remove_dir_all(&global_dir);
        let _ = fs::remove_dir_all(&project_dir);
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&project_dir).unwrap();

        let global_path = global_dir.join("config.json");
        let mut mgr = ConfigManager::with_path(global_path.clone()).unwrap();
        let mut station_cfg = CliConfig::default();
        station_cfg.api.port = 9999;
        mgr.add_profile("station".to_string(), station_cfg).unwrap();

        fs::write(project_dir.join(".wrfcoin-profile"), "station\n").unwrap();

        let resolved = ConfigManager::resolve(&project_dir, &global_path).unwrap();
        assert_eq!(resolved.config_source(), ConfigSource::ProjectProfile);
        assert_eq!(resolved.active_profile(), "station");
        assert_eq!(resolved.get_config().api.port, 9999);

        fs::remove_dir_all(&global_dir).unwrap();
        fs::remove_dir_all(&project_dir).unwrap();
    }

    #[test]
    fn project_profile_resolution_failures_are_config_errors() {
        let suffix = std::process::id();
        let global_dir = std::env::temp_dir().join(format!(
            "wrfcoin-test-profile-errors-global-{suffix}"
        ));
        let project_dir = std::env::temp_dir().join(format!(
            "wrfcoin-test-profile-errors-project-{suffix}"
        ));
        let _ = fs::remove_dir_all(&global_dir);
        let _ = fs::remove_dir_all(&project_dir);
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&project_dir).unwrap();

        let global_path = global_dir.join("config.json");
        let _mgr = ConfigManager::with_path(global_path.clone()).unwrap();
        let profile_path = project_dir.join(".wrfcoin-profile");

        fs::write(&profile_path, "missing\n").unwrap();
        let err = match ConfigManager::resolve(&project_dir, &global_path) {
            Ok(_) => panic!("unknown project profile must be a typed configuration error"),
            Err(err) => err,
        };
        let code = CliErrorCode::from_error(&err);
        assert_eq!(code, CliErrorCode::ConfigError);
        assert_eq!(code.exit_code(), 8);

        fs::write(&profile_path, "\n").unwrap();
        let err = match ConfigManager::resolve(&project_dir, &global_path) {
            Ok(_) => panic!("empty project profile must be a typed configuration error"),
            Err(err) => err,
        };
        assert_eq!(CliErrorCode::from_error(&err), CliErrorCode::ConfigError);

        fs::remove_dir_all(&global_dir).unwrap();
        fs::remove_dir_all(&project_dir).unwrap();
    }

    #[test]
    fn project_local_config_dir_overrides_global() {
        let global_dir = std::env::temp_dir().join("wrfcoin-test-local-config-global");
        let project_dir = std::env::temp_dir().join("wrfcoin-test-local-config-project");
        let _ = fs::remove_dir_all(&global_dir);
        let _ = fs::remove_dir_all(&project_dir);
        fs::create_dir_all(&global_dir).unwrap();
        let local_wrfcoin = project_dir.join(".wrfcoin");
        fs::create_dir_all(&local_wrfcoin).unwrap();

        let global_path = global_dir.join("config.json");
        let _global = ConfigManager::with_path(global_path.clone()).unwrap();

        let local_path = local_wrfcoin.join("config.json");
        let mut local_mgr = ConfigManager::with_path(local_path.clone()).unwrap();
        local_mgr.update_config(|cfg| cfg.api.port = 7777).unwrap();

        let resolved = ConfigManager::resolve(&project_dir, &global_path).unwrap();
        assert_eq!(resolved.config_source(), ConfigSource::ProjectLocal);
        assert_eq!(resolved.get_config().api.port, 7777);

        fs::remove_dir_all(&global_dir).unwrap();
        fs::remove_dir_all(&project_dir).unwrap();
    }

    #[test]
    fn profile_crud_lifecycle() {
        let dir = std::env::temp_dir().join("wrfcoin-test-crud");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");

        let mut mgr = ConfigManager::with_path(path.clone()).unwrap();

        assert_eq!(mgr.active_profile(), "default");
        assert_eq!(mgr.list_profiles(), vec!["default"]);

        let mut station_cfg = CliConfig::default();
        station_cfg.api.port = 5555;
        mgr.add_profile("station".to_string(), station_cfg).unwrap();
        assert_eq!(mgr.list_profiles().len(), 2);

        mgr.use_profile("station").unwrap();
        assert_eq!(mgr.active_profile(), "station");
        assert_eq!(mgr.get_config().api.port, 5555);

        mgr.update_config(|cfg| cfg.api.port = 6666).unwrap();
        assert_eq!(mgr.get_config().api.port, 6666);

        let err = mgr
            .use_profile("nonexistent")
            .expect_err("missing profile must be a typed configuration error");
        let code = CliErrorCode::from_error(&err);
        assert_eq!(code, CliErrorCode::ConfigError);
        assert_eq!(code.exit_code(), 8);

        assert!(mgr.get_profile("default").is_some());
        assert!(mgr.get_profile("nope").is_none());

        let reloaded = ConfigManager::with_path(path).unwrap();
        assert_eq!(reloaded.active_profile(), "station");
        assert_eq!(reloaded.get_config().api.port, 6666);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legacy_single_config_migrates_to_profile_store() {
        let dir = std::env::temp_dir().join("wrfcoin-test-legacy");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");

        let legacy = CliConfig::default();
        let legacy_json = serde_json::to_string_pretty(&legacy).unwrap();
        fs::write(&path, &legacy_json).unwrap();

        let mgr = ConfigManager::with_path(path).unwrap();
        assert_eq!(mgr.active_profile(), "default");
        assert_eq!(mgr.list_profiles(), vec!["default"]);
        assert_eq!(mgr.get_config().api.port, legacy.api.port);

        fs::remove_dir_all(&dir).unwrap();
    }
}
