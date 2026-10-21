//! Proof-of-Stake (PoS) consensus algorithm implementation.

use async_trait::async_trait;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::{
    blockchain::{block::Block, chain::Blockchain, transaction::Transaction},
    crypto::hash::{Hash, hash_bytes},
    monitoring::CORE_METRICS,
};

use crate::equivocation::{EquivocationEvidence, EvidenceStore, InMemoryEvidenceStore};
use crate::selection::bft_finality::{BftFinalityGadget, FinalityEvent, FinalityVote};
use crate::traits::ConsensusEngine;
use crate::types::{ConsensusConfig, ConsensusError, ConsensusResult, ValidatorInfo};

type SlashingMap = FxHashMap<(String, u64), Hash>;

/// Proof-of-Stake consensus algorithm.
#[derive(Debug)]
pub struct ProofOfStake {
    min_stake: u64,
    target_block_time: u64,
    validators: Arc<RwLock<FxHashMap<String, ValidatorInfo>>>,
    /// Fast path: maps (validator, height) to the first-seen block hash,
    /// used to detect equivocation on the next observation. This map is
    /// in-memory only — the durable record lives in [`evidence_store`].
    slashing_records: Arc<RwLock<SlashingMap>>,
    /// Durable store for equivocation proofs (#1218).
    ///
    /// Defaults to an [`InMemoryEvidenceStore`], which preserves
    /// pre-#1218 behavior (evidence lost on restart). Production nodes
    /// should inject a disk-backed store via [`Self::with_evidence_store`].
    evidence_store: Arc<dyn EvidenceStore>,
    config: ConsensusConfig,
    current_epoch: u64,
    blocks_per_epoch: u64,
    /// BFT finality gadget bound to this engine for hot-path vote
    /// submission (#1388). When attached via [`Self::attach_bft_finality`],
    /// every successful `validate_block` for the local validator emits a
    /// signed vote into the gadget. The gadget's existing finality
    /// channel sink (set up in `launch_api/main.rs`) then advances
    /// `Blockchain::set_finalized_height` and arms the reorg guard.
    /// `None` keeps the engine in pre-#1388 behavior.
    bft_finality: Option<Arc<RwLock<BftFinalityGadget>>>,
    /// Local validator identity used to mint signed votes. Set
    /// alongside [`Self::bft_finality`] via
    /// [`Self::attach_bft_finality`]. Only blocks whose `validator`
    /// metadata matches this id produce a self-vote — the engine never
    /// signs on behalf of remote validators.
    local_validator_id: Option<String>,
    /// Ed25519 signing key the local validator uses for finality votes.
    /// Held as `Arc` so the engine remains `Sync` despite the key
    /// material being unique per node.
    local_validator_signing_key: Option<Arc<ed25519_dalek::SigningKey>>,
}

impl ProofOfStake {
    pub fn new(min_stake: u64, target_block_time: u64) -> Self {
        let config = ConsensusConfig {
            min_stake: Some(min_stake),
            target_block_time,
            ..ConsensusConfig::default()
        };
        Self {
            min_stake,
            target_block_time,
            validators: Arc::new(RwLock::new(FxHashMap::default())),
            slashing_records: Arc::new(RwLock::new(FxHashMap::default())),
            evidence_store: Arc::new(InMemoryEvidenceStore::new()),
            config,
            current_epoch: 0,
            blocks_per_epoch: 100,
            bft_finality: None,
            local_validator_id: None,
            local_validator_signing_key: None,
        }
    }

    /// Attach a BFT finality gadget for hot-path self-voting (#1388).
    ///
    /// After this call, every `validate_block` accepted for `local_id`
    /// produces a signed [`FinalityVote`] submitted via
    /// [`BftFinalityGadget::submit_vote_verified`]. Vote and certificate
    /// metric counters (`wrfcoin_chain_bft_finality_votes_received_total`
    /// and `..._certificates_emitted_total`) are bumped inline.
    ///
    /// This wires the engine seam expected by `launch_api/main.rs` so the
    /// pre-existing `finality_signal_tx` channel (and its consumer
    /// `run_finality_event_loop`) finally has an active source. Without
    /// this attachment, the engine is unchanged from its pre-#1388
    /// behavior — no panics, no extra latency.
    pub fn attach_bft_finality(
        &mut self,
        gadget: Arc<RwLock<BftFinalityGadget>>,
        local_id: String,
        signing_key: Arc<ed25519_dalek::SigningKey>,
    ) {
        self.bft_finality = Some(gadget);
        self.local_validator_id = Some(local_id);
        self.local_validator_signing_key = Some(signing_key);
    }

    /// Mint and submit a self-vote for the just-validated block, if
    /// (1) a BFT gadget is attached, (2) `validator_addr` matches our
    /// local validator id. Bumps metric counters and is fail-soft —
    /// any error from the gadget is logged at WARN and swallowed so the
    /// consensus hot path is unaffected.
    async fn maybe_submit_self_finality_vote(&self, block: &Block, validator_addr: &str) {
        use ed25519_dalek::Signer;
        use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

        let Some(ref gadget) = self.bft_finality else {
            return;
        };
        let Some(ref local_id) = self.local_validator_id else {
            return;
        };
        let Some(ref sk) = self.local_validator_signing_key else {
            return;
        };
        if validator_addr != local_id {
            return; // never sign on behalf of a remote validator
        }
        let Some(block_hash) = block.hash else {
            return; // unhashed block — caller invariant violation, but skip safely
        };

        // #2256: bind the vote to the committee this gadget serves, so a vote
        // signed under one validator set cannot be counted under another.
        let (epoch, validator_set_hash) = {
            let g = gadget.read().await;
            (g.epoch(), g.validator_set_hash())
        };

        let signing_msg = BftFinalityGadget::signing_message_for(
            *block_hash.as_bytes(),
            block.height,
            epoch,
            validator_set_hash,
            local_id,
        );
        let signature = sk.sign(&signing_msg);
        let vote = FinalityVote {
            block_hash: *block_hash.as_bytes(),
            block_height: block.height,
            epoch,
            validator_set_hash,
            validator_id: local_id.clone(),
            signature: signature.to_bytes().to_vec(),
        };

        let mut g = gadget.write().await;
        match g.submit_vote_verified(vote) {
            Ok(event) => {
                CORE_METRICS.bft_finality_votes_received_total.inc();
                if matches!(event, FinalityEvent::BlockFinalized { .. }) {
                    CORE_METRICS.bft_finality_certificates_emitted_total.inc();
                }
            },
            Err(e) => {
                log::warn!(
                    "BFT self-vote rejected at height={}: {e} \
                     (engine continues, finality is fail-soft)",
                    block.height
                );
            },
        }
    }

    /// Attach a persistent equivocation evidence store (#1218).
    ///
    /// Production nodes should call this during consensus setup to wire
    /// in a disk-backed store (typically from the native-chain storage
    /// layer) so that double-signing proofs survive restarts.
    pub fn with_evidence_store(mut self, store: Arc<dyn EvidenceStore>) -> Self {
        self.evidence_store = store;
        self
    }

    /// Accessor for the evidence store, primarily for tests and
    /// operator tooling that wants to enumerate persisted proofs.
    pub fn evidence_store(&self) -> Arc<dyn EvidenceStore> {
        Arc::clone(&self.evidence_store)
    }

    pub fn from_config(config: &ConsensusConfig) -> Self {
        let mut engine = Self::new(config.min_stake.unwrap_or(1000), config.target_block_time);
        engine.config = config.clone();
        engine.blocks_per_epoch = config
            .algorithm_params
            .get("blocks_per_epoch")
            .and_then(|value| value.as_u64())
            .unwrap_or(100);
        engine
    }

    pub async fn add_validator(&self, address: String, stake: u64) -> ConsensusResult<()> {
        if stake < self.min_stake {
            return Err(ConsensusError::InsufficientResources(format!(
                "Stake {stake} is below minimum {}",
                self.min_stake
            )));
        }

        let validator = ValidatorInfo {
            address: address.clone(),
            stake,
            reputation: 1.0,
            blocks_validated: 0,
            last_activity: current_unix_timestamp(),
            is_active: true,
        };

        self.validators.write().await.insert(address, validator);
        Ok(())
    }

    pub async fn remove_validator(&self, address: &str) -> ConsensusResult<()> {
        self.validators.write().await.remove(address);
        Ok(())
    }

    /// Snapshot the active validator set as `(address, stake)` tuples,
    /// sorted by address for cross-node determinism (#1388).
    ///
    /// This is the seam used by both the BFT finality gadget (PR #1424)
    /// and the on-chain slashing application path (PR #1425). The
    /// projection is taken under a single short-lived read lock; the
    /// returned `Vec` owns its data so the lock drops before any caller
    /// awaits on it.
    ///
    /// Returns only validators with `is_active == true` and
    /// `stake >= self.min_stake` — the same predicate `select_validator`
    /// uses, so the BFT validator set matches the block-producer set.
    pub async fn active_finality_validators(&self) -> Vec<(String, u64)> {
        let validators = self.validators.read().await;
        let mut out: Vec<(String, u64)> = validators
            .values()
            .filter(|v| v.is_active && v.stake >= self.min_stake)
            .map(|v| (v.address.clone(), v.stake))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Variant of [`Self::active_finality_validators`] that cross-checks
    /// each in-memory validator against authoritative on-chain
    /// `Account.stake_locked` via a caller-provided lookup (#1420).
    ///
    /// `StateTransition::Slash` debits `Account.stake_locked` immediately
    /// on apply, but the in-memory `ValidatorInfo::stake` mirror is only
    /// reconciled at epoch boundaries through
    /// [`Self::evict_sub_min_stake_validators`]. Between a slash and the
    /// next epoch, this method ensures a freshly-slashed validator whose
    /// on-chain stake fell below `min_stake` is not returned as eligible
    /// for finality voting or block selection.
    ///
    /// `on_chain_stake` returns the validator's `Account.stake_locked` in
    /// base units, or `None` if no `Account` exists (validator never
    /// staked through the protocol path). When `None`, the in-memory
    /// `stake` is used as the fallback so genesis / test-only validators
    /// continue to work without state plumbing.
    ///
    /// The returned tuple's stake field is the **on-chain** value when
    /// available — that is the authoritative number for downstream BFT
    /// weighting and audit logging.
    pub async fn active_finality_validators_with_on_chain_stake<F>(
        &self,
        on_chain_stake: F,
    ) -> Vec<(String, u64)>
    where
        F: Fn(&str) -> Option<u64>,
    {
        let validators = self.validators.read().await;
        let mut out: Vec<(String, u64)> = validators
            .values()
            .filter_map(|v| {
                if !v.is_active {
                    return None;
                }
                let effective_stake = on_chain_stake(&v.address).unwrap_or(v.stake);
                if effective_stake < self.min_stake {
                    return None;
                }
                Some((v.address.clone(), effective_stake))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Debit a validator's stake by `amount` base units (#1393).
    ///
    /// Called from `blockchain::state::State::apply_protocol_transaction`
    /// when processing `TransactionType::SlashingReport` through
    /// `StateTransition::Slash`. Uses a `u128` intermediate to avoid
    /// overflow on large penalties, then saturates down to the `u64`
    /// mirror stored in `ValidatorInfo::stake`.
    ///
    /// Returns the amount actually debited (in base units), which may be
    /// less than `amount` if the validator's remaining stake is smaller.
    /// Returns `0` if the validator is unknown — callers should decide
    /// whether an unknown target is a hard error at the state-machine
    /// layer (commit 5).
    pub async fn debit_validator_stake(&self, address: &str, amount: u128) -> u128 {
        let mut validators = self.validators.write().await;
        let Some(validator) = validators.get_mut(address) else {
            return 0;
        };
        let current = validator.stake as u128;
        let debit = amount.min(current);
        let remaining = current.saturating_sub(debit);
        validator.stake = u64::try_from(remaining).unwrap_or(u64::MAX);
        debit
    }

    fn selection_seed(previous_hash: &str) -> u64 {
        let hash = hash_bytes(previous_hash.as_bytes());
        let bytes = hash.as_bytes();
        u64::from_be_bytes(bytes[..8].try_into().unwrap_or([0; 8]))
    }

    #[allow(dead_code)]
    async fn select_validator(&self, seed: u64) -> ConsensusResult<String> {
        let validators = self.validators.read().await;
        let mut active = validators
            .values()
            .filter(|validator| validator.is_active && validator.stake >= self.min_stake)
            .cloned()
            .collect::<Vec<_>>();

        active.sort_by(|a, b| a.address.cmp(&b.address));
        if active.is_empty() {
            return Err(ConsensusError::InvalidState(
                "No validators available".to_string(),
            ));
        }

        let total_stake: u64 = active.iter().map(|validator| validator.stake).sum();
        if total_stake == 0 {
            return Err(ConsensusError::InvalidState(
                "No active stake available".to_string(),
            ));
        }

        let mut cursor = seed % total_stake;
        for validator in active {
            if cursor < validator.stake {
                return Ok(validator.address);
            }
            cursor -= validator.stake;
        }

        Err(ConsensusError::InvalidState(
            "Failed to select validator".to_string(),
        ))
    }

    async fn record_double_sign(
        &self,
        validator_address: &str,
        block: &Block,
    ) -> ConsensusResult<()> {
        let Some(hash) = block.hash else {
            return Err(ConsensusError::ValidationFailed(
                "PoS block missing hash".to_string(),
            ));
        };

        let key = (validator_address.to_string(), block.height);

        // First-seen hash for this (validator, height) pair, if any.
        // Extract under a short read lock so we can release it before
        // the longer write path.
        let first_seen = {
            let records = self.slashing_records.read().await;
            records.get(&key).copied()
        };

        match first_seen {
            Some(existing) if existing != hash => {
                // Equivocation detected — persist durable evidence (#1218)
                // before returning the error so that the proof survives
                // restarts and can be audited externally.
                let evidence = EquivocationEvidence {
                    validator: validator_address.to_string(),
                    height: block.height,
                    first_block_hash: existing,
                    second_block_hash: hash,
                    detected_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                };
                if let Err(e) = self.evidence_store.persist(evidence).await {
                    // Persistence failure is logged but does not swallow
                    // the malicious-validator error — detection still fires.
                    log::error!(
                        "Failed to persist equivocation evidence for {validator_address} at height {}: {e}",
                        block.height
                    );
                }

                // #1393: bump the detection counter and log loudly. Evidence
                // is durable (#1218) but there is no code path yet that
                // turns it into an on-chain slashing transaction, so the
                // only operator-visible signal of malicious behavior is
                // this metric + log line. The companion alert rule should
                // page on `rate(wrfcoin_chain_malicious_validator_detected_total[5m]) > 0`
                // so oncall sees the gap between detection and enforcement.
                CORE_METRICS.inc_malicious_validator_detected();
                log::warn!(
                    "EQUIVOCATION DETECTED: validator={validator_address} height={} \
                     first_hash={} second_hash={} — evidence persisted, but NO \
                     on-chain slashing transaction is produced yet (#1393). \
                     Stake is NOT burned. Monitor \
                     wrfcoin_chain_malicious_validator_detected_total.",
                    block.height,
                    hex::encode(existing),
                    hex::encode(hash)
                );

                Err(ConsensusError::MaliciousValidator(format!(
                    "Validator {validator_address} double-signed height {}",
                    block.height
                )))
            },
            Some(_existing) => Ok(()), // same hash, duplicate observation
            None => {
                // First observation — record the hash under write lock.
                let mut records = self.slashing_records.write().await;
                records.insert(key, hash);
                Ok(())
            },
        }
    }

    async fn update_validator_stats(&self, validator_address: &str) -> ConsensusResult<()> {
        let mut validators = self.validators.write().await;
        if let Some(validator) = validators.get_mut(validator_address) {
            validator.blocks_validated += 1;
            validator.last_activity = current_unix_timestamp();
        }
        Ok(())
    }

    async fn is_valid_validator(&self, address: &str) -> bool {
        self.validators
            .read()
            .await
            .get(address)
            .map(|validator| validator.is_active && validator.stake >= self.min_stake)
            .unwrap_or(false)
    }

    async fn epoch_boundary_metadata_valid(&self, block: &Block) -> bool {
        if self.blocks_per_epoch == 0 || block.height % self.blocks_per_epoch != 0 {
            return true;
        }

        // Evict any validator whose stake has dropped below `min_stake` after
        // intra-epoch slashing (#1393). This runs at epoch boundaries so the
        // next epoch's validator set is consistent with post-slash stake.
        self.evict_sub_min_stake_validators().await;

        block.metadata.contains_key("epoch_signatures")
    }

    /// Evict validators whose stake has fallen below `self.min_stake`.
    ///
    /// Called from `epoch_boundary_metadata_valid` at every epoch boundary.
    /// Returns the list of evicted addresses for metrics / audit logging.
    /// Idempotent: a validator already below the threshold but still in the
    /// map is removed on the next call if not already cleared.
    pub async fn evict_sub_min_stake_validators(&self) -> Vec<String> {
        let mut validators = self.validators.write().await;
        let evicted: Vec<String> = validators
            .iter()
            .filter(|(_, info)| info.stake < self.min_stake)
            .map(|(addr, _)| addr.clone())
            .collect();
        for addr in &evicted {
            validators.remove(addr);
        }
        evicted
    }
}

#[async_trait]
impl ConsensusEngine for ProofOfStake {
    async fn validate_block(&self, block: &Block, _chain: &Blockchain) -> ConsensusResult<bool> {
        if let Some(ct) = block.metadata.get("consensus_type") {
            if ct != "ProofOfStake" {
                return Ok(false);
            }
        }

        let validator_addr = block
            .metadata
            .get("validator")
            .map(String::as_str)
            .unwrap_or("");
        if !self.is_valid_validator(validator_addr).await {
            return Ok(false);
        }

        if !self.epoch_boundary_metadata_valid(block).await {
            return Ok(false);
        }

        self.record_double_sign(validator_addr, block).await?;

        // #1388: hot-path emission. After equivocation check passes, mint
        // and submit a signed finality vote if a BFT gadget is attached
        // and this block was produced by the local validator. The
        // gadget's existing finality_signal sink takes care of the rest.
        self.maybe_submit_self_finality_vote(block, validator_addr)
            .await;

        Ok(true)
    }

    async fn create_block(
        &self,
        transactions: Vec<Transaction>,
        previous_hash: &str,
        miner_address: &str,
        chain: &Blockchain,
    ) -> ConsensusResult<Block> {
        if !self.is_valid_validator(miner_address).await {
            return Err(ConsensusError::CreationFailed(format!(
                "Address {miner_address} is not a valid validator"
            )));
        }

        let prev = previous_hash
            .parse()
            .unwrap_or_else(|_| hash_bytes(previous_hash.as_bytes()));
        let mut block = Block::new(3, prev, transactions, 1, chain.height + 1);
        block
            .metadata
            .insert("validator".into(), miner_address.into());
        block
            .metadata
            .insert("consensus_type".into(), "ProofOfStake".into());
        block.metadata.insert(
            "selection_seed".into(),
            Self::selection_seed(previous_hash).to_string(),
        );
        let selected_validator = self
            .select_validator(Self::selection_seed(previous_hash))
            .await?;
        block
            .metadata
            .insert("selected_validator".into(), selected_validator);

        if self.blocks_per_epoch > 0 && block.height % self.blocks_per_epoch == 0 {
            let epoch = block.height / self.blocks_per_epoch;
            block
                .metadata
                .insert("epoch_boundary".into(), epoch.to_string());
            block
                .metadata
                .insert("epoch_signatures".into(), miner_address.to_string());
        }

        let hash = block.calculate_hash();
        block.set_hash(hash);

        self.record_double_sign(miner_address, &block).await?;
        self.update_validator_stats(miner_address).await?;
        Ok(block)
    }

    async fn get_difficulty(&self) -> u64 {
        1
    }

    fn algorithm_name(&self) -> &'static str {
        "ProofOfStake"
    }

    async fn configure(&mut self, config: &ConsensusConfig) -> ConsensusResult<()> {
        self.config = config.clone();
        self.target_block_time = config.target_block_time;
        self.min_stake = config.min_stake.unwrap_or(self.min_stake);
        self.blocks_per_epoch = config
            .algorithm_params
            .get("blocks_per_epoch")
            .and_then(|value| value.as_u64())
            .unwrap_or(self.blocks_per_epoch);
        Ok(())
    }

    fn get_metrics(&self) -> FxHashMap<String, serde_json::Value> {
        let mut metrics = FxHashMap::default();
        metrics.insert("algorithm".to_string(), "ProofOfStake".into());
        metrics.insert("min_stake".to_string(), self.min_stake.into());
        metrics.insert(
            "target_block_time".to_string(),
            self.target_block_time.into(),
        );
        metrics.insert("current_epoch".to_string(), self.current_epoch.into());
        metrics.insert("blocks_per_epoch".to_string(), self.blocks_per_epoch.into());
        metrics
    }
}

fn current_unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    // CORE_METRICS is process-global. These tests deliberately exercise the
    // production equivocation path, which increments the shared malicious
    // validator counter. Serialize only the tests that can increment it so
    // the exact-once delta assertion cannot observe another test's event.
    static POS_METRIC_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn pos_selects_validator_by_stake() {
        let engine = ProofOfStake::new(10, 5);
        engine.add_validator("small".into(), 10).await.unwrap();
        engine.add_validator("large".into(), 90).await.unwrap();

        let mut small = 0;
        let mut large = 0;
        for seed in 0..1_000 {
            match engine.select_validator(seed).await.unwrap().as_str() {
                "small" => small += 1,
                "large" => large += 1,
                other => panic!("unexpected validator {other}"),
            }
        }

        assert!(large > small);
    }

    #[tokio::test]
    async fn pos_rejects_unstaked_validator() {
        let engine = ProofOfStake::new(100, 5);
        assert!(engine.add_validator("tiny".into(), 50).await.is_err());
    }

    #[tokio::test]
    async fn pos_detects_double_sign() {
        let _metric_guard = POS_METRIC_TEST_LOCK.lock().unwrap();
        let engine = ProofOfStake::new(10, 5);
        engine
            .add_validator("validator-1".into(), 100)
            .await
            .unwrap();
        let chain = Blockchain::default();

        let block_a = engine
            .create_block(
                Vec::new(),
                &Hash::default().to_string(),
                "validator-1",
                &chain,
            )
            .await
            .unwrap();
        let mut block_b = Block::new(3, hash_bytes(b"other-parent"), Vec::new(), 1, 1);
        block_b
            .metadata
            .insert("validator".into(), "validator-1".into());
        block_b
            .metadata
            .insert("consensus_type".into(), "ProofOfStake".into());
        let hash = block_b.calculate_hash();
        block_b.set_hash(hash);

        assert!(engine.validate_block(&block_a, &chain).await.unwrap());
        let err = engine.validate_block(&block_b, &chain).await.unwrap_err();
        assert!(matches!(err, ConsensusError::MaliciousValidator(_)));
    }

    // ── Detection metric (#1393) ─────────────────────────────────────
    //
    // CORE_METRICS is a process-wide LazyLock, so we assert a strict
    // delta rather than absolute values (earlier tests in the same
    // process may have bumped the counter). The delta proves our
    // detection site is wired to the counter.

    #[tokio::test]
    async fn pos_detection_increments_malicious_validator_counter() {
        let _metric_guard = POS_METRIC_TEST_LOCK.lock().unwrap();
        let engine = ProofOfStake::new(10, 5);
        engine
            .add_validator("validator-1".into(), 100)
            .await
            .unwrap();
        let chain = Blockchain::default();

        let block_a = engine
            .create_block(
                Vec::new(),
                &Hash::default().to_string(),
                "validator-1",
                &chain,
            )
            .await
            .unwrap();
        let mut block_b = Block::new(3, hash_bytes(b"other-parent"), Vec::new(), 1, 1);
        block_b
            .metadata
            .insert("validator".into(), "validator-1".into());
        block_b
            .metadata
            .insert("consensus_type".into(), "ProofOfStake".into());
        let hash_b = block_b.calculate_hash();
        block_b.set_hash(hash_b);

        // Baseline before equivocation.
        let before = CORE_METRICS.malicious_validator_detected_total.get();

        // First observation — no detection yet.
        assert!(engine.validate_block(&block_a, &chain).await.unwrap());
        assert_eq!(
            CORE_METRICS.malicious_validator_detected_total.get(),
            before,
            "first observation must NOT bump the detection counter"
        );

        // Second observation at same height — equivocation detected.
        let err = engine.validate_block(&block_b, &chain).await.unwrap_err();
        assert!(matches!(err, ConsensusError::MaliciousValidator(_)));

        assert_eq!(
            CORE_METRICS.malicious_validator_detected_total.get(),
            before + 1,
            "equivocation detection must increment \
             wrfcoin_chain_malicious_validator_detected_total exactly once (#1393)"
        );
    }

    // ── Equivocation evidence persistence (#1218) ─────────────────────

    #[tokio::test]
    async fn pos_persists_double_sign_evidence() {
        let _metric_guard = POS_METRIC_TEST_LOCK.lock().unwrap();
        let store = Arc::new(InMemoryEvidenceStore::new());
        let engine = ProofOfStake::new(10, 5).with_evidence_store(store.clone());
        engine
            .add_validator("validator-1".into(), 100)
            .await
            .unwrap();
        let chain = Blockchain::default();

        let block_a = engine
            .create_block(
                Vec::new(),
                &Hash::default().to_string(),
                "validator-1",
                &chain,
            )
            .await
            .unwrap();
        let mut block_b = Block::new(3, hash_bytes(b"other-parent"), Vec::new(), 1, 1);
        block_b
            .metadata
            .insert("validator".into(), "validator-1".into());
        block_b
            .metadata
            .insert("consensus_type".into(), "ProofOfStake".into());
        let hash_b = block_b.calculate_hash();
        block_b.set_hash(hash_b);

        // Before equivocation: store is empty.
        assert_eq!(store.count().await.unwrap(), 0);

        // First block succeeds.
        assert!(engine.validate_block(&block_a, &chain).await.unwrap());
        // Store still empty — no equivocation yet.
        assert_eq!(store.count().await.unwrap(), 0);

        // Second block at same height triggers equivocation.
        let err = engine.validate_block(&block_b, &chain).await.unwrap_err();
        assert!(matches!(err, ConsensusError::MaliciousValidator(_)));

        // Evidence persisted.
        let loaded = store.load_for_validator("validator-1").await.unwrap();
        assert_eq!(
            loaded.len(),
            1,
            "evidence must be persisted on equivocation"
        );
        let ev = &loaded[0];
        assert_eq!(ev.validator, "validator-1");
        assert_eq!(ev.height, block_a.height);
        // Both block hashes are captured.
        assert_eq!(ev.first_block_hash, block_a.hash.unwrap());
        assert_eq!(ev.second_block_hash, hash_b);
        assert_ne!(ev.first_block_hash, ev.second_block_hash);
    }

    #[tokio::test]
    async fn pos_duplicate_observation_does_not_persist_evidence() {
        let store = Arc::new(InMemoryEvidenceStore::new());
        let engine = ProofOfStake::new(10, 5).with_evidence_store(store.clone());
        engine
            .add_validator("validator-1".into(), 100)
            .await
            .unwrap();
        let chain = Blockchain::default();

        let block = engine
            .create_block(
                Vec::new(),
                &Hash::default().to_string(),
                "validator-1",
                &chain,
            )
            .await
            .unwrap();

        // Submit the SAME block twice — not an equivocation.
        assert!(engine.validate_block(&block, &chain).await.unwrap());
        assert!(engine.validate_block(&block, &chain).await.unwrap());

        assert_eq!(
            store.count().await.unwrap(),
            0,
            "idempotent re-submission must not generate evidence"
        );
    }

    #[tokio::test]
    async fn pos_evidence_store_accessor_returns_attached_store() {
        let store = Arc::new(InMemoryEvidenceStore::new());
        let engine = ProofOfStake::new(10, 5).with_evidence_store(store.clone());

        // The accessor should return the same underlying store so that
        // persisting via the engine is visible through the returned handle.
        let accessed = engine.evidence_store();
        let ev = EquivocationEvidence {
            validator: "v1".to_string(),
            height: 1,
            first_block_hash: Hash::from([1u8; 32]),
            second_block_hash: Hash::from([2u8; 32]),
            detected_at: 0,
        };
        accessed.persist(ev).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn pos_default_evidence_store_is_in_memory() {
        // Callers that don't opt in still get a working (non-persistent) store.
        let engine = ProofOfStake::new(10, 5);
        let store = engine.evidence_store();
        assert_eq!(store.count().await.unwrap(), 0);
    }

    // ── #1388: active_finality_validators accessor ────────────────────

    // ── #1388: hot-path BFT vote on validate_block ────────────────────

    #[tokio::test]
    async fn pos_validate_block_emits_finality_signal_for_local_validator() {
        use crate::selection::bft_finality::{BftFinalityGadget, FinalityValidator};
        use ed25519_dalek::SigningKey;
        use wrfcoin_shared_protocol::consensus::finality::finality_signal_channel;

        // Single-validator gadget — one self-vote crosses the 2/3+ threshold.
        let mut sk_bytes = [0u8; 32];
        sk_bytes[0] = 7;
        let sk = SigningKey::from_bytes(&sk_bytes);
        let pubkey = sk.verifying_key().to_bytes();

        let gadget = Arc::new(RwLock::new(BftFinalityGadget::with_finality_sink(
            vec![FinalityValidator {
                id: "validator-1".into(),
                stake_weight: 100,
                public_key: Some(pubkey),
            }],
            {
                let (tx, _rx) = finality_signal_channel(); // sink is set; capture rx outside
                tx
            },
        )));

        let (sink_tx, mut sink_rx) = finality_signal_channel();
        gadget.write().await.set_finality_sink(sink_tx);

        let mut engine = ProofOfStake::new(10, 5);
        engine.attach_bft_finality(gadget.clone(), "validator-1".into(), Arc::new(sk));
        engine
            .add_validator("validator-1".into(), 100)
            .await
            .unwrap();

        let chain = Blockchain::default();
        let block = engine
            .create_block(
                Vec::new(),
                &Hash::default().to_string(),
                "validator-1",
                &chain,
            )
            .await
            .unwrap();

        // create_block already calls record_double_sign for the local validator,
        // so we need a fresh engine to validate without tripping equivocation.
        // Build a fresh engine + gadget purely for the validate_block path.
        let gadget2 = Arc::new(RwLock::new(BftFinalityGadget::with_finality_sink(
            vec![FinalityValidator {
                id: "validator-1".into(),
                stake_weight: 100,
                public_key: Some(pubkey),
            }],
            {
                let (tx, _) = finality_signal_channel();
                tx
            },
        )));
        let (sink2_tx, mut sink2_rx) = finality_signal_channel();
        gadget2.write().await.set_finality_sink(sink2_tx);

        // Reuse the signing key for the fresh engine (same validator id).
        let mut sk2_bytes = [0u8; 32];
        sk2_bytes[0] = 7;
        let sk2 = SigningKey::from_bytes(&sk2_bytes);
        let mut engine2 = ProofOfStake::new(10, 5);
        engine2.attach_bft_finality(gadget2.clone(), "validator-1".into(), Arc::new(sk2));
        engine2
            .add_validator("validator-1".into(), 100)
            .await
            .unwrap();

        let _ = sink_rx.try_recv(); // drain anything from the unused sink

        // Validate the block via the fresh engine — this is the hot path that
        // should emit a finality signal once the self-vote crosses the
        // 2/3+ supermajority threshold.
        assert!(engine2.validate_block(&block, &chain).await.unwrap());

        let signal = sink2_rx
            .recv()
            .await
            .expect("validate_block must emit a FinalitySignal for the local validator");
        assert_eq!(signal.height, block.height);
        assert_eq!(signal.block_hash, *block.hash.unwrap().as_bytes());
        assert_eq!(signal.source, "bft-gadget");
    }

    #[tokio::test]
    async fn pos_validate_block_does_not_self_vote_for_remote_validator() {
        use crate::selection::bft_finality::{BftFinalityGadget, FinalityValidator};
        use ed25519_dalek::SigningKey;
        use wrfcoin_shared_protocol::consensus::finality::finality_signal_channel;

        let mut sk_bytes = [0u8; 32];
        sk_bytes[0] = 9;
        let sk = SigningKey::from_bytes(&sk_bytes);

        let gadget = Arc::new(RwLock::new(BftFinalityGadget::with_finality_sink(
            vec![FinalityValidator {
                id: "remote".into(),
                stake_weight: 100,
                public_key: Some(sk.verifying_key().to_bytes()),
            }],
            {
                let (tx, _) = finality_signal_channel();
                tx
            },
        )));
        let (sink_tx, mut sink_rx) = finality_signal_channel();
        gadget.write().await.set_finality_sink(sink_tx);

        let mut engine = ProofOfStake::new(10, 5);
        engine.attach_bft_finality(gadget, "local".into(), Arc::new(sk));
        engine.add_validator("remote".into(), 100).await.unwrap();

        let chain = Blockchain::default();
        let mut block = Block::new(3, hash_bytes(b"parent"), Vec::new(), 1, 1);
        block.metadata.insert("validator".into(), "remote".into());
        block
            .metadata
            .insert("consensus_type".into(), "ProofOfStake".into());
        let h = block.calculate_hash();
        block.set_hash(h);

        assert!(engine.validate_block(&block, &chain).await.unwrap());
        assert!(
            sink_rx.try_recv().is_err(),
            "engine must NOT mint a vote on behalf of a remote validator"
        );
    }

    #[tokio::test]
    async fn pos_active_finality_validators_returns_sorted_active_set() {
        let engine = ProofOfStake::new(10, 5);
        engine.add_validator("zeta".into(), 30).await.unwrap();
        engine.add_validator("alpha".into(), 50).await.unwrap();
        engine.add_validator("mike".into(), 20).await.unwrap();

        let active = engine.active_finality_validators().await;
        assert_eq!(
            active,
            vec![
                ("alpha".to_string(), 50),
                ("mike".to_string(), 20),
                ("zeta".to_string(), 30),
            ],
            "must be deterministically sorted by address for cross-node parity"
        );
    }

    #[tokio::test]
    async fn pos_active_finality_validators_excludes_below_min_stake() {
        let engine = ProofOfStake::new(100, 5);
        engine.add_validator("rich".into(), 500).await.unwrap();
        // add_validator already enforces min_stake on entry, so directly
        // mutate the underlying map to simulate a validator whose stake
        // has been debited below the threshold by slashing.
        engine.validators.write().await.insert(
            "poor".into(),
            ValidatorInfo {
                address: "poor".into(),
                stake: 50, // below min_stake=100
                reputation: 1.0,
                blocks_validated: 0,
                last_activity: 0,
                is_active: true,
            },
        );

        let active = engine.active_finality_validators().await;
        assert_eq!(active, vec![("rich".to_string(), 500)]);
    }

    #[tokio::test]
    async fn pos_active_finality_validators_excludes_inactive() {
        let engine = ProofOfStake::new(10, 5);
        engine.add_validator("active".into(), 100).await.unwrap();
        engine.add_validator("inactive".into(), 100).await.unwrap();
        // Flip "inactive" to is_active=false to simulate eviction.
        engine
            .validators
            .write()
            .await
            .get_mut("inactive")
            .unwrap()
            .is_active = false;

        let active = engine.active_finality_validators().await;
        assert_eq!(active, vec![("active".to_string(), 100)]);
    }

    // ── #1420: validator-set reconciliation against on-chain stake ────
    //
    // `StateTransition::Slash` debits `Account.stake_locked` immediately,
    // but the in-memory `ValidatorInfo::stake` mirror is only reconciled
    // at epoch boundaries through `evict_sub_min_stake_validators`.
    // `active_finality_validators_with_on_chain_stake` closes that
    // intra-epoch window so a slashed validator whose on-chain stake has
    // fallen below `min_stake` cannot be selected for finality voting or
    // block production while the in-memory mirror still says they are
    // eligible.

    #[tokio::test]
    async fn with_on_chain_stake_excludes_post_slash_below_min() {
        let engine = ProofOfStake::new(100, 5);
        engine.add_validator("slashed".into(), 500).await.unwrap();
        engine.add_validator("healthy".into(), 500).await.unwrap();

        // Pre-slash: in-memory stake 500 >= min_stake 100 — both eligible.
        let pre = engine.active_finality_validators().await;
        assert_eq!(
            pre,
            vec![("healthy".to_string(), 500), ("slashed".to_string(), 500),],
            "without on-chain check, both validators are eligible"
        );

        // Post-slash: state debited "slashed".stake_locked from 500 → 50,
        // but `ValidatorInfo::stake` mirror still reads 500. The on-chain
        // lookup must filter the slashed validator out of the eligible
        // set immediately, not at the next epoch boundary.
        let post = engine
            .active_finality_validators_with_on_chain_stake(|addr| match addr {
                "slashed" => Some(50),
                "healthy" => Some(500),
                _ => None,
            })
            .await;
        assert_eq!(
            post,
            vec![("healthy".to_string(), 500)],
            "freshly-slashed validator must not appear in the active set"
        );
    }

    #[tokio::test]
    async fn with_on_chain_stake_returns_authoritative_value() {
        let engine = ProofOfStake::new(100, 5);
        engine.add_validator("v1".into(), 500).await.unwrap();

        // On-chain stake 250 is above min but differs from in-memory 500.
        // The returned tuple must carry the on-chain (authoritative) value
        // so downstream BFT weight calculations match the apply-path math.
        let result = engine
            .active_finality_validators_with_on_chain_stake(|_| Some(250))
            .await;
        assert_eq!(
            result,
            vec![("v1".to_string(), 250)],
            "returned stake must be the on-chain value, not the in-memory mirror"
        );
    }

    #[tokio::test]
    async fn with_on_chain_stake_falls_back_to_in_memory_when_lookup_returns_none() {
        let engine = ProofOfStake::new(100, 5);
        engine.add_validator("genesis".into(), 200).await.unwrap();

        // `None` from the lookup means "no on-chain Account" — typical for
        // genesis or test-only validators that never went through a stake
        // tx. Must fall back to the in-memory stake so those validators
        // are not silently excluded.
        let result = engine
            .active_finality_validators_with_on_chain_stake(|_| None)
            .await;
        assert_eq!(
            result,
            vec![("genesis".to_string(), 200)],
            "validators with no on-chain account must fall back to in-memory stake"
        );
    }

    #[tokio::test]
    async fn with_on_chain_stake_still_excludes_inactive_validators() {
        let engine = ProofOfStake::new(100, 5);
        engine.add_validator("active".into(), 200).await.unwrap();
        engine.add_validator("inactive".into(), 200).await.unwrap();
        engine
            .validators
            .write()
            .await
            .get_mut("inactive")
            .unwrap()
            .is_active = false;

        // Even with a generous on-chain stake, an inactive validator
        // (e.g. one that has been jailed by a prior epoch reconciliation)
        // must remain excluded.
        let result = engine
            .active_finality_validators_with_on_chain_stake(|_| Some(1_000))
            .await;
        assert_eq!(result, vec![("active".to_string(), 1_000)]);
    }

    #[tokio::test]
    async fn pos_epoch_boundary_requires_signatures() {
        let mut config = ConsensusConfig::default();
        config.min_stake = Some(10);
        config
            .algorithm_params
            .insert("blocks_per_epoch".into(), serde_json::Value::from(2));
        let engine = ProofOfStake::from_config(&config);
        engine
            .add_validator("validator-1".into(), 100)
            .await
            .unwrap();

        let mut chain = Blockchain::default();
        let first = engine
            .create_block(
                Vec::new(),
                &Hash::default().to_string(),
                "validator-1",
                &chain,
            )
            .await
            .unwrap();
        chain.add_block(first).unwrap();

        let mut second = Block::new(3, chain.tip, Vec::new(), 1, 2);
        second
            .metadata
            .insert("validator".into(), "validator-1".into());
        second
            .metadata
            .insert("consensus_type".into(), "ProofOfStake".into());
        second.set_hash(second.calculate_hash());

        assert!(!engine.validate_block(&second, &chain).await.unwrap());
    }

    // ── #1393: slashing apply path primitives ──────────────────────────
    //
    // These tests pin the behavior of the two PoS-engine primitives used by
    // the on-chain slashing flow (commits 4 & 6 of the #1393 roadmap):
    //
    //   - `debit_validator_stake(addr, amount) -> actually_debited` — u128
    //     intermediate, saturating u64 mirror, and silent no-op for unknown
    //     validators so state.rs can treat "not a PoS validator" as a warning
    //     rather than a hard failure.
    //
    //   - `evict_sub_min_stake_validators() -> Vec<addr>` — removes any
    //     validator whose `stake < min_stake` so that a post-slash validator
    //     set is never observed producing blocks in the next epoch.

    #[tokio::test]
    async fn debit_validator_stake_debits_exact_amount() {
        let engine = ProofOfStake::new(10, 5);
        engine.add_validator("v1".into(), 100).await.unwrap();

        let debited = engine.debit_validator_stake("v1", 30).await;
        assert_eq!(debited, 30, "debit must equal requested amount when funded");

        let v = engine
            .validators
            .read()
            .await
            .get("v1")
            .cloned()
            .expect("validator still present after partial debit");
        assert_eq!(v.stake, 70);
    }

    #[tokio::test]
    async fn debit_validator_stake_saturates_at_current_balance() {
        // Requesting more than the validator holds debits only what is
        // there — no underflow, no wrap — and reports the actual debit.
        let engine = ProofOfStake::new(10, 5);
        engine.add_validator("v1".into(), 50).await.unwrap();

        let debited = engine.debit_validator_stake("v1", 1_000).await;
        assert_eq!(debited, 50, "debit must saturate at current stake");

        let v = engine
            .validators
            .read()
            .await
            .get("v1")
            .cloned()
            .expect("validator still present after full debit");
        assert_eq!(v.stake, 0);
    }

    #[tokio::test]
    async fn debit_validator_stake_unknown_validator_returns_zero() {
        // Unknown-validator slashing is not a hard error at the PoS layer;
        // state.rs (which owns the account model) is the source of truth.
        let engine = ProofOfStake::new(10, 5);
        let debited = engine.debit_validator_stake("does-not-exist", 100).await;
        assert_eq!(debited, 0);
    }

    #[tokio::test]
    async fn evict_sub_min_stake_removes_only_underfunded_validators() {
        // After a slash takes a validator below min_stake, the next epoch
        // boundary must drop it from the active set. Validators whose
        // stake remains >= min_stake stay.
        let engine = ProofOfStake::new(100, 4);
        engine.add_validator("healthy".into(), 200).await.unwrap();
        engine
            .add_validator("borderline".into(), 100)
            .await
            .unwrap();
        engine
            .add_validator("to-be-evicted".into(), 150)
            .await
            .unwrap();

        // Simulate an on-chain slash that drains one validator below min.
        let _ = engine.debit_validator_stake("to-be-evicted", 100).await;
        assert_eq!(
            engine
                .validators
                .read()
                .await
                .get("to-be-evicted")
                .unwrap()
                .stake,
            50
        );

        let evicted = engine.evict_sub_min_stake_validators().await;
        assert_eq!(evicted, vec!["to-be-evicted".to_string()]);

        let validators = engine.validators.read().await;
        assert!(validators.contains_key("healthy"));
        assert!(
            validators.contains_key("borderline"),
            "exactly-at-min stake must not be evicted"
        );
        assert!(!validators.contains_key("to-be-evicted"));
    }

    // ── #1735 hardening batch 8b: PoS difficulty-floor regression guards ───────
    //
    // PoS returns a constant difficulty of 1 from get_difficulty().  These
    // tests pin that invariant under adversarial engine states so it cannot
    // silently regress to 0 (the same bug class fixed in PoP by PR #1985).

    /// `get_difficulty` must always return 1 even with no validators registered.
    /// Zero-validator PoS must not accidentally yield 0 work.
    #[tokio::test]
    async fn pos_get_difficulty_zero_validators_returns_one() {
        let engine = ProofOfStake::new(100, 5);
        // No validators added — empty set.
        let d = engine.get_difficulty().await;
        assert_eq!(
            d, 1,
            "PoS get_difficulty with 0 validators must return 1, got {d} \
             — zero difficulty means any block is trivially valid (#1985 class)"
        );
    }

    /// `get_difficulty` must return 1 with exactly one validator active.
    #[tokio::test]
    async fn pos_get_difficulty_single_validator_returns_one() {
        let engine = ProofOfStake::new(10, 5);
        engine.add_validator("solo".into(), 100).await.unwrap();
        let d = engine.get_difficulty().await;
        assert_eq!(
            d, 1,
            "PoS get_difficulty with 1 validator must return 1, got {d}"
        );
    }

    #[tokio::test]
    async fn evict_sub_min_stake_is_idempotent() {
        // Calling twice in a row must be a no-op on the second call — no
        // validator is "re-evicted", no spurious log spam in production.
        let engine = ProofOfStake::new(100, 4);
        // add_validator enforces stake >= min_stake, so seed at 200 and
        // simulate the slash via debit_validator_stake to drop below.
        engine.add_validator("v1".into(), 200).await.unwrap();
        let _ = engine.debit_validator_stake("v1", 180).await;

        let first = engine.evict_sub_min_stake_validators().await;
        assert_eq!(first, vec!["v1".to_string()]);
        let second = engine.evict_sub_min_stake_validators().await;
        assert!(
            second.is_empty(),
            "second eviction pass must report nothing to evict"
        );
    }
}
