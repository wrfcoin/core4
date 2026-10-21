use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use log::{info, warn};
use crate::output::format_wrf_units;
use rand::{Rng, thread_rng};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Transaction validation options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationOptions {
    /// Number of confirmations required for transaction acceptance
    pub required_confirmations: u64,
    /// Maximum allowed transaction age (seconds)
    pub max_transaction_age: u64,
    /// Whether to strictly validate transaction nonces
    pub strict_nonce_validation: bool,
    /// Whether to use enhanced validation for high-value transactions
    pub enhanced_validation_for_high_value: bool,
    /// High value threshold in base units (1 WRF = 100_000_000 units)
    pub high_value_threshold: u64,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            required_confirmations: 6,
            max_transaction_age: 3600, // 1 hour
            strict_nonce_validation: true,
            enhanced_validation_for_high_value: true,
            high_value_threshold: 100_000_000_000, // 1000 WRF in base units
        }
    }
}

/// Anti-DDoS options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AntiDDoSOptions {
    /// Maximum requests per minute per IP
    pub max_requests_per_minute: u32,
    /// Maximum concurrent connections per IP
    pub max_concurrent_connections: u32,
    /// Automatic blacklist time for suspicious IPs (seconds)
    pub blacklist_time: u64,
    /// Proof-of-work difficulty for new connections (0 = disabled)
    pub pow_difficulty: u8,
    /// Whether to enable connection throttling
    pub enable_throttling: bool,
}

impl Default for AntiDDoSOptions {
    fn default() -> Self {
        Self {
            max_requests_per_minute: 120,
            max_concurrent_connections: 10,
            blacklist_time: 300, // 5 minutes
            pow_difficulty: 0,
            enable_throttling: true,
        }
    }
}

/// Checkpoint block information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckpointBlock {
    /// Block height
    pub height: u64,
    /// Block hash
    pub hash: String,
    /// Timestamp
    pub timestamp: String,
}

/// Transaction verification result
#[derive(Clone, Debug, PartialEq)]
pub enum VerificationResult {
    /// Transaction is valid
    Valid,
    /// Transaction is invalid with reason
    Invalid(String),
    /// Transaction requires additional verification
    NeedsAdditionalVerification(String),
}

/// Security service for Wrfcoin
#[allow(clippy::disallowed_types)]
pub struct SecurityService {
    /// Validation options
    validation_options: ValidationOptions,
    /// Anti-DDoS options
    anti_ddos_options: AntiDDoSOptions,
    /// Checkpoint blocks
    checkpoints: Vec<CheckpointBlock>,
    /// Blacklisted IPs (IP -> expiration time)
    blacklisted_ips: Arc<Mutex<HashMap<String, Instant>>>,
    /// Rate limiter (IP -> (last request time, request count))
    rate_limiter: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
    /// Concurrent connections (IP -> connection count)
    concurrent_connections: Arc<Mutex<HashMap<String, u32>>>,
    /// Used nonces (sender address -> HashSet of used nonces)
    used_nonces: Arc<Mutex<HashMap<String, HashSet<u64>>>>,
}

impl Default for SecurityService {
    fn default() -> Self {
        Self::new(
            ValidationOptions::default(),
            AntiDDoSOptions::default(),
            Vec::new(),
        )
    }
}

impl SecurityService {
    /// Create a new security service
    #[allow(clippy::disallowed_types)]
    pub fn new(
        validation_options: ValidationOptions,
        anti_ddos_options: AntiDDoSOptions,
        checkpoints: Vec<CheckpointBlock>,
    ) -> Self {
        Self {
            validation_options,
            anti_ddos_options,
            checkpoints,
            blacklisted_ips: Arc::new(Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(Mutex::new(HashMap::new())),
            concurrent_connections: Arc::new(Mutex::new(HashMap::new())),
            used_nonces: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a checkpoint block
    pub fn add_checkpoint(&mut self, checkpoint: CheckpointBlock) {
        let height = checkpoint.height;
        self.checkpoints.push(checkpoint);
        self.checkpoints.sort_by_key(|c| c.height);
        info!("Added checkpoint block at height {height}");
    }

    /// Get the latest checkpoint block
    pub fn get_latest_checkpoint(&self) -> Option<&CheckpointBlock> {
        self.checkpoints.last()
    }

    /// Get checkpoint at or before a given height
    pub fn get_checkpoint_at_or_before_height(&self, height: u64) -> Option<&CheckpointBlock> {
        self.checkpoints.iter().rev().find(|c| c.height <= height)
    }

    /// Check if a block is a checkpoint
    pub fn is_checkpoint(&self, height: u64) -> bool {
        self.checkpoints.iter().any(|c| c.height == height)
    }

    /// Verify that a block matches the checkpoint hash
    pub fn verify_checkpoint(&self, height: u64, hash: &str) -> Result<bool> {
        if let Some(checkpoint) = self.checkpoints.iter().find(|c| c.height == height) {
            Ok(checkpoint.hash == hash)
        } else {
            Ok(true) // Not a checkpoint block, so valid
        }
    }

    /// Check if a connection is allowed based on Anti-DDoS rules
    pub fn check_connection_allowed(&self, ip: &str) -> Result<bool> {
        // Check if IP is blacklisted
        {
            let mut blacklisted_ips = self.blacklisted_ips.lock().unwrap();

            // Clean up expired blacklist entries
            blacklisted_ips.retain(|_, expiration| *expiration > Instant::now());

            // Check if IP is blacklisted
            if let Some(expiration) = blacklisted_ips.get(ip) {
                let remaining = expiration
                    .checked_duration_since(Instant::now())
                    .unwrap_or(Duration::from_secs(0));

                warn!(
                    "Connection from blacklisted IP {ip}: {} seconds remaining on blacklist",
                    remaining.as_secs()
                );
                return Ok(false);
            }
        }

        // Check rate limiting
        if self.anti_ddos_options.enable_throttling {
            let mut rate_limiter = self.rate_limiter.lock().unwrap();

            let now = Instant::now();
            let (last_request, count) = rate_limiter.entry(ip.to_string()).or_insert((now, 0));

            // Reset counter if minute has passed
            if last_request
                .checked_duration_since(now)
                .unwrap_or(Duration::from_secs(0))
                .as_secs()
                >= 60
            {
                *last_request = now;
                *count = 1;
            } else {
                // Increment request count
                *count += 1;

                // Check if limit exceeded
                if *count > self.anti_ddos_options.max_requests_per_minute {
                    warn!(
                        "Rate limit exceeded for IP {ip}: {} requests/minute",
                        *count
                    );

                    // Blacklist IP if threshold significantly exceeded
                    #[allow(clippy::excessive_nesting)]
                    if *count > self.anti_ddos_options.max_requests_per_minute * 2 {
                        let mut blacklisted_ips = self.blacklisted_ips.lock().unwrap();
                        blacklisted_ips.insert(
                            ip.to_string(),
                            Instant::now()
                                + Duration::from_secs(self.anti_ddos_options.blacklist_time),
                        );
                        warn!(
                            "IP {ip} blacklisted for {} seconds due to rate limit violation",
                            self.anti_ddos_options.blacklist_time
                        );
                    }

                    return Ok(false);
                }
            }
        }

        // Check concurrent connections
        {
            let mut concurrent_connections = self.concurrent_connections.lock().unwrap();
            let conn_count = concurrent_connections.entry(ip.to_string()).or_insert(0);

            // Increment connection count
            *conn_count += 1;

            // Check if limit exceeded
            if *conn_count > self.anti_ddos_options.max_concurrent_connections {
                warn!(
                    "Too many concurrent connections from IP {ip}: {}",
                    *conn_count
                );

                // Decrement connection count since it's being rejected
                *conn_count -= 1;

                return Ok(false);
            }
        }

        // Connection allowed
        Ok(true)
    }

    /// Release a connection
    pub fn release_connection(&self, ip: &str) {
        let mut concurrent_connections = self.concurrent_connections.lock().unwrap();
        if let Some(conn_count) = concurrent_connections.get_mut(ip) {
            if *conn_count > 0 {
                *conn_count -= 1;
            }
        }
    }

    /// Blacklist an IP address
    pub fn blacklist_ip(&self, ip: &str, duration_seconds: u64) {
        let mut blacklisted_ips = self.blacklisted_ips.lock().unwrap();
        blacklisted_ips.insert(
            ip.to_string(),
            Instant::now() + Duration::from_secs(duration_seconds),
        );
        warn!("IP {ip} blacklisted for {duration_seconds} seconds");
    }

    /// Check if a nonce is valid and not reused
    pub fn check_nonce_valid(&self, sender: &str, nonce: u64) -> Result<bool> {
        if !self.validation_options.strict_nonce_validation {
            return Ok(true);
        }

        let mut used_nonces = self.used_nonces.lock().unwrap();
        let sender_nonces = used_nonces
            .entry(sender.to_string())
            .or_default();

        // Check if nonce is used
        if sender_nonces.contains(&nonce) {
            warn!("Nonce {nonce} already used for sender {sender}");
            return Ok(false);
        }

        // Add nonce to used set
        sender_nonces.insert(nonce);

        Ok(true)
    }

    /// Generate a secure random nonce
    pub fn generate_nonce() -> u64 {
        let mut rng = thread_rng();
        rng.gen()
    }

    /// Generate a time-based nonce with randomness
    pub fn generate_time_based_nonce() -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        let mut rng = thread_rng();
        let random = rng.gen::<u32>();

        // Combine time and randomness
        (now << 32) | (random as u64)
    }

    /// Verify transaction
    pub fn verify_transaction(
        &self,
        tx_data: &crate::crypto::TransactionSigningData,
        _signature: &str,
    ) -> Result<VerificationResult> {
        // Check nonce
        if !self.check_nonce_valid(&tx_data.from, tx_data.nonce)? {
            return Ok(VerificationResult::Invalid(
                "Invalid or reused nonce".to_string(),
            ));
        }

        // Check transaction age
        let tx_time = DateTime::parse_from_rfc3339(&tx_data.timestamp)
            .map_err(|e| anyhow!("Invalid transaction timestamp: {e}"))?;

        let now = Utc::now();
        let age = now.signed_duration_since(tx_time.with_timezone(&Utc));

        if age.num_seconds() < 0 {
            return Ok(VerificationResult::Invalid(
                "Transaction timestamp is in the future".to_string(),
            ));
        }

        if age.num_seconds() as u64 > self.validation_options.max_transaction_age {
            return Ok(VerificationResult::Invalid(
                "Transaction is too old".to_string(),
            ));
        }

        // Additional verification for high-value transactions
        if self.validation_options.enhanced_validation_for_high_value
            && tx_data.amount > self.validation_options.high_value_threshold
        {
            return Ok(VerificationResult::NeedsAdditionalVerification(format!(
                "High-value transaction ({}) requires additional verification",
                format_wrf_units(tx_data.amount)
            )));
        }

        // Transaction is valid
        Ok(VerificationResult::Valid)
    }

    /// Generate a proof-of-work challenge
    pub fn generate_pow_challenge(&self) -> (String, u8) {
        let mut rng = thread_rng();
        let nonce: u64 = rng.gen();

        let challenge = format!("wrfcoin-pow-{nonce}");
        (challenge, self.anti_ddos_options.pow_difficulty)
    }

    /// Verify a proof-of-work solution
    pub fn verify_pow_solution(&self, _challenge: &str, solution: &str, difficulty: u8) -> bool {
        if difficulty == 0 {
            return true; // PoW disabled
        }

        // Verify solution has required number of leading zeros
        let expected_prefix = "0".repeat(difficulty as usize);

        // In a real implementation, we would verify the hash of challenge + solution
        // For now, we just check if the solution starts with the expected prefix
        solution.starts_with(&expected_prefix)
    }

    /// Clear expired data (used nonces, etc)
    pub fn clear_expired_data(&self) {
        // Clear expired blacklisted IPs
        {
            let mut blacklisted_ips = self.blacklisted_ips.lock().unwrap();
            blacklisted_ips.retain(|_, expiration| *expiration > Instant::now());
        }

        // Clear expired rate limiter entries
        {
            let mut rate_limiter = self.rate_limiter.lock().unwrap();
            let now = Instant::now();
            rate_limiter.retain(|_, (last_request, _)| {
                last_request
                    .checked_duration_since(now)
                    .unwrap_or(Duration::from_secs(0))
                    .as_secs()
                    < 3600 // 1 hour
            });
        }

        // For a full implementation, we would also need to clear old used nonces
        // but for that we need transaction finality information
    }
}

/// Blockchain consensus result
#[derive(Clone, Debug, PartialEq)]
pub enum ConsensusResult {
    /// Block is valid
    Valid,
    /// Block is invalid with reason
    Invalid(String),
    /// Need more information to determine validity
    Indeterminate(String),
}

/// Block difficulty adjustment calculator
pub struct DifficultyAdjustment {
    /// Target block time in seconds
    target_block_time: u64,
    /// Adjustment window size (in blocks)
    #[allow(dead_code)]
    adjustment_window: u64,
    /// Maximum difficulty change factor (percentage)
    max_adjustment_factor: f64,
}

impl Default for DifficultyAdjustment {
    fn default() -> Self {
        Self {
            target_block_time: 15,       // 15 seconds
            adjustment_window: 2016,     // About 2 weeks
            max_adjustment_factor: 0.25, // 25% change max
        }
    }
}

impl DifficultyAdjustment {
    /// Create a new difficulty adjustment calculator
    pub fn new(target_block_time: u64, adjustment_window: u64, max_adjustment_factor: f64) -> Self {
        Self {
            target_block_time,
            adjustment_window,
            max_adjustment_factor,
        }
    }

    /// Calculate new difficulty based on historical block times
    pub fn calculate_next_difficulty(
        &self,
        current_difficulty: u64,
        block_timestamps: &[u64],
    ) -> u64 {
        if block_timestamps.len() < 2 {
            return current_difficulty; // Not enough blocks to adjust
        }

        // Calculate average block time
        let mut total_time = 0;
        for i in 1..block_timestamps.len() {
            let time_diff = if block_timestamps[i] > block_timestamps[i - 1] {
                block_timestamps[i] - block_timestamps[i - 1]
            } else {
                // Handle timestamp errors (possible attack vector)
                warn!(
                    "Block timestamp inconsistency detected! Block {i} has earlier timestamp than block {}",
                    i - 1
                );
                self.target_block_time // Use target time as fallback
            };

            total_time += time_diff;
        }

        let average_block_time = total_time as f64 / (block_timestamps.len() - 1) as f64;

        // Calculate adjustment factor
        let mut adjustment_factor = self.target_block_time as f64 / average_block_time;

        // Clamp adjustment to max change
        if adjustment_factor > 1.0 + self.max_adjustment_factor {
            adjustment_factor = 1.0 + self.max_adjustment_factor;
        } else if adjustment_factor < 1.0 - self.max_adjustment_factor {
            adjustment_factor = 1.0 - self.max_adjustment_factor;
        }

        // Apply adjustment
        let new_difficulty = (current_difficulty as f64 * adjustment_factor).round() as u64;

        // Ensure minimum difficulty of 1
        new_difficulty.max(1)
    }
}

/// Chain reorganization validator
pub struct ReorgValidator {
    /// Maximum allowed reorganization depth
    pub max_reorg_depth: u64,
    /// Whether checkpoints prevent reorganizations past them
    pub checkpoints_prevent_reorg: bool,
    /// Maximum difficulty change for a reorganization
    pub max_difficulty_change: f64,
}

impl Default for ReorgValidator {
    fn default() -> Self {
        Self {
            max_reorg_depth: 100,
            checkpoints_prevent_reorg: true,
            max_difficulty_change: 0.5, // 50% change max
        }
    }
}

impl ReorgValidator {
    /// Create a new reorganization validator
    pub fn new(
        max_reorg_depth: u64,
        checkpoints_prevent_reorg: bool,
        max_difficulty_change: f64,
    ) -> Self {
        Self {
            max_reorg_depth,
            checkpoints_prevent_reorg,
            max_difficulty_change,
        }
    }

    /// Validate a chain reorganization
    pub fn validate_reorg(
        &self,
        current_height: u64,
        fork_height: u64,
        current_cumulative_difficulty: u64,
        fork_cumulative_difficulty: u64,
        security_service: &SecurityService,
    ) -> ConsensusResult {
        // Calculate reorg depth
        let reorg_depth = current_height - fork_height;

        // Check reorg depth
        if reorg_depth > self.max_reorg_depth {
            return ConsensusResult::Invalid(format!(
                "Reorganization depth ({reorg_depth}) exceeds maximum allowed ({})",
                self.max_reorg_depth
            ));
        }

        // Check checkpoint constraint
        if self.checkpoints_prevent_reorg {
            if let Some(checkpoint) =
                security_service.get_checkpoint_at_or_before_height(current_height)
            {
                if checkpoint.height >= fork_height {
                    return ConsensusResult::Invalid(format!(
                        "Reorganization would revert checkpoint at height {}",
                        checkpoint.height
                    ));
                }
            }
        }

        // Check difficulty change
        if current_cumulative_difficulty > 0 && fork_cumulative_difficulty > 0 {
            let difficulty_change = if fork_cumulative_difficulty > current_cumulative_difficulty {
                (fork_cumulative_difficulty as f64 / current_cumulative_difficulty as f64) - 1.0
            } else {
                (current_cumulative_difficulty as f64 / fork_cumulative_difficulty as f64) - 1.0
            };

            if difficulty_change > self.max_difficulty_change {
                return ConsensusResult::Invalid(format!(
                    "Reorganization difficulty change ({:.2}%) exceeds maximum allowed ({:.2}%)",
                    difficulty_change * 100.0,
                    self.max_difficulty_change * 100.0
                ));
            }
        }

        // Reorg is valid
        ConsensusResult::Valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonce_validation() {
        let security = SecurityService::default();

        // First use of nonce should be valid
        assert!(security.check_nonce_valid("address1", 12345).unwrap());

        // Second use of same nonce should be invalid
        assert!(!security.check_nonce_valid("address1", 12345).unwrap());

        // Different nonce for same address should be valid
        assert!(security.check_nonce_valid("address1", 67890).unwrap());

        // Same nonce for different address should be valid
        assert!(security.check_nonce_valid("address2", 12345).unwrap());
    }

    #[test]
    fn test_checkpoint_verification() {
        let mut security = SecurityService::default();

        // Add a checkpoint
        security.add_checkpoint(CheckpointBlock {
            height: 1000,
            hash: "checkpoint_hash".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        });

        // Verify checkpoint with correct hash
        assert!(security.verify_checkpoint(1000, "checkpoint_hash").unwrap());

        // Verify checkpoint with incorrect hash
        assert!(!security.verify_checkpoint(1000, "wrong_hash").unwrap());

        // Verify non-checkpoint block (should be true)
        assert!(security.verify_checkpoint(1001, "any_hash").unwrap());
    }

    #[test]
    fn test_difficulty_adjustment() {
        let difficulty_adjuster = DifficultyAdjustment::default();

        // Test with block times exactly matching target
        let timestamps = vec![0, 15, 30, 45, 60];
        assert_eq!(
            difficulty_adjuster.calculate_next_difficulty(1000, &timestamps),
            1000
        );

        // Test with block times faster than target
        let fast_timestamps = vec![0, 7, 14, 21, 28];
        let fast_adjustment = difficulty_adjuster.calculate_next_difficulty(1000, &fast_timestamps);
        assert!(fast_adjustment > 1000); // Difficulty should increase

        // Test with block times slower than target
        let slow_timestamps = vec![0, 30, 60, 90, 120];
        let slow_adjustment = difficulty_adjuster.calculate_next_difficulty(1000, &slow_timestamps);
        assert!(slow_adjustment < 1000); // Difficulty should decrease

        // Test max adjustment cap
        let very_fast_timestamps = vec![0, 1, 2, 3, 4];
        let very_fast_adjustment =
            difficulty_adjuster.calculate_next_difficulty(1000, &very_fast_timestamps);
        assert_eq!(very_fast_adjustment, 1250); // 25% max increase
    }

    #[test]
    fn test_reorg_validation() {
        let security = SecurityService::default();
        let reorg_validator = ReorgValidator::default();

        // Valid reorg within limits
        let result = reorg_validator.validate_reorg(1000, 950, 100000, 110000, &security);
        assert_eq!(result, ConsensusResult::Valid);

        // Invalid reorg: too deep
        let deep_result = reorg_validator.validate_reorg(1000, 800, 100000, 110000, &security);
        assert!(matches!(deep_result, ConsensusResult::Invalid(_)));

        // Invalid reorg: too large difficulty change
        let large_diff_result =
            reorg_validator.validate_reorg(1000, 950, 100000, 200000, &security);
        assert!(matches!(large_diff_result, ConsensusResult::Invalid(_)));
    }
}
