use anyhow::Result;
use clap::ValueEnum;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CACHE_TTL_SECONDS: i64 = 300;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CompletionDomain {
    Bridges,
    Peers,
    Stations,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CompletionCache {
    pub fetched_at: i64,
    pub api_url: String,
    pub bridges: Vec<String>,
    pub peers: Vec<String>,
    pub stations: Vec<String>,
}

impl CompletionCache {
    fn is_fresh_for(&self, api_url: &str) -> bool {
        if self.api_url != api_url {
            return false;
        }

        let now = chrono::Utc::now().timestamp();
        now - self.fetched_at <= CACHE_TTL_SECONDS
    }
}

pub async fn fetch_completion_values(
    domain: CompletionDomain,
    api_url: &str,
    config_path: Option<&PathBuf>,
    refresh: bool,
) -> Result<Vec<String>> {
    let cache = if refresh {
        refresh_completion_cache(api_url, config_path).await?
    } else {
        let existing = load_completion_cache(config_path)?;
        if existing.is_fresh_for(api_url) {
            existing
        } else {
            refresh_completion_cache(api_url, config_path).await?
        }
    };

    let values = match domain {
        CompletionDomain::Bridges => cache.bridges,
        CompletionDomain::Peers => cache.peers,
        CompletionDomain::Stations => cache.stations,
    };
    Ok(values)
}

pub async fn refresh_completion_cache(
    api_url: &str,
    config_path: Option<&PathBuf>,
) -> Result<CompletionCache> {
    let client = Client::builder().timeout(Duration::from_secs(3)).build()?;

    let bridges_fut = fetch_bridges(&client, api_url);
    let peers_fut = fetch_peers(&client, api_url);
    let stations_fut = fetch_stations(&client, api_url);
    let (bridges, peers, stations) = tokio::join!(bridges_fut, peers_fut, stations_fut);

    let cache = CompletionCache {
        fetched_at: chrono::Utc::now().timestamp(),
        api_url: api_url.to_string(),
        bridges,
        peers,
        stations,
    };
    save_completion_cache(config_path, &cache)?;
    Ok(cache)
}

pub fn load_completion_cache(config_path: Option<&PathBuf>) -> Result<CompletionCache> {
    let path = cache_path(config_path)?;
    if !path.exists() {
        return Ok(CompletionCache::default());
    }

    let raw = fs::read_to_string(path)?;
    let cache = serde_json::from_str::<CompletionCache>(&raw)?;
    Ok(cache)
}

pub fn clear_completion_cache(config_path: Option<&PathBuf>) -> Result<()> {
    let path = cache_path(config_path)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn generate_completion_script(shell: CompletionShell) -> String {
    match shell {
        CompletionShell::Bash => bash_script(),
        CompletionShell::Zsh => zsh_script(),
        CompletionShell::Fish => fish_script(),
    }
}

fn bash_script() -> String {
    r#"_wrfcoin_completion_values() {
  local domain="$1"
  wrfcoin completion values "$domain" 2>/dev/null
}

_wrfcoin_complete() {
  local cur prev first
  COMPREPLY=()
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"
  first="${COMP_WORDS[1]}"

  case "$prev" in
    --peer) COMPREPLY=( $(compgen -W "$(_wrfcoin_completion_values peers)" -- "$cur") ); return 0 ;;
    --bridge|--target-chain|--chain) COMPREPLY=( $(compgen -W "$(_wrfcoin_completion_values bridges)" -- "$cur") ); return 0 ;;
    --station-id) COMPREPLY=( $(compgen -W "$(_wrfcoin_completion_values stations)" -- "$cur") ); return 0 ;;
  esac

  if [[ $COMP_CWORD -eq 1 ]]; then
    COMPREPLY=( $(compgen -W "start wallet profile completions" -- "$cur") )
    return 0
  fi

  case "$first" in
    wallet)
      if [[ $COMP_CWORD -eq 2 ]]; then
        COMPREPLY=( $(compgen -W "new-address list-addresses balance set-primary send history" -- "$cur") )
      fi
      ;;
    profile)
      if [[ $COMP_CWORD -eq 2 ]]; then
        COMPREPLY=( $(compgen -W "list use create" -- "$cur") )
      fi
      ;;
    completions)
      if [[ $COMP_CWORD -eq 2 ]]; then
        COMPREPLY=( $(compgen -W "bash zsh fish" -- "$cur") )
      fi
      ;;
  esac
}

complete -F _wrfcoin_complete wrfcoin
"#
    .to_string()
}

fn zsh_script() -> String {
    r#"#compdef wrfcoin

_wrfcoin_completion_values() {
  local domain="$1"
  wrfcoin completion values "$domain" 2>/dev/null
}

_wrfcoin() {
  local -a top wallet_cmds profile_cmds shells
  top=(start wallet profile completions)
  wallet_cmds=(new-address list-addresses balance set-primary send history)
  profile_cmds=(list use create)
  shells=(bash zsh fish)

  if (( CURRENT == 2 )); then
    _describe 'command' top
    return
  fi

  case "$words[2]" in
    wallet)
      if (( CURRENT == 3 )); then
        _describe 'wallet command' wallet_cmds
      fi
      ;;
    profile)
      if (( CURRENT == 3 )); then
        _describe 'profile command' profile_cmds
      fi
      ;;
    completions)
      if (( CURRENT == 3 )); then
        _describe 'shell' shells
      fi
      ;;
  esac
}

_wrfcoin "$@"
"#
    .to_string()
}

fn fish_script() -> String {
    r#"function __wrfcoin_completion_values
    set -l domain $argv[1]
    wrfcoin completion values $domain 2>/dev/null
end

complete -c wrfcoin -f -n "__fish_use_subcommand" -a "start wallet profile completions"
complete -c wrfcoin -f -n "__fish_seen_subcommand_from wallet" -a "new-address list-addresses balance set-primary send history"
complete -c wrfcoin -f -n "__fish_seen_subcommand_from profile" -a "list use create"
complete -c wrfcoin -f -n "__fish_seen_subcommand_from completions" -a "bash zsh fish"

complete -c wrfcoin -l peer -a "(__wrfcoin_completion_values peers)"
complete -c wrfcoin -l bridge -a "(__wrfcoin_completion_values bridges)"
complete -c wrfcoin -l target-chain -a "(__wrfcoin_completion_values bridges)"
complete -c wrfcoin -l chain -a "(__wrfcoin_completion_values bridges)"
complete -c wrfcoin -l station-id -a "(__wrfcoin_completion_values stations)"
"#
    .to_string()
}

async fn fetch_bridges(client: &Client, api_url: &str) -> Vec<String> {
    let mut set = BTreeSet::new();

    if let Ok(json) = get_json(client, &format!("{}/bridge/supported", api_url)).await {
        add_bridge_values(&mut set, &json);
    }
    if let Ok(json) = get_json(client, &format!("{}/crosschain/list-bridges", api_url)).await {
        add_bridge_values(&mut set, &json);
    }

    set.into_iter().collect()
}

async fn fetch_peers(client: &Client, api_url: &str) -> Vec<String> {
    let mut set = BTreeSet::new();
    if let Ok(json) = get_json(client, &format!("{}/node/peers", api_url)).await {
        add_peer_values(&mut set, &json);
    }
    set.into_iter().collect()
}

async fn fetch_stations(client: &Client, api_url: &str) -> Vec<String> {
    let mut set = BTreeSet::new();

    let nearby = format!("{}/weather/stations?lat=39.5&lng=-98.35&limit=64", api_url);
    if let Ok(json) = get_json(client, &nearby).await {
        add_station_values(&mut set, &json);
    }
    if let Ok(json) = get_json(client, &format!("{}/geographic/stations", api_url)).await {
        add_station_values(&mut set, &json);
    }

    set.into_iter().collect()
}

async fn get_json(client: &Client, url: &str) -> Result<Value> {
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("non-success response: {}", response.status());
    }
    Ok(response.json::<Value>().await?)
}

fn add_bridge_values(set: &mut BTreeSet<String>, json: &Value) {
    if let Some(array) = json.as_array() {
        for item in array {
            if let Some(value) = item.as_str() {
                set.insert(value.to_string());
            }
            if let Some(value) = item.get("name").and_then(Value::as_str) {
                set.insert(value.to_string());
            }
            if let Some(value) = item.get("id").and_then(Value::as_str) {
                set.insert(value.to_string());
            }
            if let Some(value) = item.get("chain").and_then(Value::as_str) {
                set.insert(value.to_string());
            }
        }
    }

    if let Some(obj) = json.as_object() {
        for key in ["bridges", "data", "supported", "supported_chains"] {
            if let Some(value) = obj.get(key) {
                add_bridge_values(set, value);
            }
        }
    }
}

fn add_peer_values(set: &mut BTreeSet<String>, json: &Value) {
    if let Some(array) = json.as_array() {
        for item in array {
            if let Some(value) = item.as_str() {
                set.insert(value.to_string());
            }
            for key in ["peer", "address", "id", "node"] {
                if let Some(value) = item.get(key).and_then(Value::as_str) {
                    set.insert(value.to_string());
                }
            }
        }
    }

    if let Some(obj) = json.as_object() {
        for key in ["peers", "data"] {
            if let Some(value) = obj.get(key) {
                add_peer_values(set, value);
            }
        }
    }
}

fn add_station_values(set: &mut BTreeSet<String>, json: &Value) {
    if let Some(array) = json.as_array() {
        for item in array {
            if let Some(value) = item.as_str() {
                set.insert(value.to_string());
            }
            for key in ["stationId", "station_id", "id", "name"] {
                if let Some(value) = item.get(key).and_then(Value::as_str) {
                    set.insert(value.to_string());
                }
            }
        }
    }

    if let Some(obj) = json.as_object() {
        for key in ["stations", "data"] {
            if let Some(value) = obj.get(key) {
                add_station_values(set, value);
            }
        }
    }
}

fn cache_path(config_path: Option<&PathBuf>) -> Result<PathBuf> {
    if let Some(config) = config_path {
        let parent = config
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid config path: {}", config.display()))?;
        return Ok(parent.join("completion-cache.json"));
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("home directory not found"))?;
    Ok(home.join(".wrfcoin").join("completion-cache.json"))
}

fn save_completion_cache(config_path: Option<&PathBuf>, cache: &CompletionCache) -> Result<()> {
    let path = cache_path(config_path)?;
    if let Some(parent) = Path::new(&path).parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    let raw = serde_json::to_string_pretty(cache)?;
    fs::write(path, raw)?;
    Ok(())
}
