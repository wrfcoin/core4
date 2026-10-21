use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;
use std::collections::VecDeque;
use std::time::Instant;
use wrfcoin_cli::{ApiClient, ApiConfig};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Weather,
    Network,
    Wallet,
}

impl Tab {
    pub fn all() -> &'static [Tab] {
        &[Tab::Dashboard, Tab::Weather, Tab::Network, Tab::Wallet]
    }

    pub fn title(&self) -> &'static str {
        match self {
            Tab::Dashboard => "Dashboard",
            Tab::Weather => "Weather",
            Tab::Network => "Network",
            Tab::Wallet => "Wallet",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Tab::Dashboard => 0,
            Tab::Weather => 1,
            Tab::Network => 2,
            Tab::Wallet => 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum ConnectionStatus {
    Connected,
    Degraded,
    Disconnected,
}

impl ConnectionStatus {
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            ConnectionStatus::Connected => "Connected",
            ConnectionStatus::Degraded => "Degraded",
            ConnectionStatus::Disconnected => "Disconnected",
        }
    }
}

pub struct App {
    pub current_tab: Tab,
    pub interval: u64,
    pub station_id: Option<String>,
    pub address: Option<String>,
    pub connection_status: ConnectionStatus,
    pub last_refresh: Instant,

    // Dashboard data
    pub blockchain_status: Option<Value>,
    pub mining_stats: Option<Value>,
    pub hashrate_history: VecDeque<u64>,

    // Weather data
    pub weather_data: Option<Value>,

    // Network data
    pub peers: Option<Value>,
    pub bridge_status: Option<Value>,

    // Wallet data
    pub wallet_data: Option<Value>,
    pub wallet_transactions: Option<Value>,

    // Error tracking per subsystem
    pub errors: Vec<String>,
}

impl App {
    pub fn new(interval: u64, station_id: Option<String>, address: Option<String>) -> Self {
        Self {
            current_tab: Tab::Dashboard,
            interval,
            station_id,
            address,
            connection_status: ConnectionStatus::Disconnected,
            last_refresh: Instant::now() - std::time::Duration::from_secs(interval + 1),

            blockchain_status: None,
            mining_stats: None,
            hashrate_history: VecDeque::with_capacity(60),

            weather_data: None,

            peers: None,
            bridge_status: None,

            wallet_data: None,
            wallet_transactions: None,

            errors: Vec::new(),
        }
    }

    pub fn should_refresh(&self) -> bool {
        self.last_refresh.elapsed().as_secs() >= self.interval
    }

    pub async fn fetch_data(&mut self) {
        self.last_refresh = Instant::now();
        self.errors.clear();

        let client = match ApiClient::new(ApiConfig::default()) {
            Ok(c) => c,
            Err(e) => {
                self.connection_status = ConnectionStatus::Disconnected;
                self.errors.push(format!("API client error: {}", e));
                return;
            },
        };

        let mut success_count = 0u32;
        let mut total_count = 0u32;

        // Dashboard: blockchain status
        total_count += 1;
        match client.get_blockchain_status().await {
            Ok(status) => {
                self.blockchain_status = Some(serde_json::to_value(&status).unwrap_or_default());
                success_count += 1;
            },
            Err(e) => {
                self.errors.push(format!("Blockchain status: {}", e));
            },
        }

        // Dashboard: mining stats
        total_count += 1;
        match client.get_mining_stats().await {
            Ok(stats) => {
                let hashrate = stats.hashrate;
                if self.hashrate_history.len() >= 60 {
                    self.hashrate_history.pop_front();
                }
                self.hashrate_history.push_back(hashrate);
                self.mining_stats = Some(serde_json::to_value(&stats).unwrap_or_default());
                success_count += 1;
            },
            Err(e) => {
                self.errors.push(format!("Mining stats: {}", e));
            },
        }

        // Network: peers
        total_count += 1;
        match client.get_node_peers().await {
            Ok(peers) => {
                self.peers = Some(peers);
                success_count += 1;
            },
            Err(e) => {
                self.errors.push(format!("Peers: {}", e));
            },
        }

        // `/bridge/safety/status` requires `ApiRole::Reader` and 401s without
        // a bearer token, so an unauthenticated client would burn the whole
        // retry loop (~10s per refresh) on a guaranteed failure. Gate on
        // token presence (WRFCOIN_OPERATOR_TOKEN / WRFCOIN_READER_TOKEN via
        // ApiConfig): with a token the call is authenticated and can succeed;
        // without one it is skipped, not attempted-and-failed.
        if client.has_bearer_token() {
            total_count += 1;
            match client.get_bridge_status().await {
                Ok(bridges) => {
                    self.bridge_status = Some(bridges);
                    success_count += 1;
                },
                Err(e) => {
                    self.errors.push(format!("Bridge status: {}", e));
                },
            }
        } else {
            self.bridge_status =
                Some(serde_json::json!({"status": "disabled", "reason": "no bearer token configured"}));
        }

        // Weather: station data (if station_id provided)
        if let Some(ref sid) = self.station_id {
            total_count += 1;
            match client.get_weather_station_data(sid).await {
                Ok(data) => {
                    self.weather_data = Some(data);
                    success_count += 1;
                },
                Err(e) => {
                    self.errors.push(format!("Weather data: {}", e));
                },
            }
        }

        // Wallet data (if address provided)
        if let Some(ref addr) = self.address {
            total_count += 1;
            match client.get_wallet(addr).await {
                Ok(wallet) => {
                    self.wallet_data = Some(serde_json::to_value(&wallet).unwrap_or_default());
                    success_count += 1;
                },
                Err(e) => {
                    self.errors.push(format!("Wallet: {}", e));
                },
            }

            total_count += 1;
            match client.get_wallet_transactions(addr, 1, 10).await {
                Ok(txs) => {
                    self.wallet_transactions = Some(serde_json::to_value(&txs).unwrap_or_default());
                    success_count += 1;
                },
                Err(e) => {
                    self.errors.push(format!("Wallet transactions: {}", e));
                },
            }
        }

        // Determine connection status
        self.connection_status = if success_count == total_count {
            ConnectionStatus::Connected
        } else if success_count > 0 {
            ConnectionStatus::Degraded
        } else {
            ConnectionStatus::Disconnected
        };
    }

    /// Handle key events. Returns true if the app should quit.
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => true,
            KeyCode::Tab | KeyCode::Char('l') => {
                self.next_tab();
                false
            },
            KeyCode::BackTab | KeyCode::Char('h') => {
                self.prev_tab();
                false
            },
            KeyCode::Char('1') => {
                self.current_tab = Tab::Dashboard;
                false
            },
            KeyCode::Char('2') => {
                self.current_tab = Tab::Weather;
                false
            },
            KeyCode::Char('3') => {
                self.current_tab = Tab::Network;
                false
            },
            KeyCode::Char('4') => {
                self.current_tab = Tab::Wallet;
                false
            },
            KeyCode::Char('r') => {
                // Force next refresh by resetting timer
                self.last_refresh =
                    Instant::now() - std::time::Duration::from_secs(self.interval + 1);
                false
            },
            _ => false,
        }
    }

    fn next_tab(&mut self) {
        let tabs = Tab::all();
        let idx = self.current_tab.index();
        self.current_tab = tabs[(idx + 1) % tabs.len()];
    }

    fn prev_tab(&mut self) {
        let tabs = Tab::all();
        let idx = self.current_tab.index();
        self.current_tab = tabs[(idx + tabs.len() - 1) % tabs.len()];
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use wrfcoin_cli::{ApiClient, ApiConfig};

    fn client_with_token(token: Option<&str>) -> ApiClient {
        ApiClient::new(ApiConfig {
            base_url: "http://127.0.0.1:1".to_string(),
            timeout: Duration::from_millis(10),
            retry_attempts: 0,
            retry_delay: Duration::from_millis(1),
            explain_mode: false,
            bearer_token: token.map(str::to_string),
        })
        .expect("ApiClient::new")
    }

    #[test]
    fn bridge_query_gates_on_token_presence_not_an_env_flag() {
        // The bridge route requires ApiRole::Reader; without a token every
        // call 401s through the retry loop. The refresh gate is therefore
        // token presence — there is no separate opt-in env var whose "on"
        // state could only ever fail.
        assert!(client_with_token(Some("reader-token")).has_bearer_token()); // secret-leak-ok: test-only fake literal; assert! prints the expression source, not a live credential
        assert!(!client_with_token(None).has_bearer_token()); // secret-leak-ok: asserts the ABSENCE of a token; no credential value exists here
    }
}
