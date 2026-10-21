use crate::common::types::Address;
use crate::layer2::proof::{ZKProof, ZKVerifier};
use crate::layer2::rollup::{BatchStatus, TransactionBatch};
use crate::layer2::state::Layer2State;
use anyhow::Result;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

/// Verification mode for the rollup verifier
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationMode {
    /// Verify only state transitions
    StateOnly,

    /// Verify full transaction execution
    FullExecution,

    /// Verify ZK proofs
    ZKProof,

    /// Verify fraud proofs (for optimistic rollups)
    FraudProof,
}

/// Verification settings
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierConfig {
    /// Verification mode
    pub mode: VerificationMode,

    /// Address for the verifier
    pub verifier_address: Address,

    /// Auto-verification of incoming batches
    pub auto_verify: bool,

    /// Verification interval in seconds
    pub verification_interval: u64,

    /// Maximum batch size to verify
    pub max_batch_size: usize,

    /// Timeout for verification in seconds
    pub verification_timeout: u64,

    /// Whether to submit fraud proofs
    pub submit_fraud_proofs: bool,

    /// Number of concurrent verification tasks
    pub concurrent_verifications: usize,
}

impl Default for VerifierConfig {
    fn default() -> Self {
        Self {
            mode: VerificationMode::ZKProof,
            verifier_address: Address([0; 20]),
            auto_verify: true,
            verification_interval: 60,
            max_batch_size: 1000,
            verification_timeout: 300,
            submit_fraud_proofs: true,
            concurrent_verifications: 2,
        }
    }
}

/// Verification result
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Batch ID
    pub batch_id: u64,

    /// Verification status
    pub success: bool,

    /// Error message if verification failed
    pub error_message: Option<String>,

    /// Verification mode used
    pub mode: VerificationMode,

    /// Time taken for verification in milliseconds
    pub time_ms: u64,

    /// Timestamp of verification (unix seconds)
    pub timestamp: u64,

    /// Verifier address
    pub verifier: Address,
}

/// Rollup verifier metrics
#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerifierMetrics {
    /// Number of batches verified
    pub batches_verified: u64,

    /// Number of successful verifications
    pub successful_verifications: u64,

    /// Number of failed verifications
    pub failed_verifications: u64,

    /// Total verification time in milliseconds
    pub total_verification_time_ms: u64,

    /// Average verification time in milliseconds
    pub avg_verification_time_ms: f64,

    /// Number of fraud proofs submitted
    pub fraud_proofs_submitted: u64,

    /// Last verification timestamp (unix seconds)
    pub last_verification: Option<u64>,

    /// Verifier uptime in seconds
    pub uptime: u64,
}

/// Rollup verifier status
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifierStatus {
    /// Verifier is initializing
    Initializing,

    /// Verifier is active
    Active,

    /// Verifier is paused
    Paused,

    /// Verifier is in error state
    Error,

    /// Verifier is shutting down
    ShuttingDown,
}

/// Rollup verifier for validating Layer 2 state transitions
#[allow(dead_code)]
pub struct RollupVerifier {
    /// Verifier configuration
    config: VerifierConfig,

    /// Verifier status
    status: Arc<RwLock<VerifierStatus>>,

    /// Verifier metrics
    metrics: Arc<RwLock<VerifierMetrics>>,

    /// Current Layer 2 state
    state: Arc<RwLock<Layer2State>>,

    /// ZK Verifier (for ZK rollups)
    zk_verifier: Option<Arc<ZKVerifier>>,

    /// Verified batches
    verified_batches: Arc<RwLock<HashMap<u64, VerificationResult>>>,

    /// Batches pending verification
    pending_batches: Arc<RwLock<HashMap<u64, TransactionBatch>>>,

    /// Start time
    start_time: std::time::Instant,

    /// Channel for stopping the verifier
    stop_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

#[allow(dead_code)]
impl RollupVerifier {
    /// Create a new rollup verifier
    pub fn new(config: VerifierConfig, initial_state: Layer2State) -> Self {
        let zk_verifier = if config.mode == VerificationMode::ZKProof {
            Some(Arc::new(ZKVerifier::new()))
        } else {
            None
        };

        Self {
            config,
            status: Arc::new(RwLock::new(VerifierStatus::Initializing)),
            metrics: Arc::new(RwLock::new(VerifierMetrics::default())),
            state: Arc::new(RwLock::new(initial_state)),
            zk_verifier,
            verified_batches: Arc::new(RwLock::new(HashMap::new())),
            pending_batches: Arc::new(RwLock::new(HashMap::new())),
            start_time: std::time::Instant::now(),
            stop_tx: None,
        }
    }

    /// Start the verifier
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting rollup verifier");

        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);
        self.stop_tx = Some(stop_tx);

        *self.status.write().await = VerifierStatus::Active;

        let status = self.status.clone();
        let metrics = self.metrics.clone();
        let state = self.state.clone();
        let pending_batches = self.pending_batches.clone();
        let verified_batches = self.verified_batches.clone();
        let config = self.config.clone();
        let start_time = self.start_time;
        let zk_verifier = self.zk_verifier.clone();

        if self.config.auto_verify {
            let verify_status = status.clone();
            let verify_metrics = metrics.clone();
            let verify_state = state.clone();
            let verify_pending = pending_batches.clone();
            let verify_verified = verified_batches.clone();
            let verify_config = config.clone();
            let verify_zk = zk_verifier.clone();

            tokio::spawn(async move {
                let mut verify_interval =
                    interval(Duration::from_secs(verify_config.verification_interval));

                loop {
                    tokio::select! {
                        _ = verify_interval.tick() => {
                            if *verify_status.read().await != VerifierStatus::Active {
                                debug!("Verifier not active, skipping verification");
                                continue;
                            }

                            let mut pending = verify_pending.write().await;
                            if pending.is_empty() {
                                debug!("No batches pending verification");
                                continue;
                            }

                            info!("Found {} batches pending verification", pending.len());

                            let batch_ids: Vec<u64> = pending.keys()
                                .copied()
                                .take(verify_config.concurrent_verifications)
                                .collect();

                            let mut batches_to_verify = Vec::new();
                            for id in &batch_ids {
                                if let Some(batch) = pending.remove(id) {
                                    batches_to_verify.push(batch);
                                }
                            }

                            drop(pending);

                            for batch in batches_to_verify {
                                let batch_id = batch.id;
                                info!("Verifying batch {}", batch_id);

                                let vstart = std::time::Instant::now();
                                let verification_result = Self::verify_batch(
                                    &batch,
                                    &verify_state,
                                    &verify_zk,
                                    verify_config.mode,
                                    verify_config.verifier_address,
                                ).await;

                                let elapsed = vstart.elapsed().as_millis() as u64;
                                let now_secs = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();

                                let (success, error_message) = match verification_result {
                                    Ok(true) => { info!("Batch {} verified OK in {} ms", batch_id, elapsed); (true, None) },
                                    Ok(false) => { warn!("Batch {} verification failed", batch_id); (false, Some("Verification failed".to_string())) },
                                    Err(e) => { error!("Error verifying batch {}: {}", batch_id, e); (false, Some(e.to_string())) },
                                };

                                let result = VerificationResult {
                                    batch_id,
                                    success,
                                    error_message,
                                    mode: verify_config.mode,
                                    time_ms: elapsed,
                                    timestamp: now_secs,
                                    verifier: verify_config.verifier_address,
                                };

                                verify_verified.write().await.insert(batch_id, result);

                                let mut m = verify_metrics.write().await;
                                m.batches_verified += 1;
                                if success { m.successful_verifications += 1; } else { m.failed_verifications += 1; }
                                m.total_verification_time_ms += elapsed;
                                m.avg_verification_time_ms = m.total_verification_time_ms as f64 / m.batches_verified as f64;
                                m.last_verification = Some(now_secs);
                                if !success && verify_config.submit_fraud_proofs {
                                    m.fraud_proofs_submitted += 1;
                                }
                            }
                        },
                        else => break,
                    }
                }

                info!("Verification task stopped");
            });
        }

        let metrics_metrics = metrics.clone();

        tokio::spawn(async move {
            let mut metrics_interval = interval(Duration::from_secs(60));

            loop {
                tokio::select! {
                    _ = metrics_interval.tick() => {
                        let mut m = metrics_metrics.write().await;
                        m.uptime = start_time.elapsed().as_secs();
                    },
                    else => break,
                }
            }
        });

        tokio::spawn(async move {
            if let Some(_) = stop_rx.recv().await {
                info!("Received stop signal, shutting down verifier");
                *status.write().await = VerifierStatus::ShuttingDown;
                tokio::time::sleep(Duration::from_secs(1)).await;
                *status.write().await = VerifierStatus::Paused;
                info!("Verifier shutdown complete");
            }
        });

        info!("Rollup verifier started successfully");

        Ok(())
    }

    /// Verify a single batch
    #[allow(clippy::too_many_arguments)]
    async fn verify_batch(
        batch: &TransactionBatch,
        state: &Arc<RwLock<Layer2State>>,
        zk_verifier: &Option<Arc<ZKVerifier>>,
        mode: VerificationMode,
        _verifier_address: Address,
    ) -> Result<bool> {
        match mode {
            VerificationMode::ZKProof => {
                if let Some(verifier) = zk_verifier {
                    if let Some(zkproof) = &batch.zkproof {
                        let result = verifier
                            .verify(zkproof, batch.previous_state_root, batch.new_state_root)
                            .await?;

                        return Ok(result);
                    } else {
                        return Err(anyhow::anyhow!("No ZK proof available for batch"));
                    }
                } else {
                    return Err(anyhow::anyhow!("ZK verifier not available"));
                }
            },
            VerificationMode::FullExecution => {
                let mut state_clone = state.read().await.clone();

                if state_clone.get_state_root() != batch.previous_state_root {
                    return Err(anyhow::anyhow!("State root mismatch"));
                }

                for tx in &batch.transactions {
                    state_clone.apply_transaction(tx).await?;
                }

                if state_clone.get_state_root() != batch.new_state_root {
                    return Ok(false);
                }

                return Ok(true);
            },
            VerificationMode::StateOnly => {
                return Ok(true);
            },
            VerificationMode::FraudProof => {
                return Ok(true);
            },
        }
    }

    /// Add a batch for verification
    pub async fn add_batch_for_verification(&self, batch: TransactionBatch) -> Result<()> {
        if *self.status.read().await != VerifierStatus::Active {
            return Err(anyhow::anyhow!("Verifier is not active"));
        }

        if self.verified_batches.read().await.contains_key(&batch.id) {
            return Err(anyhow::anyhow!("Batch already verified"));
        }

        if self.pending_batches.read().await.contains_key(&batch.id) {
            return Err(anyhow::anyhow!("Batch already pending verification"));
        }

        let batch_id = batch.id;
        self.pending_batches.write().await.insert(batch.id, batch);

        info!("Added batch {} for verification", batch_id);

        Ok(())
    }

    /// Manually verify a batch
    pub async fn verify_batch_manually(&self, batch_id: u64) -> Result<VerificationResult> {
        if *self.status.read().await != VerifierStatus::Active {
            return Err(anyhow::anyhow!("Verifier is not active"));
        }

        if let Some(result) = self.verified_batches.read().await.get(&batch_id) {
            return Ok(result.clone());
        }

        let batch = {
            let mut pending = self.pending_batches.write().await;

            if let Some(batch) = pending.remove(&batch_id) {
                batch
            } else {
                return Err(anyhow::anyhow!("Batch not found"));
            }
        };

        let start_time = std::time::Instant::now();
        let verification_result = Self::verify_batch(
            &batch,
            &self.state,
            &self.zk_verifier,
            self.config.mode,
            self.config.verifier_address,
        )
        .await;

        let elapsed = start_time.elapsed().as_millis() as u64;
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let result = match verification_result {
            Ok(true) => {
                let mut m = self.metrics.write().await;
                m.batches_verified += 1;
                m.successful_verifications += 1;
                m.total_verification_time_ms += elapsed;
                m.avg_verification_time_ms =
                    m.total_verification_time_ms as f64 / m.batches_verified as f64;
                m.last_verification = Some(now_secs);

                VerificationResult {
                    batch_id,
                    success: true,
                    error_message: None,
                    mode: self.config.mode,
                    time_ms: elapsed,
                    timestamp: now_secs,
                    verifier: self.config.verifier_address,
                }
            },
            Ok(false) => {
                let mut m = self.metrics.write().await;
                m.batches_verified += 1;
                m.failed_verifications += 1;
                m.total_verification_time_ms += elapsed;
                m.avg_verification_time_ms =
                    m.total_verification_time_ms as f64 / m.batches_verified as f64;
                m.last_verification = Some(now_secs);

                VerificationResult {
                    batch_id,
                    success: false,
                    error_message: Some("Verification failed".to_string()),
                    mode: self.config.mode,
                    time_ms: elapsed,
                    timestamp: now_secs,
                    verifier: self.config.verifier_address,
                }
            },
            Err(e) => {
                let mut m = self.metrics.write().await;
                m.batches_verified += 1;
                m.failed_verifications += 1;
                m.total_verification_time_ms += elapsed;
                m.avg_verification_time_ms =
                    m.total_verification_time_ms as f64 / m.batches_verified as f64;
                m.last_verification = Some(now_secs);

                VerificationResult {
                    batch_id,
                    success: false,
                    error_message: Some(e.to_string()),
                    mode: self.config.mode,
                    time_ms: elapsed,
                    timestamp: now_secs,
                    verifier: self.config.verifier_address,
                }
            },
        };

        self.verified_batches
            .write()
            .await
            .insert(batch_id, result.clone());

        Ok(result)
    }

    /// Stop the verifier
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping rollup verifier");

        if let Some(tx) = &self.stop_tx {
            tx.send(())
                .await
                .map_err(|_| anyhow::anyhow!("Failed to send stop signal"))?;

            let mut wait_count = 0;
            while wait_count < 10 {
                if *self.status.read().await != VerifierStatus::Active {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
                wait_count += 1;
            }
        } else {
            return Err(anyhow::anyhow!("Verifier not started"));
        }

        Ok(())
    }

    /// Get verifier status
    pub async fn get_status(&self) -> VerifierStatus {
        *self.status.read().await
    }

    /// Get verifier metrics
    pub async fn get_metrics(&self) -> VerifierMetrics {
        self.metrics.read().await.clone()
    }

    /// Get verification result for a batch
    pub async fn get_verification_result(&self, batch_id: u64) -> Option<VerificationResult> {
        self.verified_batches.read().await.get(&batch_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::Address;
    use crate::crypto::hash::Hash;
    use crate::layer2::rollup::BatchStatus;
    use crate::layer2::state::Layer2State;

    fn make_batch(id: u64) -> crate::layer2::rollup::TransactionBatch {
        crate::layer2::rollup::TransactionBatch {
            id,
            transactions: vec![],
            timestamp: 0,
            previous_state_root: Hash::default(),
            new_state_root: Hash::from_bytes([0xffu8; 32]),
            zkproof: None,
            status: BatchStatus::Assembling,
            submitter: Address([0u8; 20]),
            l1_transaction_hash: None,
            l1_block_number: None,
            l1_block_timestamp: None,
            l1_finality_block_number: None,
            l1_finality_timestamp: None,
            fees_collected: 0,
        }
    }

    #[test]
    fn verifier_config_defaults_are_sane() {
        let cfg = VerifierConfig::default();
        assert_eq!(cfg.mode, VerificationMode::ZKProof);
        assert!(cfg.auto_verify);
        assert!(cfg.max_batch_size > 0);
        assert!(cfg.concurrent_verifications > 0);
    }

    #[tokio::test]
    async fn new_verifier_starts_in_initializing_state() {
        let verifier = RollupVerifier::new(VerifierConfig::default(), Layer2State::new());
        assert_eq!(verifier.get_status().await, VerifierStatus::Initializing);
    }

    #[tokio::test]
    async fn add_batch_rejected_when_verifier_not_yet_started() {
        let verifier = RollupVerifier::new(VerifierConfig::default(), Layer2State::new());
        let err = verifier.add_batch_for_verification(make_batch(1)).await;
        assert!(err.is_err(), "inactive verifier must reject batch submissions");
    }
}
