use crate::blockchain::transaction::Transaction;
use crate::common::types::Address;
use crate::layer2::rollup::{BatchStatus, Rollup, RollupConfig};
use crate::layer2::state::Layer2State;
use anyhow::Result;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::{interval, Duration};

fn checked_sum_u64<I>(values: I, context: &'static str) -> Result<u64>
where
    I: IntoIterator<Item = u64>,
{
    values.into_iter().try_fold(0u64, |acc, value| {
        acc.checked_add(value)
            .ok_or_else(|| anyhow::anyhow!("{context} overflow"))
    })
}

/// Rollup operator configuration
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorConfig {
    /// Operator address
    pub operator_address: Address,

    /// Batch construction interval in seconds
    pub batch_interval: u64,

    /// Submission interval in seconds
    pub submission_interval: u64,

    /// Maximum transactions per batch
    pub max_transactions_per_batch: usize,

    /// L1 gas limit for batch submission
    pub l1_gas_limit: u64,

    /// L1 gas price threshold in gwei (don't submit if higher)
    pub l1_gas_price_threshold: u64,

    /// Whether to auto-submit batches
    pub auto_submit: bool,

    /// Whether to run the prover
    pub run_prover: bool,

    /// Number of concurrent prove tasks
    pub concurrent_prove_tasks: usize,
}

impl Default for OperatorConfig {
    fn default() -> Self {
        Self {
            operator_address: Address([0; 20]),
            batch_interval: 60,
            submission_interval: 600,
            max_transactions_per_batch: 100,
            l1_gas_limit: 1_000_000,
            l1_gas_price_threshold: 100,
            auto_submit: true,
            run_prover: true,
            concurrent_prove_tasks: 2,
        }
    }
}

/// Rollup operator metrics
#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperatorMetrics {
    /// Number of batches created
    pub batches_created: u64,

    /// Number of batches submitted
    pub batches_submitted: u64,

    /// Number of batches finalized
    pub batches_finalized: u64,

    /// Number of transactions processed
    pub transactions_processed: u64,

    /// Total fees collected
    pub total_fees_collected: u64,

    /// Average batch size in transactions
    pub avg_batch_size: f64,

    /// Average batch submission gas used
    pub avg_submission_gas: f64,

    /// Average batch submission cost in native tokens
    pub avg_submission_cost: f64,

    /// Last batch timestamp (unix seconds)
    pub last_batch_timestamp: Option<u64>,

    /// Last submission timestamp (unix seconds)
    pub last_submission_timestamp: Option<u64>,

    /// Operator uptime in seconds
    pub uptime: u64,
}

/// Rollup operator status
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorStatus {
    /// Operator is initializing
    Initializing,

    /// Operator is active
    Active,

    /// Operator is paused
    Paused,

    /// Operator is in error state
    Error,

    /// Operator is shutting down
    ShuttingDown,
}

/// Rollup operator for managing Layer 2 operations
#[allow(dead_code)]
pub struct RollupOperator {
    /// Operator configuration
    config: OperatorConfig,

    /// Operator status
    status: Arc<RwLock<OperatorStatus>>,

    /// Operator metrics
    metrics: Arc<RwLock<OperatorMetrics>>,

    /// Rollup instance
    rollup: Arc<Rollup>,

    /// Semaphore for controlling concurrent tasks
    prove_semaphore: Arc<Semaphore>,

    /// Start time
    start_time: std::time::Instant,

    /// Channel for stopping the operator
    stop_tx: Option<mpsc::Sender<()>>,
}

#[allow(dead_code)]
impl RollupOperator {
    /// Create a new rollup operator
    pub fn new(config: OperatorConfig, rollup: Arc<Rollup>) -> Self {
        let concurrent = config.concurrent_prove_tasks;
        Self {
            config,
            status: Arc::new(RwLock::new(OperatorStatus::Initializing)),
            metrics: Arc::new(RwLock::new(OperatorMetrics::default())),
            rollup,
            prove_semaphore: Arc::new(Semaphore::new(concurrent)),
            start_time: std::time::Instant::now(),
            stop_tx: None,
        }
    }

    /// Start the operator
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting rollup operator");

        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        self.stop_tx = Some(stop_tx);

        *self.status.write().await = OperatorStatus::Active;

        let status = self.status.clone();
        let metrics = self.metrics.clone();
        let rollup = self.rollup.clone();
        let config = self.config.clone();
        let start_time = self.start_time;

        let batch_status = status.clone();
        let batch_metrics = metrics.clone();
        let batch_rollup = rollup.clone();
        let batch_config = config.clone();

        tokio::spawn(async move {
            let mut batch_interval_timer =
                interval(Duration::from_secs(batch_config.batch_interval));

            loop {
                tokio::select! {
                    _ = batch_interval_timer.tick() => {
                        if *batch_status.read().await != OperatorStatus::Active {
                            debug!("Operator not active, skipping batch construction");
                            continue;
                        }

                        if batch_rollup.get_current_batch().await.is_some() {
                            debug!("Current batch exists, finalizing it");

                            if let Err(e) = batch_rollup.finalize_batch().await {
                                warn!("Failed to finalize batch: {}", e);
                            } else {
                                let mut m = batch_metrics.write().await;
                                m.batches_created += 1;
                                m.last_batch_timestamp = Some(
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs()
                                );
                            }
                        }

                        debug!("Starting new batch");
                        if let Err(e) = batch_rollup.start_new_batch().await {
                            warn!("Failed to start new batch: {}", e);
                        }
                    },
                    else => break,
                }
            }

            info!("Batch construction task stopped");
        });

        let submit_status = status.clone();
        let submit_metrics = metrics.clone();
        let submit_rollup = rollup.clone();
        let submit_config = config.clone();

        tokio::spawn(async move {
            if !submit_config.auto_submit {
                info!("Auto-submission disabled, submission task not started");
                return;
            }

            let mut submission_interval_timer =
                interval(Duration::from_secs(submit_config.submission_interval));

            loop {
                tokio::select! {
                    _ = submission_interval_timer.tick() => {
                        if *submit_status.read().await != OperatorStatus::Active {
                            debug!("Operator not active, skipping batch submission");
                            continue;
                        }

                        let batches = submit_rollup.get_batches().await;
                        let ready_batch_ids: Vec<u64> = batches
                            .iter()
                            .filter(|b| b.status == BatchStatus::ReadyForSubmission)
                            .map(|b| b.id)
                            .collect();

                        if ready_batch_ids.is_empty() {
                            debug!("No batches ready for submission");
                            continue;
                        }

                        info!("Found {} batches ready for submission", ready_batch_ids.len());

                        for batch_id in ready_batch_ids {
                            info!("Submitting batch {}", batch_id);

                            if let Err(e) = submit_rollup.submit_batch(batch_id).await {
                                warn!("Failed to submit batch {}: {}", batch_id, e);
                            } else {
                                let mut m = submit_metrics.write().await;
                                m.batches_submitted += 1;
                                m.last_submission_timestamp = Some(
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs()
                                );
                            }
                        }
                    },
                    else => break,
                }
            }

            info!("Batch submission task stopped");
        });

        let metrics_metrics = metrics.clone();
        let metrics_rollup = rollup.clone();

        tokio::spawn(async move {
            let mut metrics_interval_timer = interval(Duration::from_secs(60));

            loop {
                tokio::select! {
                    _ = metrics_interval_timer.tick() => {
                        let mut m = metrics_metrics.write().await;
                        m.uptime = start_time.elapsed().as_secs();

                        let batches = metrics_rollup.get_batches().await;
                        let tx_count = metrics_rollup.get_transaction_count().await;

                        let finalized_count = batches
                            .iter()
                            .filter(|b| b.status == BatchStatus::Finalized)
                            .count() as u64;

                        m.batches_finalized = finalized_count;
                        m.transactions_processed = tx_count as u64;

                        if !batches.is_empty() {
                            m.avg_batch_size = tx_count as f64 / batches.len() as f64;
                        }

                        match checked_sum_u64(
                            batches.iter().map(|b| b.fees_collected),
                            "operator batch fee total",
                        ) {
                            Ok(total_fees) => m.total_fees_collected = total_fees,
                            Err(err) => warn!("Failed to aggregate batch fees safely: {}", err),
                        }
                    },
                    else => break,
                }
            }

            info!("Metrics update task stopped");
        });

        tokio::spawn(async move {
            if let Some(_) = stop_rx.recv().await {
                info!("Received stop signal, shutting down operator");
                *status.write().await = OperatorStatus::ShuttingDown;
                tokio::time::sleep(Duration::from_secs(1)).await;
                *status.write().await = OperatorStatus::Paused;
                info!("Operator shutdown complete");
            }
        });

        info!("Rollup operator started successfully");

        Ok(())
    }

    /// Stop the operator
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping rollup operator");

        if let Some(tx) = &self.stop_tx {
            tx.send(())
                .await
                .map_err(|_| anyhow::anyhow!("Failed to send stop signal"))?;

            let mut wait_count = 0;
            while wait_count < 10 {
                if *self.status.read().await != OperatorStatus::Active {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
                wait_count += 1;
            }
        } else {
            return Err(anyhow::anyhow!("Operator not started"));
        }

        Ok(())
    }

    /// Get operator status
    pub async fn get_status(&self) -> OperatorStatus {
        *self.status.read().await
    }

    /// Get operator metrics
    pub async fn get_metrics(&self) -> OperatorMetrics {
        self.metrics.read().await.clone()
    }

    /// Submit a transaction to the rollup
    pub async fn submit_transaction(&self, transaction: Transaction) -> Result<()> {
        if *self.status.read().await != OperatorStatus::Active {
            return Err(anyhow::anyhow!("Operator is not active"));
        }

        self.rollup.add_transaction(transaction).await?;

        Ok(())
    }

    /// Get the current rollup state
    pub async fn get_rollup_state(&self) -> Result<Layer2State> {
        Ok(self.rollup.get_state().await)
    }
}

#[cfg(test)]
mod tests {
    use super::checked_sum_u64;

    #[test]
    fn checked_sum_u64_rejects_overflowing_fee_totals() {
        let err = checked_sum_u64([u64::MAX, 1], "operator batch fee total")
            .expect_err("overflowing fee totals must be rejected cleanly");

        assert!(
            err.to_string()
                .contains("operator batch fee total overflow"),
            "expected operator batch fee total overflow error, got {err:?}"
        );
    }
}
