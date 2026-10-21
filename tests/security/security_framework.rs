//! Security Testing Framework for Wrfcoin
//!
//! This module provides tools and utilities for security testing of the Wrfcoin blockchain.
//! It includes tests for common security vulnerabilities and attack vectors.

// Intentional security-testing *framework*: SecurityTestRunner,
// run_security_audit, and the report/vulnerability helpers are scaffolding
// wired for env-gated live-node runs, not the default `cargo test` pass, so
// they are legitimately uncalled in a normal build. Allow dead_code here
// rather than deleting the harness.
#![allow(dead_code)]

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use log::{debug, error, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::time;

/// Security test configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityTestConfig {
    /// Target API URL
    pub api_url: String,
    
    /// Whether to perform invasive tests
    /// Invasive tests might disrupt the node or network
    pub allow_invasive_tests: bool,
    
    /// Whether to exploit any vulnerabilities found
    /// When false, only reports vulnerabilities without exploiting
    pub exploit_vulnerabilities: bool,
    
    /// Maximum number of concurrent requests
    pub max_concurrent_requests: usize,
    
    /// Request timeout in seconds
    pub request_timeout_seconds: u64,
    
    /// Custom headers to add to requests
    pub custom_headers: HashMap<String, String>,
}

impl Default for SecurityTestConfig {
    fn default() -> Self {
        Self {
            api_url: std::env::var("WRFCOIN_TEST_API_URL")
                .unwrap_or_else(|_| "http://localhost:8556".to_string()),
            allow_invasive_tests: false,
            exploit_vulnerabilities: false,
            max_concurrent_requests: 10,
            request_timeout_seconds: 30,
            custom_headers: HashMap::new(),
        }
    }
}

/// Security vulnerability severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "Info"),
            Severity::Low => write!(f, "Low"),
            Severity::Medium => write!(f, "Medium"),
            Severity::High => write!(f, "High"),
            Severity::Critical => write!(f, "Critical"),
        }
    }
}

/// Security vulnerability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vulnerability {
    /// Vulnerability ID
    pub id: String,
    
    /// Vulnerability name
    pub name: String,
    
    /// Vulnerability description
    pub description: String,
    
    /// Severity level
    pub severity: Severity,
    
    /// Whether this was exploited during testing
    pub exploited: bool,
    
    /// Affected component
    pub affected_component: String,
    
    /// Recommendations for mitigation
    pub mitigation_recommendations: Vec<String>,
    
    /// Additional details
    pub details: Option<serde_json::Value>,
}

/// Security test results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityTestResults {
    /// Test configuration
    pub config: SecurityTestConfig,
    
    /// Found vulnerabilities
    pub vulnerabilities: Vec<Vulnerability>,
    
    /// Test start time
    pub start_time: chrono::DateTime<chrono::Utc>,
    
    /// Test end time
    pub end_time: chrono::DateTime<chrono::Utc>,
    
    /// API endpoints tested
    pub endpoints_tested: HashSet<String>,
    
    /// Total requests made
    pub total_requests: usize,
}

impl SecurityTestResults {
    /// Create a new empty results object
    pub fn new(config: SecurityTestConfig) -> Self {
        Self {
            config,
            vulnerabilities: Vec::new(),
            start_time: chrono::Utc::now(),
            end_time: chrono::Utc::now(),
            endpoints_tested: HashSet::new(),
            total_requests: 0,
        }
    }
    
    /// Add a vulnerability
    pub fn add_vulnerability(&mut self, vulnerability: Vulnerability) {
        self.vulnerabilities.push(vulnerability);
    }
    
    /// Mark test as completed
    pub fn complete(&mut self) {
        self.end_time = chrono::Utc::now();
    }
    
    /// Get test duration in seconds
    pub fn duration_seconds(&self) -> f64 {
        (self.end_time - self.start_time).num_milliseconds() as f64 / 1000.0
    }
    
    /// Get count of vulnerabilities by severity
    pub fn vulnerability_counts(&self) -> HashMap<Severity, usize> {
        let mut counts = HashMap::new();
        
        for vuln in &self.vulnerabilities {
            *counts.entry(vuln.severity).or_insert(0) += 1;
        }
        
        counts
    }
    
    /// Generate a report of the security test
    pub fn generate_report(&self) -> String {
        let mut report = String::new();
        
        report.push_str("======= WRFCOIN SECURITY TEST REPORT =======\n\n");
        
        // Test information
        report.push_str(&format!("Test Start: {}\n", self.start_time));
        report.push_str(&format!("Test End: {}\n", self.end_time));
        report.push_str(&format!("Duration: {:.2} seconds\n", self.duration_seconds()));
        report.push_str(&format!("Target: {}\n", self.config.api_url));
        report.push_str(&format!("Endpoints Tested: {}\n", self.endpoints_tested.len()));
        report.push_str(&format!("Total Requests: {}\n\n", self.total_requests));
        
        // Vulnerability summary
        let counts = self.vulnerability_counts();
        report.push_str("VULNERABILITY SUMMARY:\n");
        
        for severity in [Severity::Critical, Severity::High, Severity::Medium, Severity::Low, Severity::Info] {
            let count = counts.get(&severity).copied().unwrap_or(0);
            report.push_str(&format!("  {}: {}\n", severity, count));
        }
        
        report.push_str(&format!("Total Vulnerabilities: {}\n\n", self.vulnerabilities.len()));
        
        // Detailed vulnerabilities
        if !self.vulnerabilities.is_empty() {
            report.push_str("VULNERABILITIES:\n\n");
            
            for (i, vuln) in self.vulnerabilities.iter().enumerate() {
                report.push_str(&format!("{}. {} [{}]\n", i+1, vuln.name, vuln.severity));
                report.push_str(&format!("   ID: {}\n", vuln.id));
                report.push_str(&format!("   Component: {}\n", vuln.affected_component));
                report.push_str(&format!("   Description: {}\n", vuln.description));
                report.push_str(&format!("   Exploited: {}\n", vuln.exploited));
                
                if !vuln.mitigation_recommendations.is_empty() {
                    report.push_str("   Mitigation:\n");
                    for (j, rec) in vuln.mitigation_recommendations.iter().enumerate() {
                        report.push_str(&format!("     {}) {}\n", j+1, rec));
                    }
                }
                
                report.push_str("\n");
            }
        } else {
            report.push_str("No vulnerabilities found.\n\n");
        }
        
        report.push_str("============= END OF REPORT =============\n");
        
        report
    }
}

/// Base security test runner
pub struct SecurityTestRunner {
    config: SecurityTestConfig,
    client: Client,
    results: SecurityTestResults,
}

impl SecurityTestRunner {
    /// Create a new security test runner
    pub fn new(config: SecurityTestConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");
            
        let results = SecurityTestResults::new(config.clone());
        
        Self {
            config,
            client,
            results,
        }
    }
    
    /// Get test results
    pub fn results(&self) -> &SecurityTestResults {
        &self.results
    }
    
    /// Run all security tests
    pub async fn run_all_tests(&mut self) -> Result<()> {
        info!("Starting security test suite");
        
        // Get API endpoints
        let endpoints = self.discover_endpoints().await?;
        info!("Discovered {} API endpoints", endpoints.len());
        
        // API endpoint security checks
        self.check_api_endpoints(&endpoints).await?;
        
        // Network level checks
        self.check_network_security().await?;
        
        // P2P protocol checks
        self.check_p2p_protocol().await?;
        
        // Consensus vulnerabilities
        self.check_consensus_vulnerabilities().await?;
        
        // Transaction validation checks
        self.check_transaction_validation().await?;
        
        // Mempool security checks
        self.check_mempool_security().await?;
        
        // Smart contract/NFT execution security (if applicable)
        self.check_nft_execution_security().await?;
        
        // MEV protection checks
        self.check_mev_protection().await?;
        
        // Wallet security checks
        self.check_wallet_security().await?;
        
        // Complete the test results
        self.results.complete();
        
        // Generate and output report
        let report = self.results.generate_report();
        info!("\n{}", report);
        
        if !self.results.vulnerabilities.is_empty() {
            let critical_count = self.results.vulnerabilities.iter()
                .filter(|v| v.severity == Severity::Critical)
                .count();
                
            let high_count = self.results.vulnerabilities.iter()
                .filter(|v| v.severity == Severity::High)
                .count();
                
            if critical_count > 0 {
                error!("Found {} critical security vulnerabilities!", critical_count);
            } else if high_count > 0 {
                warn!("Found {} high severity security vulnerabilities!", high_count);
            }
        } else {
            info!("No security vulnerabilities were found.");
        }
        
        Ok(())
    }
    
    /// Discover API endpoints
    async fn discover_endpoints(&mut self) -> Result<Vec<String>> {
        let mut endpoints = Vec::new();
        
        // Try common API discovery endpoints
        let discovery_endpoints = [
            "/",
            "/api",
            "/openapi.json",
            "/swagger.json",
            "/api-docs",
            "/docs",
        ];
        
        for endpoint in &discovery_endpoints {
            let url = format!("{}{}", self.config.api_url, endpoint);
            
            match self.client.get(&url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        // Record this endpoint
                        endpoints.push(endpoint.to_string());
                        self.results.endpoints_tested.insert(endpoint.to_string());
                        self.results.total_requests += 1;
                        
                        // Try to parse response for more endpoints
                        let text = response.text().await.unwrap_or_default();
                        
                        // Look for endpoint patterns
                        for line in text.lines() {
                            if line.contains("/api/") || line.contains("\"path\":") {
                                // Simple heuristic to extract endpoints
                                if let Some(start) = line.find("/api/") {
                                    if let Some(end) = line[start..].find("\"") {
                                        let api_path = &line[start..start+end];
                                        endpoints.push(api_path.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    // Endpoint not available, continue
                }
            }
        }
        
        // Add common blockchain endpoints
        let common_endpoints = [
            "/blocks",
            "/transactions",
            "/mempool",
            "/peers",
            "/status",
            "/wallet",
            "/balance",
            "/mine",
        ];
        
        for endpoint in &common_endpoints {
            endpoints.push(endpoint.to_string());
        }
        
        // Add Wrfcoin-specific endpoints
        let wrfcoin_endpoints = [
            "/nfts",
            "/computational-nfts",
            "/verification",
            "/cross-chain",
            "/mev-protection",
            "/governance",
            "/layer2",
        ];
        
        for endpoint in &wrfcoin_endpoints {
            endpoints.push(endpoint.to_string());
        }
        
        // Deduplicate
        endpoints.sort();
        endpoints.dedup();
        
        Ok(endpoints)
    }
    
    /// Check API endpoint security
    async fn check_api_endpoints(&mut self, endpoints: &[String]) -> Result<()> {
        info!("Checking API endpoint security");
        
        for endpoint in endpoints {
            // Skip if this is just a path component
            if !endpoint.starts_with('/') {
                continue;
            }
            
            let url = format!("{}{}", self.config.api_url, endpoint);
            
            // Track this endpoint as tested
            self.results.endpoints_tested.insert(endpoint.clone());
            
            // Test for missing authentication
            self.check_missing_auth(&url).await?;
            
            // Test for parameter tampering
            self.check_parameter_tampering(&url).await?;
            
            // Test for injection vulnerabilities
            self.check_injection_vulnerabilities(&url).await?;
            
            // Test for rate limiting
            self.check_rate_limiting(&url).await?;
        }
        
        Ok(())
    }
    
    /// Check for missing authentication
    async fn check_missing_auth(&mut self, url: &str) -> Result<()> {
        debug!("Checking for missing authentication on {}", url);
        
        // Attempt to access without authentication
        let response = self.client.get(url).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            // If status is 200-299, it might be missing auth
            if resp.status().is_success() {
                // Check if the response is meaningful (not just an error message)
                let text = resp.text().await.unwrap_or_default();
                
                if text.len() > 50 && !text.to_lowercase().contains("error") && 
                   !text.to_lowercase().contains("unauthorized") && 
                   !text.to_lowercase().contains("permission") {
                    
                    // Endpoint may be missing authentication
                    if url.contains("/admin") || 
                       url.contains("/private") || 
                       url.contains("/wallet") || 
                       url.contains("/mine") {
                        
                        let vuln = Vulnerability {
                            id: format!("MISSING_AUTH_{}", url.replace('/', "_")),
                            name: "Missing Authentication".to_string(),
                            description: format!("The endpoint {} is accessible without authentication, but appears to contain sensitive functionality.", url),
                            severity: Severity::High,
                            exploited: false,
                            affected_component: "API Security".to_string(),
                            mitigation_recommendations: vec![
                                "Implement authentication for sensitive API endpoints".to_string(),
                                "Use OAuth2, JWT, or API keys for authenticating requests".to_string(),
                                "Add role-based access control (RBAC) to restrict access".to_string(),
                            ],
                            details: None,
                        };
                        
                        info!("Found vulnerability: {}", vuln.name);
                        self.results.add_vulnerability(vuln);
                    } else {
                        // Less sensitive endpoint
                        debug!("Endpoint {} is accessible without authentication", url);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Check for parameter tampering vulnerabilities
    async fn check_parameter_tampering(&mut self, url: &str) -> Result<()> {
        debug!("Checking for parameter tampering on {}", url);
        
        // List of test payloads for different parameters
        let test_params = [
            ("id", "1' OR '1'='1"),
            ("address", "wrffff' OR '1'='1"),
            ("block", "latest'; DROP TABLE blocks;--"),
            ("tx", "0x00000000000000' UNION SELECT * FROM users;--"),
        ];
        
        for (param, value) in &test_params {
            // Test GET parameter
            let test_url = format!("{}?{}={}", url, param, value);
            
            let response = self.client.get(&test_url).send().await;
            self.results.total_requests += 1;
            
            if let Ok(resp) = response {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                
                // Check for SQL error messages
                if text.contains("SQL syntax") || 
                   text.contains("ORA-") || 
                   text.contains("syntax error") || 
                   text.contains("SQLSTATE") {
                    
                    let vuln = Vulnerability {
                        id: format!("PARAM_TAMPER_{}", param),
                        name: "Parameter Tampering Vulnerability".to_string(),
                        description: format!("The endpoint {} appears vulnerable to parameter tampering with the '{}' parameter.", url, param),
                        severity: Severity::High,
                        exploited: false,
                        affected_component: "API Security".to_string(),
                        mitigation_recommendations: vec![
                            "Use parameterized queries instead of string concatenation".to_string(),
                            "Validate and sanitize all user input".to_string(),
                            "Implement proper input validation and escaping".to_string(),
                        ],
                        details: Some(serde_json::json!({
                            "parameter": param,
                            "payload": value,
                            "response_status": status.as_u16(),
                            "response_excerpt": text.chars().take(200).collect::<String>(),
                        })),
                    };
                    
                    info!("Found vulnerability: {}", vuln.name);
                    self.results.add_vulnerability(vuln);
                }
            }
            
            // Test POST parameter
            let params = [(param.to_string(), value.to_string())];
            
            let response = self.client
                .post(url)
                .form(&params)
                .send()
                .await;
                
            self.results.total_requests += 1;
            
            if let Ok(resp) = response {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                
                // Check for SQL error messages
                if text.contains("SQL syntax") || 
                   text.contains("ORA-") || 
                   text.contains("syntax error") || 
                   text.contains("SQLSTATE") {
                    
                    let vuln = Vulnerability {
                        id: format!("PARAM_TAMPER_POST_{}", param),
                        name: "POST Parameter Tampering Vulnerability".to_string(),
                        description: format!("The endpoint {} appears vulnerable to parameter tampering with the '{}' parameter in POST requests.", url, param),
                        severity: Severity::High,
                        exploited: false,
                        affected_component: "API Security".to_string(),
                        mitigation_recommendations: vec![
                            "Use parameterized queries instead of string concatenation".to_string(),
                            "Validate and sanitize all user input".to_string(),
                            "Implement proper input validation and escaping".to_string(),
                        ],
                        details: Some(serde_json::json!({
                            "parameter": param,
                            "payload": value,
                            "response_status": status.as_u16(),
                            "response_excerpt": text.chars().take(200).collect::<String>(),
                        })),
                    };
                    
                    info!("Found vulnerability: {}", vuln.name);
                    self.results.add_vulnerability(vuln);
                }
            }
        }
        
        Ok(())
    }
    
    /// Check for injection vulnerabilities (SQL, NoSQL, Command)
    async fn check_injection_vulnerabilities(&mut self, url: &str) -> Result<()> {
        debug!("Checking for injection vulnerabilities on {}", url);
        
        // Injection payloads to test
        let injection_payloads = [
            // SQL Injection
            "1' OR '1'='1",
            "1; DROP TABLE users; --",
            // NoSQL Injection
            "{\"$gt\": \"\"}",
            "{\"$where\": \"return true\"}",
            // Command Injection
            "test; cat /etc/passwd",
            "test && ls -la",
            // Path Traversal
            "../../../etc/passwd",
            "..\\..\\..\\Windows\\system.ini",
        ];
        
        // Craft different request bodies and parameters
        for payload in &injection_payloads {
            // Test in JSON body
            let json_body = serde_json::json!({
                "query": payload,
                "id": payload,
                "address": payload,
                "transaction": payload,
            });
            
            let response = self.client
                .post(url)
                .json(&json_body)
                .send()
                .await;
                
            self.results.total_requests += 1;
            
            if let Ok(resp) = response {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                
                // Check for signs of successful injection
                if text.contains("root:") || 
                   text.contains("uid=") || 
                   text.contains("SQL syntax") || 
                   text.contains("unexpected token") ||
                   text.contains("command not found") {
                    
                    let vuln = Vulnerability {
                        id: format!("INJECTION_{}", url.replace('/', "_")),
                        name: "Injection Vulnerability".to_string(),
                        description: format!("The endpoint {} appears vulnerable to injection attacks.", url),
                        severity: Severity::Critical,
                        exploited: false,
                        affected_component: "API Security".to_string(),
                        mitigation_recommendations: vec![
                            "Use parameterized queries for database operations".to_string(),
                            "Validate and sanitize all user inputs".to_string(),
                            "Implement strict input validation".to_string(),
                            "Use prepared statements for database queries".to_string(),
                            "Apply the principle of least privilege for database accounts".to_string(),
                        ],
                        details: Some(serde_json::json!({
                            "payload": payload,
                            "response_status": status.as_u16(),
                            "response_excerpt": text.chars().take(200).collect::<String>(),
                        })),
                    };
                    
                    info!("Found vulnerability: {}", vuln.name);
                    self.results.add_vulnerability(vuln);
                    break; // One vulnerability found is enough for this endpoint
                }
            }
        }
        
        Ok(())
    }
    
    /// Check for rate limiting
    async fn check_rate_limiting(&mut self, url: &str) -> Result<()> {
        debug!("Checking for rate limiting on {}", url);
        
        // Only perform this test on certain endpoints to avoid disruption
        if !url.contains("/blocks") && 
           !url.contains("/status") && 
           !url.contains("/transactions") {
            return Ok(());
        }
        
        // Number of rapid requests to send
        let request_count = 50;
        
        let start_time = Instant::now();

        // Send requests in parallel
        let futures = stream::iter((0..request_count).map(|i| {
            let client = self.client.clone();
            let url = url.to_string();
            
            async move {
                let result = client.get(&url).send().await;
                (i, result)
            }
        }))
        .buffer_unordered(20) // Send 20 at a time
        .collect::<Vec<_>>();
        
        let responses = futures.await;
        self.results.total_requests += request_count;
        
        let duration = start_time.elapsed();
        
        // Count successful responses
        let success_count = responses.iter()
            .filter(|(_, result)| result.is_ok() && result.as_ref().unwrap().status().is_success())
            .count();
            
        // If all or most requests succeeded, there might be no rate limiting
        if success_count > request_count * 9 / 10 {
            let vuln = Vulnerability {
                id: format!("NO_RATE_LIMIT_{}", url.replace('/', "_")),
                name: "Missing Rate Limiting".to_string(),
                description: format!("The endpoint {} does not appear to have effective rate limiting. Sent {} requests in {:.2} seconds with {} successful responses.", 
                    url, request_count, duration.as_secs_f64(), success_count),
                severity: Severity::Medium,
                exploited: false,
                affected_component: "API Security".to_string(),
                mitigation_recommendations: vec![
                    "Implement rate limiting on all API endpoints".to_string(),
                    "Use token bucket, leaky bucket, or fixed window algorithms".to_string(),
                    "Consider implementing client IP-based and/or API key-based rate limiting".to_string(),
                    "Return HTTP 429 (Too Many Requests) when rate limits are exceeded".to_string(),
                ],
                details: None,
            };
            
            info!("Found vulnerability: {}", vuln.name);
            self.results.add_vulnerability(vuln);
        }
        
        Ok(())
    }
    
    /// Check network security
    async fn check_network_security(&mut self) -> Result<()> {
        info!("Checking network security");
        
        // Check for open ports (if allowed)
        if self.config.allow_invasive_tests {
            self.check_open_ports().await?;
        }
        
        // Check for cleartext communication
        self.check_cleartext_communication().await?;
        
        Ok(())
    }
    
    /// Check for open ports
    async fn check_open_ports(&mut self) -> Result<()> {
        debug!("Checking for exposed ports");
        
        // Extract hostname from API URL
        let url = reqwest::Url::parse(&self.config.api_url)?;
        let host = url.host_str()
            .context("Failed to extract host from API URL")?
            .to_string();
            
        // Common blockchain ports to check
        let ports_to_check = [
            8545, // Ethereum JSON-RPC
            8546, // Ethereum WS-RPC
            8551, // Engine API
            30303, // Ethereum P2P
            6333, // Wrfcoin P2P
            6334, // Wrfcoin P2P alt
            5000, // API
            5001, // API alt
            22, // SSH
            3306, // MySQL
            27017, // MongoDB
        ];
        
        let mut open_ports = Vec::new();
        
        for &port in &ports_to_check {
            // Attempt to connect to the port
            let address = format!("{}:{}", host, port);
            
            match tokio::net::TcpStream::connect(&address).await {
                Ok(_) => {
                    // Port is open
                    open_ports.push(port);
                }
                Err(_) => {
                    // Port is closed or filtered
                }
            }
        }
        
        if !open_ports.is_empty() {
            let vuln = Vulnerability {
                id: "EXPOSED_PORTS".to_string(),
                name: "Exposed Network Ports".to_string(),
                description: format!("The following ports are open on {}: {}", host, open_ports.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ")),
                severity: Severity::Medium,
                exploited: false,
                affected_component: "Network Security".to_string(),
                mitigation_recommendations: vec![
                    "Restrict access to sensitive ports using a firewall".to_string(),
                    "Use VPN or SSH tunneling for administrative access".to_string(),
                    "Implement proper network segmentation".to_string(),
                    "Configure services to only listen on specific interfaces".to_string(),
                ],
                details: Some(serde_json::json!({
                    "open_ports": open_ports,
                    "host": host,
                })),
            };
            
            info!("Found vulnerability: {}", vuln.name);
            self.results.add_vulnerability(vuln);
        }
        
        Ok(())
    }
    
    /// Check for cleartext communication
    async fn check_cleartext_communication(&mut self) -> Result<()> {
        debug!("Checking for cleartext communication");
        
        // Check if API endpoint is using HTTPS
        if !self.config.api_url.starts_with("https://") {
            let vuln = Vulnerability {
                id: "CLEARTEXT_API".to_string(),
                name: "Cleartext API Communication".to_string(),
                description: "The API endpoint is using cleartext HTTP instead of encrypted HTTPS.".to_string(),
                severity: Severity::High,
                exploited: false,
                affected_component: "Network Security".to_string(),
                mitigation_recommendations: vec![
                    "Configure TLS/SSL for all API endpoints".to_string(),
                    "Redirect HTTP to HTTPS".to_string(),
                    "Implement HSTS (HTTP Strict Transport Security)".to_string(),
                    "Use modern TLS protocols (TLS 1.2+) and disable older versions".to_string(),
                ],
                details: None,
            };
            
            info!("Found vulnerability: {}", vuln.name);
            self.results.add_vulnerability(vuln);
        }
        
        Ok(())
    }
    
    /// Check P2P protocol security
    async fn check_p2p_protocol(&mut self) -> Result<()> {
        info!("Checking P2P protocol security");
        
        // Get peers information
        let url = format!("{}/peers", self.config.api_url);
        
        let response = self.client.get(&url).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            if resp.status().is_success() {
                // Check for peer authentication first (borrows resp)
                self.check_peer_authentication(&resp).await?;

                // Check for peer validation (consumes resp)
                self.check_peer_validation(resp).await?;
            }
        }
        
        Ok(())
    }
    
    /// Check peer validation
    async fn check_peer_validation(&mut self, resp: reqwest::Response) -> Result<()> {
        debug!("Checking peer validation");

        let text = resp.text().await?;
        
        // If we have access to peer details without authentication, that might be an issue
        if text.contains("peer") && text.contains("id") && text.contains("address") {
            let vuln = Vulnerability {
                id: "PEER_INFO_EXPOSURE".to_string(),
                name: "Peer Information Exposure".to_string(),
                description: "Peer information is accessible without proper authentication.".to_string(),
                severity: Severity::Low,
                exploited: false,
                affected_component: "P2P Network".to_string(),
                mitigation_recommendations: vec![
                    "Restrict access to peer information API endpoints".to_string(),
                    "Implement authentication for sensitive network information".to_string(),
                    "Limit the detail of peer information exposed via API".to_string(),
                ],
                details: None,
            };
            
            info!("Found vulnerability: {}", vuln.name);
            self.results.add_vulnerability(vuln);
        }
        
        Ok(())
    }
    
    /// Check peer authentication
    async fn check_peer_authentication(&mut self, _resp: &reqwest::Response) -> Result<()> {
        debug!("Checking peer authentication");
        
        // For now, we'll only mark this as an area to investigate further
        let vuln = Vulnerability {
            id: "PEER_AUTH_CHECK".to_string(),
            name: "Peer Authentication Check Required".to_string(),
            description: "Recommendation to manually verify P2P peer authentication mechanisms.".to_string(),
            severity: Severity::Info,
            exploited: false,
            affected_component: "P2P Network".to_string(),
            mitigation_recommendations: vec![
                "Implement authenticated encryption for P2P communications".to_string(),
                "Use mutual TLS or similar mechanism for peer authentication".to_string(),
                "Implement peer reputation and banning system".to_string(),
                "Consider using node identity certificates".to_string(),
            ],
            details: None,
        };
        
        info!("Found recommendation: {}", vuln.name);
        self.results.add_vulnerability(vuln);
        
        Ok(())
    }
    
    /// Check consensus vulnerabilities
    async fn check_consensus_vulnerabilities(&mut self) -> Result<()> {
        info!("Checking consensus vulnerabilities");
        
        // Check for timing vulnerabilities
        self.check_timing_vulnerabilities().await?;
        
        // Check for 51% attack vulnerability
        self.check_51_percent_attack().await?;
        
        // Check for long-range attack vulnerability
        self.check_long_range_attack().await?;
        
        Ok(())
    }
    
    /// Check for timing vulnerabilities
    async fn check_timing_vulnerabilities(&mut self) -> Result<()> {
        debug!("Checking for timing vulnerabilities");
        
        // This is mostly a recommendation as it requires deep analysis
        let vuln = Vulnerability {
            id: "TIMING_ANALYSIS".to_string(),
            name: "Timing Analysis Recommendation".to_string(),
            description: "Recommendation to analyze possible timing side-channels in consensus implementation.".to_string(),
            severity: Severity::Info,
            exploited: false,
            affected_component: "Consensus".to_string(),
            mitigation_recommendations: vec![
                "Implement constant-time algorithms for cryptographic operations".to_string(),
                "Use secure cryptographic libraries".to_string(),
                "Add random delays to make timing analysis harder".to_string(),
                "Review code for constant-time violations".to_string(),
            ],
            details: None,
        };
        
        info!("Found recommendation: {}", vuln.name);
        self.results.add_vulnerability(vuln);
        
        Ok(())
    }
    
    /// Check for 51% attack vulnerability
    async fn check_51_percent_attack(&mut self) -> Result<()> {
        debug!("Checking for 51% attack vulnerability");
        
        // Get blockchain status
        let url = format!("{}/status", self.config.api_url);
        
        let response = self.client.get(&url).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            if resp.status().is_success() {
                let status: serde_json::Value = resp.json().await?;
                
                // Look for hash rate or mining power distribution
                if let Some(hash_rate) = status.get("hash_rate").and_then(|v| v.as_f64()) {
                    if hash_rate < 1000.0 {
                        // Low hash rate might indicate vulnerability to 51% attack
                        let vuln = Vulnerability {
                            id: "HASH_RATE_LOW".to_string(),
                            name: "Low Hash Rate".to_string(),
                            description: format!("The blockchain has a relatively low hash rate ({} H/s), which might make it vulnerable to 51% attacks.", hash_rate),
                            severity: Severity::Medium,
                            exploited: false,
                            affected_component: "Consensus".to_string(),
                            mitigation_recommendations: vec![
                                "Increase the number of validators/miners".to_string(),
                                "Consider implementing checkpointing".to_string(),
                                "Implement time-delayed finality".to_string(),
                                "Require higher confirmations for large transactions".to_string(),
                            ],
                            details: Some(serde_json::json!({
                                "hash_rate": hash_rate,
                            })),
                        };
                        
                        info!("Found vulnerability: {}", vuln.name);
                        self.results.add_vulnerability(vuln);
                    }
                }
                
                // Check for miner/validator centralization
                if let Some(miners) = status.get("miners").and_then(|v| v.as_array()) {
                    let total_miners = miners.len();
                    
                    // If less than 10 active miners, potentially centralized
                    if total_miners < 10 {
                        let vuln = Vulnerability {
                            id: "VALIDATOR_CENTRALIZATION".to_string(),
                            name: "Validator Centralization".to_string(),
                            description: format!("The blockchain has only {} active miners/validators, which indicates centralization risk.", total_miners),
                            severity: Severity::Medium,
                            exploited: false,
                            affected_component: "Consensus".to_string(),
                            mitigation_recommendations: vec![
                                "Encourage more miners/validators to join the network".to_string(),
                                "Implement incentives for decentralization".to_string(),
                                "Consider a different consensus algorithm with better decentralization properties".to_string(),
                            ],
                            details: Some(serde_json::json!({
                                "total_miners": total_miners,
                            })),
                        };
                        
                        info!("Found vulnerability: {}", vuln.name);
                        self.results.add_vulnerability(vuln);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Check for long-range attack vulnerability
    async fn check_long_range_attack(&mut self) -> Result<()> {
        debug!("Checking for long-range attack vulnerability");
        
        // Get blockchain info
        let url = format!("{}/status", self.config.api_url);
        
        let response = self.client.get(&url).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            if resp.status().is_success() {
                let status: serde_json::Value = resp.json().await?;
                
                // Check if using PoS or PoW
                if let Some(consensus) = status.get("consensus_type").and_then(|v| v.as_str()) {
                    // If PoS without checkpointing, potentially vulnerable
                    if consensus.to_lowercase().contains("pos") || 
                       consensus.to_lowercase().contains("proof of stake") {
                        
                        let has_checkpointing = status
                            .get("features")
                            .and_then(|f| f.as_object())
                            .and_then(|o| o.get("checkpointing"))
                            .and_then(|c| c.as_bool())
                            .unwrap_or(false);
                            
                        if !has_checkpointing {
                            let vuln = Vulnerability {
                                id: "LONG_RANGE_ATTACK".to_string(),
                                name: "Long-Range Attack Vulnerability".to_string(),
                                description: "The blockchain uses Proof-of-Stake without checkpointing, which may make it vulnerable to long-range attacks.".to_string(),
                                severity: Severity::Medium,
                                exploited: false,
                                affected_component: "Consensus".to_string(),
                                mitigation_recommendations: vec![
                                    "Implement checkpointing mechanisms".to_string(),
                                    "Consider weak subjectivity with social consensus".to_string(),
                                    "Use time-based finality".to_string(),
                                ],
                                details: None,
                            };
                            
                            info!("Found vulnerability: {}", vuln.name);
                            self.results.add_vulnerability(vuln);
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Check transaction validation
    async fn check_transaction_validation(&mut self) -> Result<()> {
        info!("Checking transaction validation");
        
        // Check for signature replay
        self.check_signature_replay().await?;
        
        // Check for transaction malleability
        self.check_transaction_malleability().await?;
        
        // Check transaction format validation
        self.check_transaction_format_validation().await?;
        
        Ok(())
    }
    
    /// Check for signature replay vulnerabilities
    async fn check_signature_replay(&mut self) -> Result<()> {
        debug!("Checking for signature replay vulnerabilities");
        
        // Create a transaction
        let url = format!("{}/transactions", self.config.api_url);
        
        let tx = serde_json::json!({
            "sender": "test_address",
            "recipient": "test_recipient",
            "amount": 0.001,
            "nonce": 1,
        });
        
        let response = self.client.post(&url).json(&tx).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            if resp.status().is_success() {
                let result: serde_json::Value = resp.json().await?;
                
                if let Some(tx_id) = result.get("id").and_then(|id| id.as_str()) {
                    // Try to resubmit the same transaction
                    let second_response = self.client.post(&url).json(&tx).send().await;
                    self.results.total_requests += 1;
                    
                    if let Ok(second_resp) = second_response {
                        if second_resp.status().is_success() {
                            // If same transaction can be submitted twice, might be vulnerable
                            // to signature replay
                            let second_result: serde_json::Value = second_resp.json().await?;
                            
                            if let Some(second_tx_id) = second_result.get("id").and_then(|id| id.as_str()) {
                                if second_tx_id != tx_id {
                                    let vuln = Vulnerability {
                                        id: "SIGNATURE_REPLAY".to_string(),
                                        name: "Signature Replay Vulnerability".to_string(),
                                        description: "The same transaction can be submitted multiple times, indicating a potential signature replay vulnerability.".to_string(),
                                        severity: Severity::High,
                                        exploited: false,
                                        affected_component: "Transaction Validation".to_string(),
                                        mitigation_recommendations: vec![
                                            "Implement proper nonce tracking per account".to_string(),
                                            "Include chain ID in transaction signatures".to_string(),
                                            "Validate transactions against the current UTXO set".to_string(),
                                            "Implement transaction expiration".to_string(),
                                        ],
                                        details: Some(serde_json::json!({
                                            "first_tx_id": tx_id,
                                            "second_tx_id": second_tx_id,
                                        })),
                                    };
                                    
                                    info!("Found vulnerability: {}", vuln.name);
                                    self.results.add_vulnerability(vuln);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Check for transaction malleability
    async fn check_transaction_malleability(&mut self) -> Result<()> {
        debug!("Checking for transaction malleability");
        
        // Create a transaction
        let url = format!("{}/transactions", self.config.api_url);
        
        let tx = serde_json::json!({
            "sender": "test_address",
            "recipient": "test_recipient",
            "amount": 0.001,
            "nonce": 2,
            "data": "Original transaction data",
        });
        
        let response = self.client.post(&url).json(&tx).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            if resp.status().is_success() {
                let result: serde_json::Value = resp.json().await?;
                
                if let Some(tx_id) = result.get("id").and_then(|id| id.as_str()) {
                    // Try to submit a slightly modified version
                    let modified_tx = serde_json::json!({
                        "sender": "test_address",
                        "recipient": "test_recipient",
                        "amount": 0.001,
                        "nonce": 2,
                        "data": "Modified transaction data",
                    });
                    
                    let second_response = self.client.post(&url).json(&modified_tx).send().await;
                    self.results.total_requests += 1;
                    
                    if let Ok(second_resp) = second_response {
                        if second_resp.status().is_success() {
                            // If modified transaction accepted with same nonce, might be vulnerable
                            let second_result: serde_json::Value = second_resp.json().await?;
                            
                            if let Some(second_tx_id) = second_result.get("id").and_then(|id| id.as_str()) {
                                if second_tx_id != tx_id {
                                    let vuln = Vulnerability {
                                        id: "TRANSACTION_MALLEABILITY".to_string(),
                                        name: "Transaction Malleability".to_string(),
                                        description: "Modified transactions with the same nonce are accepted, indicating potential transaction malleability issues.".to_string(),
                                        severity: Severity::Medium,
                                        exploited: false,
                                        affected_component: "Transaction Validation".to_string(),
                                        mitigation_recommendations: vec![
                                            "Use deterministic signatures (e.g., RFC 6979)".to_string(),
                                            "Include all transaction fields in the signed data".to_string(),
                                            "Strictly validate transaction format before accepting".to_string(),
                                        ],
                                        details: Some(serde_json::json!({
                                            "original_tx_id": tx_id,
                                            "modified_tx_id": second_tx_id,
                                        })),
                                    };
                                    
                                    info!("Found vulnerability: {}", vuln.name);
                                    self.results.add_vulnerability(vuln);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Check transaction format validation
    async fn check_transaction_format_validation(&mut self) -> Result<()> {
        debug!("Checking transaction format validation");
        
        // Create a transaction with invalid data
        let url = format!("{}/transactions", self.config.api_url);
        
        // Test cases with various malformed data
        let test_cases = [
            // Negative amount
            serde_json::json!({
                "sender": "test_address",
                "recipient": "test_recipient",
                "amount": -1.0,
                "nonce": 3,
            }),
            // Extremely large amount
            serde_json::json!({
                "sender": "test_address",
                "recipient": "test_recipient",
                "amount": 1e20,
                "nonce": 4,
            }),
            // Invalid sender address
            serde_json::json!({
                "sender": "totally_invalid_address!@#$",
                "recipient": "test_recipient",
                "amount": 0.001,
                "nonce": 5,
            }),
            // Missing required fields
            serde_json::json!({
                "sender": "test_address",
                "amount": 0.001,
                "nonce": 6,
            }),
        ];
        
        for (i, test_case) in test_cases.iter().enumerate() {
            let response = self.client.post(&url).json(test_case).send().await;
            self.results.total_requests += 1;
            
            if let Ok(resp) = response {
                // If a malformed transaction is accepted, that's a problem
                if resp.status().is_success() {
                    let result: serde_json::Value = resp.json().await?;
                    
                    if let Some(tx_id) = result.get("id").and_then(|id| id.as_str()) {
                        let vuln = Vulnerability {
                            id: format!("TX_VALIDATION_{}", i),
                            name: "Insufficient Transaction Validation".to_string(),
                            description: "The blockchain accepts transactions with invalid or malformed data.".to_string(),
                            severity: Severity::High,
                            exploited: false,
                            affected_component: "Transaction Validation".to_string(),
                            mitigation_recommendations: vec![
                                "Implement strict validation for all transaction fields".to_string(),
                                "Validate sender addresses, amounts, and other critical fields".to_string(),
                                "Reject transactions with missing required fields".to_string(),
                                "Implement proper error handling and reporting".to_string(),
                            ],
                            details: Some(serde_json::json!({
                                "test_case": i,
                                "transaction": test_case,
                                "accepted_tx_id": tx_id,
                            })),
                        };
                        
                        info!("Found vulnerability: {}", vuln.name);
                        self.results.add_vulnerability(vuln);
                        break; // One vulnerability found is enough
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Check mempool security
    async fn check_mempool_security(&mut self) -> Result<()> {
        info!("Checking mempool security");
        
        // Check for mempool transaction flooding
        self.check_mempool_flooding().await?;
        
        // Check for memory exhaustion
        self.check_memory_exhaustion().await?;
        
        Ok(())
    }
    
    /// Check for mempool flooding vulnerabilities
    async fn check_mempool_flooding(&mut self) -> Result<()> {
        debug!("Checking for mempool flooding vulnerabilities");
        
        // Skip invasive tests if not allowed
        if !self.config.allow_invasive_tests {
            let vuln = Vulnerability {
                id: "MEMPOOL_FLOODING_CHECK".to_string(),
                name: "Mempool Flooding Check Recommended".to_string(),
                description: "Recommendation to manually test for mempool flooding vulnerabilities.".to_string(),
                severity: Severity::Info,
                exploited: false,
                affected_component: "Mempool".to_string(),
                mitigation_recommendations: vec![
                    "Implement transaction fee requirements".to_string(),
                    "Limit the number of transactions per sender".to_string(),
                    "Set a maximum mempool size".to_string(),
                    "Prioritize transactions by fee".to_string(),
                    "Implement rate limiting for transaction submission".to_string(),
                ],
                details: None,
            };
            
            info!("Found recommendation: {}", vuln.name);
            self.results.add_vulnerability(vuln);
            return Ok(());
        }
        
        // Create many small transactions and monitor mempool size
        let url = format!("{}/transactions", self.config.api_url);
        let mempool_url = format!("{}/mempool", self.config.api_url);
        
        // Get initial mempool size
        let initial_response = self.client.get(&mempool_url).send().await;
        self.results.total_requests += 1;
        
        let initial_size = if let Ok(resp) = initial_response {
            if resp.status().is_success() {
                let mempool: serde_json::Value = resp.json().await?;
                mempool.get("size").and_then(|s| s.as_u64()).unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };
        
        // Send a moderate number of transactions
        let num_transactions = 20;
        
        for i in 0..num_transactions {
            let tx = serde_json::json!({
                "sender": format!("test_address_{}", i),
                "recipient": "flooding_test_recipient",
                "amount": 0.00001,
                "nonce": i,
                "fee": 0.00001,
            });
            
            let _ = self.client.post(&url).json(&tx).send().await;
            self.results.total_requests += 1;
            
            // Small delay to avoid rate limiting
            time::sleep(Duration::from_millis(50)).await;
        }
        
        // Check mempool size after flooding
        let final_response = self.client.get(&mempool_url).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = final_response {
            if resp.status().is_success() {
                let mempool: serde_json::Value = resp.json().await?;
                let final_size = mempool.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
                
                // If mempool grew significantly, check for limits
                if final_size > initial_size + (num_transactions as u64 / 2) {
                    // Try to determine if there's a mempool limit
                    let has_limit = mempool.get("max_size").is_some() || 
                                    mempool.get("limit").is_some() || 
                                    mempool.get("max_transactions").is_some();
                                    
                    if !has_limit {
                        let vuln = Vulnerability {
                            id: "UNLIMITED_MEMPOOL".to_string(),
                            name: "Unlimited Mempool Size".to_string(),
                            description: "The mempool appears to accept a large number of transactions without enforcing limits, which could lead to memory exhaustion.".to_string(),
                            severity: Severity::Medium,
                            exploited: false,
                            affected_component: "Mempool".to_string(),
                            mitigation_recommendations: vec![
                                "Implement a maximum mempool size".to_string(),
                                "Set a minimum transaction fee requirement".to_string(),
                                "Evict lower-fee transactions when mempool is full".to_string(),
                                "Implement per-account transaction limits".to_string(),
                            ],
                            details: Some(serde_json::json!({
                                "initial_size": initial_size,
                                "final_size": final_size,
                                "transactions_sent": num_transactions,
                            })),
                        };
                        
                        info!("Found vulnerability: {}", vuln.name);
                        self.results.add_vulnerability(vuln);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Check for memory exhaustion vulnerabilities
    async fn check_memory_exhaustion(&mut self) -> Result<()> {
        debug!("Checking for memory exhaustion vulnerabilities");
        
        // Skip invasive tests if not allowed
        if !self.config.allow_invasive_tests {
            return Ok(());
        }
        
        // Create a transaction with large data payload
        let url = format!("{}/transactions", self.config.api_url);
        
        // Generate a large data string (about 1MB)
        let large_data = "x".repeat(1_000_000);
        
        let tx = serde_json::json!({
            "sender": "test_address_large",
            "recipient": "test_recipient_large",
            "amount": 0.001,
            "nonce": 100,
            "data": large_data,
        });
        
        let response = self.client.post(&url).json(&tx).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            if resp.status().is_success() {
                // If large transaction is accepted without error, might be vulnerable
                let vuln = Vulnerability {
                    id: "LARGE_TX_ACCEPTED".to_string(),
                    name: "Large Transaction Vulnerability".to_string(),
                    description: "The blockchain accepts transactions with very large data payloads, which could lead to memory exhaustion or denial of service.".to_string(),
                    severity: Severity::Medium,
                    exploited: false,
                    affected_component: "Transaction Validation".to_string(),
                    mitigation_recommendations: vec![
                        "Implement transaction size limits".to_string(),
                        "Scale transaction fees based on size".to_string(),
                        "Set memory limits for transaction processing".to_string(),
                        "Implement proper input validation".to_string(),
                    ],
                    details: None,
                };
                
                info!("Found vulnerability: {}", vuln.name);
                self.results.add_vulnerability(vuln);
            }
        }
        
        Ok(())
    }
    
    /// Check NFT execution security
    async fn check_nft_execution_security(&mut self) -> Result<()> {
        info!("Checking NFT execution security");
        
        // Check for NFT execution sandbox escape
        self.check_nft_sandbox_escape().await?;
        
        // Check for resource limits
        self.check_nft_resource_limits().await?;
        
        Ok(())
    }
    
    /// Check for NFT execution sandbox escape
    async fn check_nft_sandbox_escape(&mut self) -> Result<()> {
        debug!("Checking for NFT execution sandbox escape");
        
        // Create an NFT with potentially dangerous code
        let url = format!("{}/nfts", self.config.api_url);
        
        let nft_data = serde_json::json!({
            "creator": "test_creator",
            "name": "Test NFT",
            "description": "Security test NFT",
            "execution_type": "wasm",
            "code": "console.log(process.env); try { require('fs').readFileSync('/etc/passwd') } catch(e) { console.log(e) }",
        });
        
        let response = self.client.post(&url).json(&nft_data).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            
            // If the NFT is created successfully or returns interesting error
            if status.is_success() || text.contains("fs") || text.contains("passwd") || text.contains("process") {
                let vuln = Vulnerability {
                    id: "NFT_SANDBOX_ESCAPE".to_string(),
                    name: "NFT Execution Sandbox Escape Potential".to_string(),
                    description: "The NFT execution environment may be vulnerable to sandbox escape attempts.".to_string(),
                    severity: Severity::High,
                    exploited: false,
                    affected_component: "NFT Execution".to_string(),
                    mitigation_recommendations: vec![
                        "Implement strict sandboxing for NFT code execution".to_string(),
                        "Use isolated containers for execution".to_string(),
                        "Limit available APIs and system calls".to_string(),
                        "Implement proper code validation before execution".to_string(),
                        "Set resource limits for CPU, memory, and network".to_string(),
                    ],
                    details: Some(serde_json::json!({
                        "response_status": status.as_u16(),
                        "response_excerpt": text.chars().take(200).collect::<String>(),
                    })),
                };
                
                info!("Found vulnerability: {}", vuln.name);
                self.results.add_vulnerability(vuln);
            }
        }
        
        Ok(())
    }
    
    /// Check for NFT resource limits
    async fn check_nft_resource_limits(&mut self) -> Result<()> {
        debug!("Checking for NFT resource limits");
        
        // Check if there are documented resource limits
        let url = format!("{}/nfts/limits", self.config.api_url);
        
        let response = self.client.get(&url).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            if !resp.status().is_success() {
                // If no resource limits documented, add recommendation
                let vuln = Vulnerability {
                    id: "NFT_RESOURCE_LIMITS".to_string(),
                    name: "NFT Resource Limits Recommendation".to_string(),
                    description: "No documented resource limits found for NFT execution.".to_string(),
                    severity: Severity::Medium,
                    exploited: false,
                    affected_component: "NFT Execution".to_string(),
                    mitigation_recommendations: vec![
                        "Implement CPU time limits for NFT execution".to_string(),
                        "Set memory usage limits".to_string(),
                        "Restrict network access".to_string(),
                        "Implement execution timeouts".to_string(),
                        "Monitor and log resource usage".to_string(),
                    ],
                    details: None,
                };
                
                info!("Found recommendation: {}", vuln.name);
                self.results.add_vulnerability(vuln);
            }
        } else {
            // If endpoint not available, add recommendation
            let vuln = Vulnerability {
                id: "NFT_RESOURCE_LIMITS_UNKNOWN".to_string(),
                name: "NFT Resource Limits Unknown".to_string(),
                description: "Could not determine if NFT execution has resource limits.".to_string(),
                severity: Severity::Info,
                exploited: false,
                affected_component: "NFT Execution".to_string(),
                mitigation_recommendations: vec![
                    "Implement CPU time limits for NFT execution".to_string(),
                    "Set memory usage limits".to_string(),
                    "Restrict network access".to_string(),
                    "Implement execution timeouts".to_string(),
                    "Monitor and log resource usage".to_string(),
                ],
                details: None,
            };
            
            info!("Found recommendation: {}", vuln.name);
            self.results.add_vulnerability(vuln);
        }
        
        Ok(())
    }
    
    /// Check MEV protection
    async fn check_mev_protection(&mut self) -> Result<()> {
        info!("Checking MEV protection");
        
        // Check if MEV protection is enabled
        let url = format!("{}/mev-protection", self.config.api_url);
        
        let response = self.client.get(&url).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            if resp.status().is_success() {
                let mev_data: serde_json::Value = resp.json().await?;
                
                // Check if protection is enabled
                let enabled = mev_data
                    .get("enabled")
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false);
                    
                if !enabled {
                    let vuln = Vulnerability {
                        id: "MEV_PROTECTION_DISABLED".to_string(),
                        name: "MEV Protection Disabled".to_string(),
                        description: "MEV protection appears to be disabled, which could allow front-running, sandwich attacks, and other MEV extraction.".to_string(),
                        severity: Severity::Medium,
                        exploited: false,
                        affected_component: "MEV Protection".to_string(),
                        mitigation_recommendations: vec![
                            "Enable MEV protection".to_string(),
                            "Implement transaction ordering fairness mechanisms".to_string(),
                            "Consider using an auction or commit-reveal scheme for transactions".to_string(),
                            "Implement batch auctions for transaction ordering".to_string(),
                        ],
                        details: None,
                    };
                    
                    info!("Found vulnerability: {}", vuln.name);
                    self.results.add_vulnerability(vuln);
                } else {
                    // Check what types of MEV are protected against
                    let protection_types = mev_data
                        .get("protected_types")
                        .and_then(|t| t.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                        .unwrap_or_default();
                        
                    // Check for missing protection types
                    let missing_types = vec!["frontrunning", "sandwich", "arbitrage", "liquidation"]
                        .into_iter()
                        .filter(|&t| !protection_types.iter().any(|pt| pt.contains(t)))
                        .collect::<Vec<_>>();
                        
                    if !missing_types.is_empty() {
                        let vuln = Vulnerability {
                            id: "INCOMPLETE_MEV_PROTECTION".to_string(),
                            name: "Incomplete MEV Protection".to_string(),
                            description: format!("MEV protection is enabled but does not protect against all common attack types. Missing protection for: {}", missing_types.join(", ")),
                            severity: Severity::Low,
                            exploited: false,
                            affected_component: "MEV Protection".to_string(),
                            mitigation_recommendations: vec![
                                "Expand MEV protection to cover additional attack types".to_string(),
                                "Implement front-running protection".to_string(),
                                "Add sandwich attack detection and prevention".to_string(),
                                "Consider implementing custom solutions for specialized MEV types".to_string(),
                            ],
                            details: Some(serde_json::json!({
                                "protected_types": protection_types,
                                "missing_types": missing_types,
                            })),
                        };
                        
                        info!("Found vulnerability: {}", vuln.name);
                        self.results.add_vulnerability(vuln);
                    }
                }
            } else {
                // No MEV protection endpoint
                let vuln = Vulnerability {
                    id: "NO_MEV_PROTECTION".to_string(),
                    name: "No MEV Protection".to_string(),
                    description: "No MEV protection API endpoint found, suggesting MEV protection might not be implemented.".to_string(),
                    severity: Severity::Medium,
                    exploited: false,
                    affected_component: "MEV Protection".to_string(),
                    mitigation_recommendations: vec![
                        "Implement MEV protection mechanisms".to_string(),
                        "Consider using a fair transaction ordering solution".to_string(),
                        "Implement commit-reveal schemes for transaction ordering".to_string(),
                        "Add monitoring for MEV activities".to_string(),
                    ],
                    details: None,
                };
                
                info!("Found vulnerability: {}", vuln.name);
                self.results.add_vulnerability(vuln);
            }
        } else {
            // Could not access endpoint
            let vuln = Vulnerability {
                id: "MEV_PROTECTION_UNKNOWN".to_string(),
                name: "MEV Protection Status Unknown".to_string(),
                description: "Could not determine if MEV protection is implemented.".to_string(),
                severity: Severity::Info,
                exploited: false,
                affected_component: "MEV Protection".to_string(),
                mitigation_recommendations: vec![
                    "Implement MEV protection mechanisms".to_string(),
                    "Consider using a fair transaction ordering solution".to_string(),
                    "Implement commit-reveal schemes for transaction ordering".to_string(),
                    "Add monitoring for MEV activities".to_string(),
                ],
                details: None,
            };
            
            info!("Found recommendation: {}", vuln.name);
            self.results.add_vulnerability(vuln);
        }
        
        Ok(())
    }
    
    /// Check wallet security
    async fn check_wallet_security(&mut self) -> Result<()> {
        info!("Checking wallet security");
        
        // Check wallet encryption
        self.check_wallet_encryption().await?;
        
        // Check key derivation
        self.check_key_derivation().await?;
        
        Ok(())
    }
    
    /// Check wallet encryption
    async fn check_wallet_encryption(&mut self) -> Result<()> {
        debug!("Checking wallet encryption");
        
        // Try to access wallet without authentication
        let url = format!("{}/wallet", self.config.api_url);
        
        let response = self.client.get(&url).send().await;
        self.results.total_requests += 1;
        
        if let Ok(resp) = response {
            if resp.status().is_success() {
                // If wallet info is accessible without authentication
                let wallet_data: serde_json::Value = resp.json().await?;
                
                if wallet_data.get("addresses").is_some() || 
                   wallet_data.get("balance").is_some() ||
                   wallet_data.get("keys").is_some() {
                    
                    let vuln = Vulnerability {
                        id: "WALLET_ACCESS".to_string(),
                        name: "Unprotected Wallet Access".to_string(),
                        description: "Wallet information is accessible without proper authentication.".to_string(),
                        severity: Severity::Critical,
                        exploited: false,
                        affected_component: "Wallet".to_string(),
                        mitigation_recommendations: vec![
                            "Implement authentication for wallet API access".to_string(),
                            "Use strong encryption for stored wallet data".to_string(),
                            "Never expose private keys via API".to_string(),
                            "Implement proper access controls".to_string(),
                        ],
                        details: None,
                    };
                    
                    info!("Found vulnerability: {}", vuln.name);
                    self.results.add_vulnerability(vuln);
                }
                
                // Check if encryption is used
                let is_encrypted = wallet_data
                    .get("encrypted")
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false);
                    
                if !is_encrypted {
                    let vuln = Vulnerability {
                        id: "WALLET_ENCRYPTION".to_string(),
                        name: "Wallet Encryption Missing".to_string(),
                        description: "The wallet data does not appear to be encrypted.".to_string(),
                        severity: Severity::High,
                        exploited: false,
                        affected_component: "Wallet".to_string(),
                        mitigation_recommendations: vec![
                            "Implement strong encryption for wallet data".to_string(),
                            "Use industry-standard encryption algorithms".to_string(),
                            "Store encryption keys securely".to_string(),
                            "Consider using a hardware security module (HSM)".to_string(),
                        ],
                        details: None,
                    };
                    
                    info!("Found vulnerability: {}", vuln.name);
                    self.results.add_vulnerability(vuln);
                }
            }
        }
        
        Ok(())
    }
    
    /// Check key derivation
    async fn check_key_derivation(&mut self) -> Result<()> {
        debug!("Checking key derivation");
        
        // This is mostly a recommendation
        let vuln = Vulnerability {
            id: "KEY_DERIVATION_CHECK".to_string(),
            name: "Key Derivation Check Recommended".to_string(),
            description: "Recommendation to manually audit the key derivation process for security best practices.".to_string(),
            severity: Severity::Info,
            exploited: false,
            affected_component: "Wallet".to_string(),
            mitigation_recommendations: vec![
                "Use BIP-32/39/44 standards for key derivation".to_string(),
                "Implement proper entropy sources for key generation".to_string(),
                "Use strong key derivation functions (e.g., PBKDF2, scrypt)".to_string(),
                "Perform regular security audits of cryptographic implementations".to_string(),
            ],
            details: None,
        };
        
        info!("Found recommendation: {}", vuln.name);
        self.results.add_vulnerability(vuln);
        
        Ok(())
    }
}

/// Run a security audit on a Wrfcoin node
pub async fn run_security_audit(
    api_url: &str,
    allow_invasive_tests: bool,
) -> Result<()> {
    info!("Starting Wrfcoin security audit");
    
    let config = SecurityTestConfig {
        api_url: api_url.to_string(),
        allow_invasive_tests,
        exploit_vulnerabilities: false,
        max_concurrent_requests: 10,
        request_timeout_seconds: 30,
        custom_headers: HashMap::new(),
    };
    
    let mut tester = SecurityTestRunner::new(config);
    tester.run_all_tests().await?;
    
    // Generate detailed report
    let report = tester.results().generate_report();
    println!("\n{}", report);
    
    info!("Security audit completed");
    
    Ok(())
}