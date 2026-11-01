use crate::errors::{CliError, CliErrorCode};
use anyhow::{Result, anyhow};
use log::{debug, error, warn};
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

fn transport_error(err: reqwest::Error) -> anyhow::Error {
    let code = if err.is_timeout() || err.is_connect() {
        CliErrorCode::ConnectionError
    } else {
        CliErrorCode::UserError
    };
    CliError::new(code, format!("API request failed: {err}")).into()
}

/// API configuration
///
/// `Debug` is hand-implemented below rather than derived: this struct holds a
/// bearer token, and a derived `Debug` renders it in full anywhere the config
/// reaches a `{:?}` — a log line, an `anyhow` context, a panic payload.
#[derive(Clone)] // secret-leak-ok: Clone moves the token between owners without rendering it; Debug is hand-implemented below and redacts bearer_token
pub struct ApiConfig {
    /// API base URL
    pub base_url: String,
    /// API timeout
    pub timeout: Duration,
    /// Number of retry attempts
    pub retry_attempts: u32,
    /// Delay between retry attempts
    pub retry_delay: Duration,
    /// Explain routing/retry decisions
    pub explain_mode: bool,
    /// Bearer token for authenticated launch_api routes (e.g.
    /// `/bridge/safety/status`, which requires `ApiRole::Reader` and 401s
    /// without one). `None` means unauthenticated: callers must skip
    /// role-gated routes rather than burn the retry loop on guaranteed 401s.
    pub bearer_token: Option<String>,
}

impl std::fmt::Debug for ApiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiConfig")
            .field("base_url", &self.base_url)
            .field("timeout", &self.timeout)
            .field("retry_attempts", &self.retry_attempts)
            .field("retry_delay", &self.retry_delay)
            .field("explain_mode", &self.explain_mode)
            // Presence is operationally useful (it decides whether role-gated
            // routes are attempted at all); the value never is.
            .field(
                "bearer_token",
                &self.bearer_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl ApiConfig {
    /// Base URL used when neither a flag nor `WRFCOIN_API_URL` names one.
    pub const DEFAULT_BASE_URL: &'static str = "http://127.0.0.1:8556";
}

/// Which HTTP service a subcommand family talks to (#3204).
///
/// `wrfcoin compute|storage|iot|aiml` were written against **backend's**
/// per-family microservices, not the node. Before #3204 they inherited the
/// node default (`127.0.0.1:8556`) and 404ed against routes `launch_api`
/// never served. Each family now has its own env var and a default that
/// points at the backend service's documented port, with backend's `/api`
/// prefix baked into the base URL so the command paths line up with the
/// routes backend actually registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceTarget {
    /// backend `server/compute_networks_server.ts` (port 6000)
    Compute,
    /// backend `server/storage_networks_server.js` (port 7000)
    Storage,
    /// backend `server/iot_sensor_networks_server*.ts` (port 3002)
    Iot,
    /// backend `server/ai_ml_networks_server*.js` (port 6001)
    AiMl,
}

impl ServiceTarget {
    /// Environment variable that overrides this family's base URL.
    pub const fn env_var(self) -> &'static str {
        match self {
            Self::Compute => "WRFCOIN_COMPUTE_API_URL",
            Self::Storage => "WRFCOIN_STORAGE_API_URL",
            Self::Iot => "WRFCOIN_IOT_API_URL",
            Self::AiMl => "WRFCOIN_AIML_API_URL",
        }
    }

    /// Default base URL: backend's service port from its own service
    /// registry (`backend/server/CLAUDE.md`), plus the `/api` prefix all
    /// four servers mount their routes under.
    pub const fn default_base_url(self) -> &'static str {
        match self {
            Self::Compute => "http://127.0.0.1:6000/api",
            Self::Storage => "http://127.0.0.1:7000/api",
            Self::Iot => "http://127.0.0.1:3002/api",
            Self::AiMl => "http://127.0.0.1:6001/api",
        }
    }

    /// Human label used in output and error messages.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Compute => "backend compute-networks service",
            Self::Storage => "backend storage-networks service",
            Self::Iot => "backend iot-sensor-networks service",
            Self::AiMl => "backend ai-ml-networks service",
        }
    }

    /// Honest status of the target as of 2026-09 (epic wrfcoin/infra#943).
    /// Surfaced in every response so a correctly-routed call cannot be
    /// mistaken for a live integration.
    pub const fn status_note(self) -> &'static str {
        match self {
            Self::Compute => {
                "backend#1118: generic compute submit/jobs/metrics answer 501 NOT_IMPLEMENTED (no provider client is wired); /compute/networks is a design catalog. Real routes on that service are /compute/forecast-jobs (backend#47) and /compute/tasks/* (backend#57)."
            },
            Self::Storage => {
                "backend#1117 (blocked on storage-providers#65 / infra#944): the storage service is still a simulation — responses are not backed by IPFS/Filecoin/Arweave."
            },
            Self::Iot => {
                "the iot service exposes /iot/networks and /iot/metrics only; device register/read/coverage have no backend route yet and will report NOT_FOUND."
            },
            Self::AiMl => {
                "the ai-ml service serves /aiml/networks/status and /ai/inference; treat results as unverified until the backend marks them real."
            },
        }
    }

    /// JSON block attached to every service-family command result.
    pub fn describe(self, config: &ApiConfig) -> Value {
        json!({
            "service": self.label(),
            "base_url": config.base_url,
            "env_var": self.env_var(),
            "note": self.status_note(),
        })
    }

    /// Attach [`Self::describe`] to a command's JSON output under `"target"`.
    pub fn attach(self, config: &ApiConfig, out: &mut Value) {
        if let Some(map) = out.as_object_mut() {
            map.insert("target".to_string(), self.describe(config));
        }
    }
}

impl ApiConfig {
    /// Config for a backend service-family client (#3204).
    ///
    /// Precedence: the family env var, else the backend service default.
    /// `WRFCOIN_API_URL` is deliberately NOT consulted — it names the node,
    /// and inheriting it is exactly the misrouting this fixes. The node's
    /// operator/reader bearer token is likewise not forwarded: backend
    /// services are a different trust domain.
    pub fn for_service(target: ServiceTarget) -> Self {
        let base_url = std::env::var(target.env_var())
            .ok()
            .map(|url| url.trim().trim_end_matches('/').to_string())
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| target.default_base_url().to_string());

        Self {
            base_url,
            bearer_token: None,
            ..Self::default()
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        let base_url = std::env::var("WRFCOIN_API_URL")
            .ok()
            .filter(|url| !url.trim().is_empty())
            .unwrap_or_else(|| Self::DEFAULT_BASE_URL.to_string());

        Self {
            base_url,
            timeout: Duration::from_secs(30),
            retry_attempts: 5,
            retry_delay: Duration::from_secs(2),
            explain_mode: std::env::var("WRFCOIN_CLI_WHY")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            // Same precedence as `wrfcoin setup`: the operator token first,
            // then the read-only token that role-gated read routes accept.
            bearer_token: ["WRFCOIN_OPERATOR_TOKEN", "WRFCOIN_READER_TOKEN"]
                .iter()
                .find_map(|var| {
                    std::env::var(var)
                        .ok()
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty())
                }),
        }
    }
}

/// API client for Wrfcoin
pub struct ApiClient {
    /// HTTP client
    client: Client,
    /// API configuration
    config: ApiConfig,
}

/// Wallet information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wallet {
    /// Wallet address
    pub address: String,
    /// Wallet balance
    pub balance: f64,
    /// Wallet creation timestamp
    pub created_at: String,
    /// Number of transactions
    pub transactions: u64,
    /// Staking status
    pub staking: bool,
    /// Staking amount
    pub staking_amount: Option<f64>,
}

/// Consensus-backed account values returned by `GET /addresses/{address}`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddressInfo {
    pub address: String,
    pub balance: String,
    pub spendable_balance: String,
    pub stake_locked: String,
    pub earned_balance_total: String,
    #[serde(default)]
    pub nonce: Option<String>,
}

/// The fee in force at the next block, from a `/network/info` body (bare or
/// wrapped in `data`). `None` when absent, inactive or zero.
pub fn parse_transfer_fee(info: &Value) -> Option<u64> {
    let fee = info.get("transfer_fee").or_else(|| info.get("data")?.get("transfer_fee"))?;
    if fee.get("active").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    fee.get("base_units")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|units| *units > 0)
}

/// Acceptance receipt returned by `POST /transactions/submit`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubmitAccepted {
    pub accepted: bool,
    pub tx_id: String,
    #[serde(default)]
    pub tx_type: Option<Value>,
}

/// Transaction information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    /// Transaction ID
    pub id: String,
    /// Transaction block height
    pub block_height: Option<u64>,
    /// Transaction timestamp
    pub timestamp: String,
    /// Sender address
    pub from: String,
    /// Recipient address
    pub to: String,
    /// Transaction amount
    pub amount: f64,
    /// Transaction fee
    pub fee: f64,
    /// Transaction status
    pub status: String,
    /// Transaction type
    pub tx_type: String,
    /// Transaction hash
    pub hash: String,
}

/// Block information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    /// Block hash
    pub hash: String,
    /// Block height
    pub height: u64,
    /// Block timestamp
    pub timestamp: String,
    /// Previous block hash
    pub prev_hash: String,
    /// Merkle root
    pub merkle_root: String,
    /// Block size
    pub size: u64,
    /// Block nonce
    pub nonce: u64,
    /// Block difficulty
    pub difficulty: u64,
    /// Number of transactions
    pub transactions_count: u64,
    /// Block type
    pub block_type: String,
}

/// Mining statistics
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MiningStats {
    /// Current network hashrate
    pub hashrate: u64,
    /// Current network difficulty
    pub difficulty: u64,
    /// Next difficulty adjustment (block height)
    pub next_adjustment: u64,
    /// Current miners count
    pub miners_count: u64,
    /// Last block height
    pub last_block_height: u64,
    /// Average block time (seconds)
    pub avg_block_time: f64,
}

/// Network statistics
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkStats {
    /// Current block height
    pub block_height: u64,
    /// Total transactions count
    pub transactions_count: u64,
    /// Active nodes count
    pub nodes_count: u64,
    /// Network version
    pub version: String,
    /// Network name
    pub network: String,
}

/// NFT information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NFT {
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
    /// NFT metadata
    pub metadata: Value,
    /// NFT type
    pub nft_type: String,
    /// Computational NFT status (if applicable)
    pub computational_status: Option<String>,
}

/// API paged response
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PagedResponse<T> {
    /// Response data
    pub data: Vec<T>,
    /// Response metadata
    pub meta: PageMetadata,
}

/// Page metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageMetadata {
    /// Current page
    pub page: u64,
    /// Page size
    pub page_size: u64,
    /// Total items
    pub total: u64,
    /// Total pages
    pub total_pages: u64,
}

/// Fee estimate for a transaction
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeeEstimate {
    pub base_fee: f64,
    pub priority_fee: f64,
    pub total_fee: f64,
    pub estimated_confirmation_seconds: u64,
}

impl FeeEstimate {
    /// Compute a local fee estimate when the API is unavailable.
    /// Uses 0.5% of the amount with a minimum of 0.001 WRF.
    pub fn local_estimate(amount: f64) -> Self {
        let fee = (amount * 0.005).max(0.001);
        Self {
            base_fee: fee * 0.8,
            priority_fee: fee * 0.2,
            total_fee: fee,
            estimated_confirmation_seconds: 15,
        }
    }
}

/// Swap quote returned by the DEX
#[derive(Clone, Debug, Serialize, Deserialize)] // secret-leak-ok: from_token/to_token are DEX asset symbols (WRF, USDC), not credentials
pub struct SwapQuote {
    pub from_token: String,
    pub to_token: String,
    pub input_amount: f64,
    pub output_amount: f64,
    pub exchange_rate: f64,
    pub price_impact: f64,
    pub fee: f64,
    pub route: Vec<String>,
    pub expires_at: String,
}

/// Token price information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenPrice {
    pub pair: String,
    pub price: f64,
    pub change_24h: f64,
    pub volume_24h: f64,
    pub last_updated: String,
}

/// Liquidity pool information
#[derive(Clone, Debug, Serialize, Deserialize)] // secret-leak-ok: token_a/token_b are DEX asset symbols, not credentials
pub struct PoolLiquidity {
    pub pair: String,
    pub token_a: String,
    pub token_b: String,
    pub reserve_a: f64,
    pub reserve_b: f64,
    pub total_liquidity_usd: f64,
    pub volume_24h: f64,
    pub fee_tier: f64,
    pub apy: f64,
}

/// Extract the human reason from backend's honest-gap 501 envelope
/// (`server/utils/honest-gap.js`, backend#1118):
/// `{ success:false, unavailable:true, reason, error:{ code:"NOT_IMPLEMENTED", ... } }`.
/// Returns `None` for any other 501 body so it is reported verbatim.
pub fn honest_gap_reason(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let unavailable = value
        .get("unavailable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let code = value
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str);
    if !unavailable && code != Some("NOT_IMPLEMENTED") {
        return None;
    }
    let reason = value.get("reason").and_then(Value::as_str).or_else(|| {
        value
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
    })?;
    let see = value
        .get("error")
        .and_then(|e| e.get("see"))
        .and_then(Value::as_str);
    Some(match see {
        Some(see) => format!("{reason} [see {see}]"),
        None => reason.to_string(),
    })
}

impl ApiClient {
    /// Authenticated receipt submission must never forward credentials through redirects.
    pub fn new_without_redirects(config: ApiConfig) -> Result<Self> {
        let client = ClientBuilder::new()
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| CliError::new(CliErrorCode::UserError, "API client configuration invalid"))?;
        Ok(Self { client, config })
    }

    /// Create a new API client
    pub fn new(config: ApiConfig) -> Result<Self> {
        // Build HTTP client
        let client = ClientBuilder::new()
            .timeout(config.timeout)
            .build()
            .map_err(|err| {
                CliError::new(
                    CliErrorCode::UserError,
                    format!("API client configuration is invalid: {err}"),
                )
            })?;

        Ok(Self { client, config })
    }

    /// Whether this client can authenticate to role-gated launch_api routes.
    /// Callers use this to SKIP such routes entirely when unauthenticated —
    /// `/bridge/safety/status` requires `ApiRole::Reader`, so calling it
    /// without a token can only ever 401 through the whole retry loop.
    pub fn has_bearer_token(&self) -> bool {
        self.config.bearer_token.is_some()
    }

    /// Make a GET request
    pub async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: Option<&[(&str, &str)]>,
    ) -> Result<T> {
        self.request_with_retry("GET", path, query, None).await
    }

    /// Make a POST request
    pub async fn post<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        self.request_with_retry("POST", path, None, body).await
    }

    /// Credential-bearing receipt requests never retry or expose untrusted bodies,
    /// URLs, transport details, or deserialization diagnostics.
    pub async fn receipt_request(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let url = format!("{}{}", self.config.base_url, path);
        let mut request = match method {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            _ => return Err(anyhow!("RECEIPT_METHOD")),
        };
        if let Some(body) = body { request = request.json(&body); }
        if let Some(token) = &self.config.bearer_token { request = request.bearer_auth(token); }
        let mut response = request.send().await.map_err(|_| anyhow!("RECEIPT_TRANSPORT"))?;
        if !response.status().is_success() {
            return Err(anyhow!("RECEIPT_HTTP_{}", response.status().as_u16()));
        }
        const CAP: usize = 40 * 1024;
        if response.content_length().is_some_and(|size| size > CAP as u64) {
            return Err(anyhow!("RECEIPT_RESPONSE_BOUND"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| anyhow!("RECEIPT_RESPONSE"))? {
            if chunk.len() > CAP.saturating_sub(bytes.len()) {
                return Err(anyhow!("RECEIPT_RESPONSE_BOUND"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| anyhow!("RECEIPT_RESPONSE"))?;
        Ok(value.get("data").cloned().unwrap_or(value))
    }

    /// Make a PUT request
    pub async fn put<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        self.request_with_retry("PUT", path, None, body).await
    }

    /// Make a DELETE request
    pub async fn delete<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        self.request_with_retry("DELETE", path, None, None).await
    }

    /// Make a request with retry logic
    async fn request_with_retry<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        path: &str,
        query: Option<&[(&str, &str)]>,
        body: Option<Value>,
    ) -> Result<T> {
        let url = format!("{}{}", self.config.base_url, path);
        let max_attempts = self.config.retry_attempts + 1;

        if self.config.explain_mode {
            crate::why_report::record_route(
                method,
                &url,
                &self.config.base_url,
                max_attempts,
                self.config.retry_delay.as_millis() as u64,
            );
        }

        for attempt in 1..=max_attempts {
            match self.request(method, &url, query, body.clone()).await {
                Ok(response) => {
                    return Ok(response);
                },
                Err(err) => {
                    if attempt < max_attempts {
                        warn!("API request failed (attempt {attempt}/{max_attempts}): {err}");
                        #[allow(clippy::excessive_nesting)]
                        if self.config.explain_mode {
                            crate::why_report::record_attempt(
                                attempt,
                                &err.to_string(),
                                self.config.retry_delay.as_millis() as u64,
                            );
                        }
                        sleep(self.config.retry_delay).await;
                    } else {
                        error!("API request failed after {attempt} attempts: {err}");
                        #[allow(clippy::excessive_nesting)]
                        if self.config.explain_mode {
                            crate::why_report::record_final_failure(attempt, &err.to_string());
                        }
                        return Err(err);
                    }
                },
            }
        }

        // This should never happen due to the loop above
        Err(anyhow!("API request failed - retry logic error"))
    }

    /// Send a non-idempotent request exactly once. A transport failure after
    /// the node accepts a transfer is ambiguous, so retrying could submit the
    /// same signed transfer more than once.
    async fn request_once<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        let url = format!("{}{}", self.config.base_url, path);
        if self.config.explain_mode {
            crate::why_report::record_route(method, &url, &self.config.base_url, 1, 0);
        }
        let result = self.request(method, &url, None, body).await;
        if let Err(error) = &result {
            if self.config.explain_mode {
                crate::why_report::record_final_failure(1, &error.to_string());
            }
        }
        result
    }

    /// Make a request
    async fn request<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        url: &str,
        query: Option<&[(&str, &str)]>,
        body: Option<Value>,
    ) -> Result<T> {
        let mut request = match method {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "DELETE" => self.client.delete(url),
            _ => return Err(anyhow!("Unsupported HTTP method: {method}")),
        };

        // Add query parameters
        if let Some(q) = query {
            request = request.query(q);
        }

        // Add body for POST/PUT
        if let Some(b) = body {
            request = request.json(&b);
        }

        // Authenticate when a token is configured; role-gated routes 401
        // otherwise and callers are expected to have skipped them.
        if let Some(token) = &self.config.bearer_token {
            request = request.bearer_auth(token);
        }

        // Send request
        debug!("Making {method} request to {url}");
        let response = request.send().await.map_err(transport_error)?;

        // Check status code
        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            let code = match status.as_u16() {
                401 | 403 => CliErrorCode::AuthError,
                // 501 with the honest-gap envelope (backend#1118) means the
                // route exists but the capability behind it is not wired;
                // classify with NotFound (exit 7) rather than UserError so
                // agents can tell "target cannot serve this" from a bad call.
                404 | 501 => CliErrorCode::NotFound,
                429 => CliErrorCode::RateLimitError,
                400 | 422 => CliErrorCode::ValidationError,
                _ => CliErrorCode::UserError,
            };
            let message = match status.as_u16() {
                404 => format!(
                    "API request failed: {status} at {method} {url} — no such route on this service \
                     (the target may be the wrong service or the route may not exist yet; \
                     see the WrfcoinCLI README 'Service-family subcommands')"
                ),
                501 => match honest_gap_reason(&error_text) {
                    Some(reason) => format!(
                        "Target reports this capability is not wired: {reason} ({method} {url})"
                    ),
                    None => {
                        format!("API request failed: {status} at {method} {url} - {error_text}")
                    },
                },
                _ => format!("API request failed: {status} - {error_text}"),
            };
            return Err(CliError::new(code, message).into());
        }

        // Parse response. Prefer envelope.data when available.
        let response_body = response.text().await.map_err(transport_error)?;
        let parsed_value: Value = serde_json::from_str(&response_body).map_err(|err| {
            CliError::new(
                CliErrorCode::UserError,
                format!("Failed to parse API response: {err}"),
            )
        })?;

        if let Some(data) = parsed_value.get("data") {
            if let Ok(parsed_data) = serde_json::from_value::<T>(data.clone()) {
                return Ok(parsed_data);
            }
        }

        match serde_json::from_value::<T>(parsed_value.clone()) {
            Ok(parsed) => Ok(parsed),
            Err(err) => {
                error!("Failed to parse API response: {err}");
                error!("Response: {response_body}");
                Err(CliError::new(
                    CliErrorCode::UserError,
                    format!("Failed to parse API response: {err}"),
                )
                .into())
            },
        }
    }

    // ========== Blockchain API ==========

    /// Get blockchain status
    pub async fn get_blockchain_status(&self) -> Result<NetworkStats> {
        let network: Value = self.get("/network/info", None).await?;
        // The legacy backend mounts aggregate statistics at `/api/stats`, while
        // launch_api exposes the equivalent transaction count at
        // `/mining/stats`.  Selecting from the already-validated network shape
        // avoids an expensive retry loop against launch_api's nonexistent
        // `/stats` route on every TUI refresh.
        let stats_path = if network.get("chain_height").is_some() {
            "/mining/stats"
        } else {
            "/stats"
        };
        let stats: Value = self
            .get(stats_path, None)
            .await
            .unwrap_or_else(|_| json!({}));

        let block_height = network
            .get("chain_height")
            .or_else(|| network.get("chainId"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                anyhow!(
                    "network/info response is missing numeric `chain_height` \
                     (launch_api) or `chainId` (legacy backend)"
                )
            })?;
        let network_name = network
            .get("network_id")
            .or_else(|| network.get("networkName"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "network/info response is missing non-empty `network_id` \
                     (launch_api) or `networkName` (legacy backend)"
                )
            })?;
        let version = network
            .get("node_version")
            .or_else(|| network.get("consensusType"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "network/info response is missing non-empty `node_version` \
                     (launch_api) or `consensusType` (legacy backend)"
                )
            })?;

        Ok(NetworkStats {
            block_height,
            transactions_count: stats
                .get("totalTransactions")
                .or_else(|| stats.get("total_transactions"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            nodes_count: network
                .get("peer_count")
                .and_then(Value::as_u64)
                .or_else(|| stats.get("totalNodes").and_then(Value::as_u64))
                .unwrap_or(0),
            version: version.to_string(),
            network: network_name.to_string(),
        })
    }

    /// Get latest blocks
    pub async fn get_latest_blocks(&self, limit: u64) -> Result<Vec<Block>> {
        let path = format!("/chain/blocks?limit={limit}");
        let data: Value = self.get(&path, None).await?;
        let blocks = data
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(parse_block_from_value)
            .collect();
        Ok(blocks)
    }

    /// Get block by hash
    pub async fn get_block_by_hash(&self, hash: &str) -> Result<Block> {
        let path = format!("/chain/block?hash={hash}");
        let data: Value = self.get(&path, None).await?;
        Ok(parse_block_from_value(data))
    }

    /// Get block by height
    pub async fn get_block_by_height(&self, height: u64) -> Result<Block> {
        let path = format!("/chain/block?height={height}");
        let data: Value = self.get(&path, None).await?;
        Ok(parse_block_from_value(data))
    }

    /// Get mining statistics
    pub async fn get_mining_stats(&self) -> Result<MiningStats> {
        let data: Value = self.get("/mining/stats", None).await?;
        #[allow(clippy::excessive_nesting)]
        let parse_u64 = |value: Option<&Value>| -> u64 {
            value
                .and_then(|v| {
                    if let Some(n) = v.as_u64() {
                        Some(n)
                    } else {
                        v.as_str().and_then(|s| {
                            let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
                            digits.parse::<u64>().ok()
                        })
                    }
                })
                .unwrap_or(0)
        };

        Ok(MiningStats {
            hashrate: parse_u64(data.get("hashrate")),
            difficulty: parse_u64(data.get("difficulty")),
            next_adjustment: 0,
            miners_count: 0,
            last_block_height: parse_u64(
                data.get("chain_height").or_else(|| data.get("blocksFound")),
            ),
            avg_block_time: data
                .get("avg_block_time_secs")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        })
    }

    // ========== Wallet API ==========

    /// Read chain account state. Decimal quantities remain strings so no
    /// precision is lost at the CLI boundary.
    pub async fn get_address(&self, address: &str) -> Result<AddressInfo> {
        self.get(&format!("/addresses/{address}"), None).await
    }

    /// Submit one client-authorized transparent transfer to the node.
    ///
    /// `fee` is the burned network fee the user was shown (base units). It is
    /// sent only when known, so a node that charges none receives exactly the
    /// body it always did; the node refuses a value below the chain's fee
    /// (`FEE_BELOW_MINIMUM`) and debits exactly the chain's fee otherwise.
    pub async fn submit_transparent_transfer(
        &self,
        from: &str,
        to: &str,
        amount: u64,
        nonce: u64,
        auth_pubkey: &str,
        auth_signature: &str,
        fee: Option<u64>,
    ) -> Result<SubmitAccepted> {
        self.submit_transparent_transfer_with_security_key(
            from,
            to,
            amount,
            nonce,
            auth_pubkey,
            auth_signature,
            fee,
            None,
        )
        .await
    }

    /// The burned transfer fee a send would pay in the next block, from the
    /// node's `/network/info` `transfer_fee` object. One attempt, no retry:
    /// this only feeds the preview.
    ///
    /// - `Ok(None)`: the node charges no fee (or predates the fee and does
    ///   not report one, which also means it charges none).
    /// - `Ok(Some(units))`: the fee is in force at the next block.
    /// - `Err`: the node could not be asked; the caller must not claim "no fee".
    pub async fn get_transfer_fee(&self) -> Result<Option<u64>> {
        let info: Value = self.request_once("GET", "/network/info", None).await?;
        Ok(parse_transfer_fee(&info))
    }

    /// [`Self::submit_transparent_transfer`] carrying the account's
    /// security-key assertion (`security_key_spend`) when it has one.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_transparent_transfer_with_security_key(
        &self,
        from: &str,
        to: &str,
        amount: u64,
        nonce: u64,
        auth_pubkey: &str,
        auth_signature: &str,
        fee: Option<u64>,
        security_key_auth: Option<
            &wrfcoin_shared_protocol::accounts::security_key::SecurityKeyAssertionWire,
        >,
    ) -> Result<SubmitAccepted> {
        let mut body = json!({
            "from": from,
            "to": to,
            "amount": amount,
            "nonce": nonce.to_string(),
            "transparent_opt_out": true,
            "transparent_opt_out_reason": "cli",
            "auth_pubkey": auth_pubkey,
            "auth_signature": auth_signature,
        });
        if let Some(fee) = fee {
            body["fee"] = json!(fee);
        }
        if let Some(proof) = security_key_auth {
            body["security_key_auth"] = serde_json::to_value(proof)?;
        }
        self.request_once("POST", "/transactions/submit", Some(body)).await
    }

    /// `GET /accounts/{address}/security-key` (`security_key_spend`).
    pub async fn get_security_key_status(&self, address: &str) -> Result<Value> {
        self.request_once("GET", &format!("/accounts/{address}/security-key"), None)
            .await
    }

    /// `POST /accounts/security-key/actions` (`security_key_spend`).
    pub async fn submit_security_key_action(&self, payload: Value) -> Result<Value> {
        self.request_once("POST", "/accounts/security-key/actions", Some(payload))
            .await
    }

    /// Address history is intentionally node-backed. Until the node serves
    /// this route its 404 is surfaced as the CLI's explicit unavailable error.
    pub async fn get_address_transactions(&self, address: &str) -> Result<Value> {
        self.get(&format!("/addresses/{address}/transactions"), None)
            .await
    }

    /// Get wallet by address
    pub async fn get_wallet(&self, address: &str) -> Result<Wallet> {
        self.get(&format!("/wallet/{address}"), None).await
    }

    /// Create a new wallet
    pub async fn create_wallet(&self, label: Option<&str>) -> Result<Wallet> {
        let body = json!({
            "label": label
        });
        self.post("/wallet/create", Some(body)).await
    }

    /// Get wallet transactions
    pub async fn get_wallet_transactions(
        &self,
        address: &str,
        page: u64,
        page_size: u64,
    ) -> Result<PagedResponse<Transaction>> {
        let page_str = page.to_string();
        let page_size_str = page_size.to_string();
        let query = [
            ("address", address),
            ("page", page_str.as_str()),
            ("limit", page_size_str.as_str()),
        ];
        let payload: Value = self.get("/transactions", Some(&query)).await?;

        let tx_data = payload
            .get("data")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![]));
        let meta = payload.get("pagination").cloned().unwrap_or_else(
            || json!({"page": page, "limit": page_size, "total": 0, "totalPages": 0}),
        );

        Ok(PagedResponse {
            data: serde_json::from_value(tx_data)?,
            meta: PageMetadata {
                page: meta.get("page").and_then(Value::as_u64).unwrap_or(page),
                page_size: meta
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(page_size),
                total: meta.get("total").and_then(Value::as_u64).unwrap_or(0),
                total_pages: meta.get("totalPages").and_then(Value::as_u64).unwrap_or(0),
            },
        })
    }

    // ========== Transaction API ==========

    /// Get transaction by ID
    pub async fn get_transaction(&self, tx_id: &str) -> Result<Transaction> {
        self.get(&format!("/transactions/{tx_id}"), None).await
    }

    /// Send a transaction
    pub async fn send_transaction(
        &self,
        from: &str,
        to: &str,
        amount: f64,
        fee: f64,
        signature: &str,
    ) -> Result<Transaction> {
        let body = json!({
            "from": from,
            "to": to,
            "amount": amount,
            "fee": fee,
            "signature": signature
        });

        self.post("/transactions", Some(body)).await
    }

    /// Get pending transactions
    pub async fn get_pending_transactions(&self) -> Result<Vec<Transaction>> {
        self.get("/chain/pending-transactions", None).await
    }

    // ========== NFT API ==========

    /// Get NFT by ID
    pub async fn get_nft(&self, nft_id: &str) -> Result<NFT> {
        self.get(&format!("/nft/{nft_id}"), None).await
    }

    /// Get NFTs by owner
    pub async fn get_nfts_by_owner(
        &self,
        owner_address: &str,
        page: u64,
        page_size: u64,
    ) -> Result<PagedResponse<NFT>> {
        let page_str = page.to_string();
        let page_size_str = page_size.to_string();
        let query = [
            ("owner", owner_address),
            ("page", page_str.as_str()),
            ("limit", page_size_str.as_str()),
        ];
        let payload: Value = self.get("/nft/marketplace", Some(&query)).await?;
        let nft_data = payload
            .get("data")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![]));
        let meta = payload.get("pagination").cloned().unwrap_or_else(
            || json!({"page": page, "limit": page_size, "total": 0, "totalPages": 0}),
        );
        Ok(PagedResponse {
            data: serde_json::from_value(nft_data)?,
            meta: PageMetadata {
                page: meta.get("page").and_then(Value::as_u64).unwrap_or(page),
                page_size: meta
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(page_size),
                total: meta.get("total").and_then(Value::as_u64).unwrap_or(0),
                total_pages: meta.get("totalPages").and_then(Value::as_u64).unwrap_or(0),
            },
        })
    }

    /// Create an NFT
    pub async fn create_nft(
        &self,
        name: &str,
        description: &str,
        owner: &str,
        nft_type: &str,
        metadata: Value,
        _signature: &str,
    ) -> Result<NFT> {
        let body = json!({
            "title": name,
            "description": description,
            "computationType": nft_type,
            "creator": owner,
            "reward": 0.1,
            "metadata": metadata
        });
        self.post("/nft/create", Some(body)).await
    }

    /// Transfer an NFT
    pub async fn transfer_nft(
        &self,
        nft_id: &str,
        from: &str,
        to: &str,
        _signature: &str,
    ) -> Result<NFT> {
        let body = json!({
            "from": from,
            "to": to
        });
        self.post(&format!("/nft/{nft_id}/transfer"), Some(body))
            .await
    }

    // ========== Node API ==========

    /// Get node status
    pub async fn get_node_status(&self) -> Result<Value> {
        self.get("/network/info", None).await
    }

    /// Get node peers
    pub async fn get_node_peers(&self) -> Result<Value> {
        self.get("/network/peers", None).await
    }

    // ========== Cross-Chain API ==========

    /// Get cross-chain mining rewards
    pub async fn get_cross_chain_mining_rewards(&self) -> Result<Value> {
        self.get("/mining/rewards", None).await
    }

    /// Get bridge status
    pub async fn get_bridge_status(&self) -> Result<Value> {
        self.get("/bridge/safety/status", None).await
    }

    /// Get supported bridges
    pub async fn get_supported_bridges(&self) -> Result<Vec<Value>> {
        self.get("/bridge/safety/status", None).await
    }

    /// Initiate cross-chain transfer
    pub async fn initiate_cross_chain_transfer(
        &self,
        _from_chain: &str,
        _to_chain: &str,
        amount: f64,
        recipient: &str,
        _signature: &str,
    ) -> Result<Value> {
        let body = json!({
            "to": recipient,
            "amount": amount,
            "token": "WRF"
        });

        self.post("/bridges/transfer", Some(body)).await
    }

    // ========== Privacy Features API ==========

    /// Get privacy features status
    pub async fn get_privacy_features(&self) -> Result<Value> {
        self.get("/privacy/features", None).await
    }

    /// Enable privacy mode
    pub async fn enable_privacy_mode(&self, mode: &str) -> Result<Value> {
        let body = json!({
            "mode": mode,
            "enabled": true
        });

        self.post("/privacy/enable", Some(body)).await
    }

    // ========== IoT Integration API ==========

    /// Get IoT sensor data
    pub async fn get_iot_sensor_data(&self, sensor_id: &str) -> Result<Value> {
        self.get(&format!("/iot/sensors/{sensor_id}"), None).await
    }

    /// Submit IoT sensor reading
    pub async fn submit_sensor_reading(
        &self,
        sensor_id: &str,
        reading_type: &str,
        value: f64,
        timestamp: &str,
        signature: &str,
    ) -> Result<Value> {
        let body = json!({
            "sensor_id": sensor_id,
            "reading_type": reading_type,
            "value": value,
            "timestamp": timestamp,
            "signature": signature
        });

        self.post("/iot/readings", Some(body)).await
    }

    // ========== Computational Networks API ==========

    /// Get compute network status
    pub async fn get_compute_network_status(&self) -> Result<Value> {
        self.get("/compute/status", None).await
    }

    /// Submit compute task
    pub async fn submit_compute_task(
        &self,
        task_type: &str,
        payload: Value,
        priority: u32,
    ) -> Result<Value> {
        let body = json!({
            "taskType": task_type,
            "payload": payload,
            "priority": priority
        });
        self.post("/compute/tasks", Some(body)).await
    }

    // ========== Storage Networks API ==========

    /// Get storage network status
    pub async fn get_storage_network_status(&self) -> Result<Value> {
        self.get("/storage/status", None).await
    }

    /// Store data on distributed network
    pub async fn store_data(
        &self,
        data: &str,
        storage_type: &str,
        redundancy: u32,
    ) -> Result<Value> {
        let body = json!({
            "data": data,
            "storageType": storage_type,
            "redundancy": redundancy
        });
        self.post("/storage/store", Some(body)).await
    }

    // ========== AI/ML Networks API ==========

    /// Get AI/ML network status
    pub async fn get_aiml_network_status(&self) -> Result<Value> {
        self.get("/aiml/status", None).await
    }

    /// Submit ML inference task
    pub async fn submit_ml_inference(
        &self,
        model_id: &str,
        input_data: Value,
        parameters: Option<Value>,
    ) -> Result<Value> {
        let body = json!({
            "modelId": model_id,
            "inputData": input_data,
            "parameters": parameters
        });
        self.post("/aiml/inference", Some(body)).await
    }

    // ========== Weather Data API ==========

    /// Get weather station data
    pub async fn get_weather_station_data(&self, station_id: &str) -> Result<Value> {
        self.get(&format!("/weather/stations/{station_id}"), None)
            .await
    }

    /// Submit weather observation
    pub async fn submit_weather_observation(
        &self,
        station_id: &str,
        observation: Value,
        signature: &str,
    ) -> Result<Value> {
        let sensor_type = observation
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("generic");
        let sensor_value = observation
            .get("value")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let timestamp = observation
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("1970-01-01T00:00:00Z");

        // Build sensor data item, including location if present
        let mut sensor_item = json!({
            "type": sensor_type,
            "value": sensor_value
        });
        if let Some(unit) = observation.get("unit") {
            sensor_item["unit"] = unit.clone();
        }
        if let Some(lat) = observation.get("latitude") {
            sensor_item["latitude"] = lat.clone();
        }
        if let Some(lon) = observation.get("longitude") {
            sensor_item["longitude"] = lon.clone();
        }
        if let Some(dir) = observation.get("direction") {
            sensor_item["direction"] = dir.clone();
        }
        if let Some(dev) = observation.get("device_id") {
            sensor_item["device_id"] = dev.clone();
        }
        if let Some(elev) = observation.get("elevation") {
            sensor_item["elevation"] = elev.clone();
        }

        let body = json!({
            "stationId": station_id,
            "sensorData": [sensor_item],
            "amount": 0.1,
            "timestamp": timestamp,
            "signature": signature
        });

        self.post("/weather/submit", Some(body)).await
    }

    // ========== Network Metrics API ==========

    /// Get network performance metrics
    pub async fn get_network_metrics(&self) -> Result<Value> {
        self.get("/stats", None).await
    }

    /// Get blockchain performance metrics
    pub async fn get_blockchain_metrics(&self) -> Result<Value> {
        self.get("/stats", None).await
    }

    /// Get system health status
    pub async fn get_system_health(&self) -> Result<Value> {
        self.get("/health", None).await
    }

    // ========== Transaction Fee API ==========

    /// Estimate transaction fee
    pub async fn estimate_fee(&self, from: &str, to: &str, amount: f64) -> Result<FeeEstimate> {
        let amount_str = amount.to_string();
        let query = [("from", from), ("to", to), ("amount", amount_str.as_str())];
        self.get("/transactions/estimate-fee", Some(&query)).await
    }

    // ========== Swap / DEX API ==========

    /// Get a swap quote
    pub async fn get_swap_quote(&self, from: &str, to: &str, amount: f64) -> Result<SwapQuote> {
        let amount_str = amount.to_string();
        let query = [("from", from), ("to", to), ("amount", amount_str.as_str())];
        self.get("/swap/quote", Some(&query)).await
    }

    /// Execute a swap
    pub async fn execute_swap(
        &self,
        from: &str,
        to: &str,
        amount: f64,
        slippage: f64,
        signature: &str,
    ) -> Result<Value> {
        let body = json!({
            "from_token": from,
            "to_token": to,
            "amount": amount,
            "slippage": slippage,
            "signature": signature
        });
        self.post("/swap/execute", Some(body)).await
    }

    /// Get token price
    pub async fn get_token_price(&self, pair: &str) -> Result<TokenPrice> {
        self.get(&format!("/swap/price/{pair}"), None).await
    }

    /// Get pool liquidity
    pub async fn get_pool_liquidity(&self, pair: &str) -> Result<PoolLiquidity> {
        self.get(&format!("/swap/liquidity/{pair}"), None).await
    }
}

fn parse_block_from_value(value: Value) -> Block {
    Block {
        hash: value
            .get("hash")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        height: value.get("height").and_then(Value::as_u64).unwrap_or(0),
        timestamp: value
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        prev_hash: value
            .get("prev_hash")
            .or_else(|| value.get("previousHash"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        merkle_root: value
            .get("merkle_root")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        size: value.get("size").and_then(Value::as_u64).unwrap_or(0),
        nonce: value.get("nonce").and_then(Value::as_u64).unwrap_or(0),
        difficulty: value.get("difficulty").and_then(Value::as_u64).unwrap_or(0),
        transactions_count: value
            .get("transactions_count")
            .or_else(|| value.get("transactions"))
            .and_then(|v| {
                if let Some(n) = v.as_u64() {
                    Some(n)
                } else {
                    v.as_array().map(|arr| arr.len() as u64)
                }
            })
            .unwrap_or(0),
        block_type: value
            .get("block_type")
            .and_then(Value::as_str)
            .unwrap_or("regular")
            .to_string(),
    }
}

#[cfg(test)]
mod bearer_token_redaction_tests {
    use super::ApiConfig;

    /// The token must not survive a `{:?}` of the config.
    ///
    /// This is the regression control for the exposure filed as core4#2978:
    /// `ApiConfig` derived `Debug` while carrying `bearer_token`, so any
    /// `{:?}` — a `debug!` line, an `anyhow` context, a panic payload — printed
    /// the credential verbatim. Reverting the hand-written `Debug` impl to a
    /// derive makes this test fail.
    #[test]
    fn debug_does_not_render_the_bearer_token() {
        // Deliberately NOT shaped like a credential, and not named like one. An
        // earlier revision bound a token-prefixed literal to a variable called
        // `secret`; detect-secrets flagged it as a Secret Keyword, correctly — a
        // leak test should not plant a string that reads as a live token. The
        // canary only needs to be distinctive enough that its presence in the
        // rendered output is unambiguous.
        let canary = "REDACTION-CANARY-not-a-credential";
        let cfg = ApiConfig {
            bearer_token: Some(canary.to_string()),
            ..ApiConfig::default()
        };

        let rendered = format!("{cfg:?}");

        assert!(
            !rendered.contains(canary),
            "bearer token leaked into Debug output: {rendered}"
        );
        // Presence is still reported — callers use it to decide whether to
        // attempt role-gated routes — so the field must not simply vanish.
        assert!(
            rendered.contains("bearer_token"),
            "bearer_token field disappeared from Debug output: {rendered}"
        );
        assert!(
            rendered.contains("<redacted>"),
            "expected an explicit redaction marker: {rendered}"
        );
    }

    /// An absent token renders as `None`, not as a redaction marker — otherwise
    /// "unauthenticated" and "authenticated, hidden" become indistinguishable
    /// in the logs, which is exactly what an operator needs to tell apart.
    #[test]
    fn debug_distinguishes_absent_from_redacted() {
        let cfg = ApiConfig {
            bearer_token: None,
            ..ApiConfig::default()
        };

        let rendered = format!("{cfg:?}");

        assert!(rendered.contains("None"), "expected None: {rendered}");
        assert!(
            !rendered.contains("<redacted>"),
            "an absent token must not render as redacted: {rendered}"
        );
    }
}

#[cfg(test)]
mod service_target_tests {
    use super::*;

    #[test]
    fn parse_transfer_fee_reads_only_an_active_positive_fee() {
        let active = json!({"transfer_fee": {"active": true, "base_units": "5000"}});
        assert_eq!(parse_transfer_fee(&active), Some(5_000));
        let wrapped = json!({"data": {"transfer_fee": {"active": true, "base_units": "5000"}}});
        assert_eq!(parse_transfer_fee(&wrapped), Some(5_000));
        for none in [
            json!({}),
            json!({"transfer_fee": {"active": false, "base_units": "5000"}}),
            json!({"transfer_fee": {"active": true, "base_units": "0"}}),
            json!({"transfer_fee": {"active": true, "base_units": 5000}}),
            json!({"transfer_fee": {"active": true, "base_units": "-1"}}),
        ] {
            assert_eq!(parse_transfer_fee(&none), None, "{none}");
        }
    }
    use std::sync::Mutex;

    // Env-var tests mutate process state; serialize them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn defaults_point_at_backend_service_ports_not_the_node() {
        let _g = ENV_LOCK.lock().unwrap();
        for target in [
            ServiceTarget::Compute,
            ServiceTarget::Storage,
            ServiceTarget::Iot,
            ServiceTarget::AiMl,
        ] {
            std::env::remove_var(target.env_var());
        }
        std::env::set_var("WRFCOIN_API_URL", "http://node.example:8556");
        assert_eq!(
            ApiConfig::for_service(ServiceTarget::Compute).base_url,
            "http://127.0.0.1:6000/api"
        );
        assert_eq!(
            ApiConfig::for_service(ServiceTarget::Storage).base_url,
            "http://127.0.0.1:7000/api"
        );
        assert_eq!(
            ApiConfig::for_service(ServiceTarget::Iot).base_url,
            "http://127.0.0.1:3002/api"
        );
        assert_eq!(
            ApiConfig::for_service(ServiceTarget::AiMl).base_url,
            "http://127.0.0.1:6001/api"
        );
        // The node URL must never leak into a service-family client.
        assert!(
            !ApiConfig::for_service(ServiceTarget::Compute)
                .base_url
                .contains("8556")
        );
        std::env::remove_var("WRFCOIN_API_URL");
    }

    #[test]
    fn family_env_var_overrides_and_trailing_slash_is_trimmed() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var(
            "WRFCOIN_COMPUTE_API_URL",
            " http://compute.internal:6000/api/ ",
        );
        assert_eq!(
            ApiConfig::for_service(ServiceTarget::Compute).base_url,
            "http://compute.internal:6000/api"
        );
        std::env::remove_var("WRFCOIN_COMPUTE_API_URL");
    }

    #[test]
    fn service_clients_do_not_forward_the_node_bearer_token() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("WRFCOIN_OPERATOR_TOKEN", "node-secret");
        assert!(ApiConfig::default().bearer_token.is_some()); // secret-leak-ok: test; asserts a bool (`is_some`), no token value formatted
        assert!(
            // secret-leak-ok: test; asserts a bool (`is_none`), no token value formatted
            ApiConfig::for_service(ServiceTarget::Storage)
                .bearer_token
                .is_none()
        );
        std::env::remove_var("WRFCOIN_OPERATOR_TOKEN");
    }

    #[test]
    fn honest_gap_reason_parses_backend_1118_envelope_only() {
        let body = r#"{"success":false,"unavailable":true,"reason":"Flux integration not yet wired","error":{"code":"NOT_IMPLEMENTED","message":"Flux integration not yet wired","see":"wrfcoin/backend#1118"}}"#;
        assert_eq!(
            honest_gap_reason(body).as_deref(),
            Some("Flux integration not yet wired [see wrfcoin/backend#1118]")
        );
        assert_eq!(honest_gap_reason("Not Implemented"), None);
        assert_eq!(
            honest_gap_reason(r#"{"success":false,"error":"nope"}"#),
            None
        );
    }

    #[test]
    fn describe_attaches_target_block() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("WRFCOIN_AIML_API_URL");
        let config = ApiConfig::for_service(ServiceTarget::AiMl);
        let mut out = json!({"summary": "x"});
        ServiceTarget::AiMl.attach(&config, &mut out);
        assert_eq!(out["target"]["base_url"], "http://127.0.0.1:6001/api");
        assert_eq!(out["target"]["env_var"], "WRFCOIN_AIML_API_URL");
        assert!(out["target"]["note"].as_str().unwrap().contains("ai-ml"));
    }
}
