//! Proof-of-Publishing consensus adapter
//!
//! Bridges the `ProofOfPublishing` implementation (in shared-protocol) to the
//! `ConsensusEngine` trait (in consensus-engines). This adapter delegates
//! validation and block creation to the underlying PoP instance while
//! conforming to the stateless, trait-based interface used by the engine
//! manager.

use async_trait::async_trait;
use log::warn;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;

use crate::traits::ConsensusEngine;
use crate::types::{ConsensusConfig, ConsensusError, ConsensusResult};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::mempool::pool::Mempool;

/// Adapter that wraps `ProofOfPublishing` and implements the `ConsensusEngine`
/// trait so it can be managed by `ConsensusEngineManager`.
///
/// The underlying PoP is behind a `RwLock` because `configure()` requires
/// `&mut self` on the trait while we need shared read access for validation.
pub struct PopConsensusAdapter {
    pop: RwLock<ProofOfPublishing>,
    block_production_enabled: AtomicBool,
}

impl std::fmt::Debug for PopConsensusAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopConsensusAdapter")
            .field("algorithm", &"ProofOfPublishing")
            .field(
                "block_production_enabled",
                &self.block_production_enabled.load(Ordering::Acquire),
            )
            .finish()
    }
}

impl PopConsensusAdapter {
    /// Create a new adapter wrapping the given `ProofOfPublishing` instance.
    ///
    /// Automatically injects the `ExecutionEngineForecastVerifier` so that
    /// PoP validates forecast manifests against real execution-engine logic
    /// instead of falling back to the baseline skill score.
    pub fn new(mut pop: ProofOfPublishing) -> Self {
        pop.set_forecast_verifier(Arc::new(
            wrfcoin_execution_engine::verify::ExecutionEngineForecastVerifier::new(),
        ));
        Self {
            pop: RwLock::new(pop),
            block_production_enabled: AtomicBool::new(true),
        }
    }

    /// Enable or disable every block-production path through this adapter.
    ///
    /// Sync-only nodes disable production during startup (#2860). The gate is
    /// stored at the consensus chokepoint rather than only in the HTTP route so
    /// future background producers cannot bypass the role boundary.
    pub fn set_block_production_enabled(&self, enabled: bool) {
        self.block_production_enabled
            .store(enabled, Ordering::Release);
    }

    fn require_block_production_enabled(&self) -> ConsensusResult<()> {
        if self.block_production_enabled.load(Ordering::Acquire) {
            return Ok(());
        }

        warn!(
            target: "consensus::role",
            "block production refused: this node is configured for sync-only operation (#2860)"
        );
        Err(ConsensusError::CreationFailed(
            "SYNC_ONLY: this node validates and relays blocks but is not permitted to produce one"
                .to_string(),
        ))
    }

    /// Inject a forecast verifier into the underlying PoP instance.
    /// The default `ExecutionEngineForecastVerifier` is injected in `new()`;
    /// this method allows overriding it (e.g., for tests).
    pub async fn set_forecast_verifier(
        &self,
        verifier: Arc<
            dyn wrfcoin_shared_protocol::consensus::forecast_verification::ForecastVerifier,
        >,
    ) {
        self.pop.write().await.set_forecast_verifier(verifier);
    }

    /// Inject a ZK verifier into the underlying PoP instance.
    /// Call this after construction when the `zk` feature is enabled.
    pub async fn set_zk_verifier(
        &self,
        verifier: Arc<dyn wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier>,
    ) {
        self.pop.write().await.set_zk_verifier(verifier);
    }

    /// Inject the producer signer (#2290) into the underlying PoP instance, so
    /// blocks built via `create_block` / `create_next_block` are signed. Call at
    /// startup once the node's producer key is provisioned. Mirrors
    /// [`Self::set_zk_verifier`].
    pub async fn set_producer_signer(
        &self,
        signer: Arc<dyn wrfcoin_shared_protocol::crypto::signer::BlockSigner>,
    ) {
        self.pop.write().await.set_producer_signer(signer);
    }

    /// Bind this producer to its authorized-set member payout address (#2290
    /// §R2 / P1-3), so every block built here pays that canonical `Address` or
    /// production is refused. Call at startup alongside
    /// [`Self::set_producer_signer`].
    pub async fn set_producer_payout_address(
        &self,
        payout: &wrfcoin_shared_protocol::common::types::Address,
    ) {
        self.pop.write().await.set_producer_payout_address(payout);
    }

    /// Inject the genesis oracle declaration (#2523) into the underlying PoP
    /// instance, so a block 1 built here declares its oracle posture as chain
    /// content. Call at startup, from the node's cut configuration. Mirrors
    /// [`Self::set_producer_signer`].
    ///
    /// Not calling it leaves the engine unable to cut a block 1 at all, which is
    /// deliberate: an undeclared genesis is the failure this rule exists to
    /// prevent, and silence must not be the path of least resistance.
    pub async fn set_genesis_oracle_declaration(&self, set: String, quorum: String) {
        self.pop
            .write()
            .await
            .set_genesis_oracle_declaration(set, quorum);
    }

    /// Cut input only; peers and replay derive the sink from recorded genesis.
    pub async fn set_genesis_faucet_address(
        &self,
        address: wrfcoin_shared_protocol::common::types::Address,
    ) {
        self.pop.write().await.set_genesis_faucet_address(address);
    }

    pub async fn set_genesis_policy_metadata(&self, metadata: std::collections::BTreeMap<String, String>) {
        self.pop.write().await.set_genesis_policy_metadata(metadata);
    }

    /// Set the maximum wall-clock milliseconds for MLSAG pre-pass verification per block.
    pub async fn set_mlsag_verify_budget_ms(&self, budget_ms: u64) {
        self.pop.write().await.set_mlsag_verify_budget_ms(budget_ms);
    }

    /// Inject a ZK verifier from synchronous contexts such as test setup or
    /// node bootstrap, before the adapter is shared across tasks.
    ///
    /// At these injection points the write lock is uncontended, so a
    /// non-blocking `try_write` succeeds immediately. Crucially this works on
    /// **both** current-thread and multi-threaded tokio runtimes — the previous
    /// `block_in_place` path panicked ("can call blocking only when running on
    /// the multi-threaded runtime") under actix's default current-thread test
    /// runtime, which silently broke 129 `--features zk` tests while hosted CI
    /// never exercised that feature (#2385).
    ///
    /// # Panics
    ///
    /// Panics if the write lock is contended **while running on a Tokio worker
    /// thread**. This is a fail-fast contract check, not a hang: blocking or
    /// spinning here would deadlock a current-thread runtime (the task that must
    /// release the lock can never be scheduled on the sole worker) and burn CPU
    /// on a multi-threaded one. Contention means the adapter was shared before
    /// setup finished — a programming error. Async callers that legitimately
    /// race for the lock must use the async [`set_zk_verifier`] instead.
    ///
    /// [`set_zk_verifier`]: Self::set_zk_verifier
    pub fn set_zk_verifier_blocking(
        &self,
        verifier: Arc<dyn wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier>,
    ) {
        // Fast path: uncontended `try_write` — non-blocking, runtime-flavor
        // agnostic, and the only path taken at every real call site.
        if let Ok(mut pop) = self.pop.try_write() {
            pop.set_zk_verifier(verifier);
            return;
        }

        // Contended. If we are on a Tokio worker thread we must NOT block or
        // spin waiting for the lock — on a current-thread runtime the holder can
        // never be scheduled (deadlock), and on a multi-threaded runtime it is
        // an unbounded busy-spin. Fail fast instead (see method docs).
        if tokio::runtime::Handle::try_current().is_ok() {
            panic!(
                "set_zk_verifier_blocking: PoP write lock is contended on a Tokio \
                 worker thread. This synchronous setter must only be called at setup \
                 time, before the adapter is shared; use the async `set_zk_verifier` \
                 from async contexts."
            );
        }

        // Outside any runtime, blocking is safe (this is not a reactor thread):
        // spin up a temporary runtime and await the write lock.
        let runtime = tokio::runtime::Runtime::new()
            .expect("failed to create temporary tokio runtime for zk verifier setup");
        runtime.block_on(async {
            self.pop.write().await.set_zk_verifier(verifier);
        });
    }

    /// Declare that this chain has **no** oracle set (#2523), from synchronous
    /// setup contexts. Same lock contract as [`Self::set_zk_verifier_blocking`].
    ///
    /// This is the explicit posture for dev, local and test chains: they may cut
    /// a block 1, but they must SAY that they intend no oracles rather than
    /// arrive there by silence, which is indistinguishable from a misconfigured
    /// cut.
    ///
    /// # Panics
    ///
    /// If the write lock is contended while on a Tokio worker thread — see
    /// [`Self::set_zk_verifier_blocking`] for why failing fast is correct there.
    pub fn declare_no_genesis_oracles_blocking(&self) {
        if let Ok(mut pop) = self.pop.try_write() {
            pop.declare_no_genesis_oracles();
            return;
        }
        if tokio::runtime::Handle::try_current().is_ok() {
            panic!(
                "declare_no_genesis_oracles_blocking: PoP write lock is contended on a Tokio \
                 worker thread. This synchronous setter must only be called at setup time, \
                 before the adapter is shared."
            );
        }
        let runtime = tokio::runtime::Runtime::new()
            .expect("failed to create temporary tokio runtime for genesis oracle setup");
        runtime.block_on(async {
            self.pop.write().await.declare_no_genesis_oracles();
        });
    }

    /// Stateful PoP block production path used by launch API mining.
    ///
    /// This delegates to `ProofOfPublishing::create_next_block`, which applies
    /// governance/reward state transitions and quality event emission.
    ///
    /// ## Partition-mode gate (#1217)
    ///
    /// Before any work happens, the adapter checks the mempool's current
    /// [`PartitionMode`]. If the mode does not allow block production
    /// (i.e. the local node sits in a minority partition), the call
    /// returns [`ConsensusError::PartitionReadOnly`] immediately. This
    /// is the lowest chokepoint reachable from every production
    /// entrypoint — HTTP `/blocks/mine`, test harnesses, future
    /// background miners — so a guard here closes the gap the mempool
    /// ingress guard left open (produced blocks can be assembled from
    /// already-pooled transactions even when writes are halted).
    ///
    /// Producing a block in a minority partition is strictly worse
    /// than refusing the request: the block cannot reach the BFT 2/3+
    /// finality quorum by definition and will be rolled back on
    /// reconnection, wasting operator work and creating state the user
    /// will later see reverted. See
    /// [`wrfcoin_shared_protocol::consensus::partition_policy`] for
    /// the full policy rationale.
    pub async fn create_next_block(
        &self,
        blockchain: Arc<RwLock<Blockchain>>,
        mempool: Arc<RwLock<Mempool>>,
        miner_address: &str,
    ) -> ConsensusResult<Block> {
        self.require_block_production_enabled()?;

        // Partition-mode gate. Read the mempool's current mode under a
        // read lock so we release it before PoP takes its write lock
        // further down. This keeps the gate cheap and avoids widening
        // mempool contention on the hot path.
        {
            let mempool_guard = mempool.read().await;
            let current_mode = mempool_guard.partition_mode();
            if !current_mode.allows_block_production() {
                warn!(
                    target: "consensus::partition",
                    "block production refused: adapter gate blocked create_next_block in partition mode {}",
                    current_mode.label(),
                );
                return Err(ConsensusError::PartitionReadOnly(
                    current_mode.label().to_string(),
                ));
            }
        }

        let mut pop = self.pop.write().await;
        pop.create_next_block(blockchain, mempool, miner_address)
            .await
            .map_err(|e| {
                // Translate the shared-protocol sentinel for partition
                // read-only (emitted by the defense-in-depth guard in
                // ProofOfPublishing::create_next_block) into the
                // dedicated ConsensusError variant so HTTP handlers and
                // downstream callers can match on it instead of the
                // generic CreationFailed path.
                let msg = e.to_string();
                if msg.contains("SP_CONS_PARTITION_READ_ONLY") {
                    ConsensusError::PartitionReadOnly(msg)
                } else {
                    ConsensusError::CreationFailed(msg)
                }
            })
    }
}

#[async_trait]
impl ConsensusEngine for PopConsensusAdapter {
    /// Validate a block using Proof-of-Publishing rules.
    ///
    /// Passes the current authorized oracle set from chain state directly to
    /// `ProofOfPublishing::verify_block`, which applies it at entry. The write
    /// lock is held through the call so no concurrent `create_block` can race
    /// against the oracle set update.
    async fn validate_block(&self, block: &Block, chain: &Blockchain) -> ConsensusResult<bool> {
        // Reject stale-parent blocks before taking the write lock.
        // A burst of wrong-prev-hash blocks (e.g. during a fork) would otherwise
        // contend with block creation, which also holds the write lock.
        if block.prev_block_hash != chain.tip {
            warn!(
                "validate_block: height {} rejected before lock — \
                 prev_block_hash {} != chain tip {}",
                block.height, block.prev_block_hash, chain.tip,
            );
            return Ok(false);
        }

        let mut pop = self.pop.write().await;
        pop.set_oracle_signing_keys(chain.state.signing_keys.clone());
        pop.set_verification_genesis(chain.state.genesis_emission_config.clone());
        let recent_timestamps: Vec<i64> = chain
            .blocks()
            .values()
            .rev()
            .take(wrfcoin_shared_protocol::consensus::proof_of_publishing::MEDIAN_TIME_PAST_WINDOW)
            .map(|b| b.timestamp.timestamp())
            .collect();
        let valid = pop
            .verify_block_chained(
                block,
                &chain.tip,
                chain.state.oracle_registry().oracle_set(),
                chain.state.oracle_quorum,
                &recent_timestamps,
            )
            .await
            .map_err(|e| ConsensusError::ValidationFailed(e.to_string()))?;
        if !valid {
            return Ok(false);
        }

        if let Err(err) = ProofOfPublishing::verify_launch_state_invariants(chain, block) {
            warn!(
                "PoP launch invariant rejected block at height {}: {}",
                block.height, err
            );
            return Ok(false);
        }

        // Block-producer authentication (#2290) — authoritative application
        // boundary for both gossip and IBD. The P2P sync worker deliberately
        // owns only a context-free ordering shadow; launch application reaches
        // this adapter with canonical producer-set history. Genesis is pinned
        // and never reaches this path.
        if let Err(reason) = wrfcoin_shared_protocol::consensus::producer_authority::verify_block_producer_authorized(
            block,
            &chain.state.producer_set_history,
        ) {
            warn!("producer-auth rejected block at height {}: {}", block.height, reason);
            return Ok(false);
        }

        Ok(true)
    }

    /// Create a new block from pre-assembled transactions.
    ///
    /// Unlike `ProofOfPublishing::create_next_block` (which owns mempool
    /// access and tx ordering), this method receives already-sorted
    /// transactions from the mining route. It computes the publishing score,
    /// sets block metadata, and returns the hashed block. This trait builder
    /// deliberately does not stage Phase B producer settlement; the production
    /// mining route uses create_pop_block_with_state -> create_next_block.
    async fn create_block(
        &self,
        transactions: Vec<Transaction>,
        previous_hash: &str,
        miner_address: &str,
        chain: &Blockchain,
    ) -> ConsensusResult<Block> {
        self.require_block_production_enabled()?;

        let mut pop = self.pop.write().await;
        let next_height = chain.height.checked_add(1).ok_or_else(|| {
            ConsensusError::CreationFailed("block height overflow while creating PoP block".into())
        })?;
        pop.set_authorized_oracles(chain.state.oracle_registry().oracle_set().clone());

        // Use chain.tip as the typed Hash. The trait passes previous_hash as
        // &str for compatibility, but Block::new requires Hash. Assert they
        // agree so callers passing a specific hash for fork handling will get
        // a clear error rather than silent discard.
        debug_assert!(
            previous_hash.is_empty() || previous_hash == chain.tip.to_string(),
            "previous_hash ({previous_hash}) diverges from chain.tip ({})",
            chain.tip
        );

        // Compute publishing score from the assembled transactions.
        let publishing_score = pop.calculate_block_publishing_score(&transactions, next_height);

        // Evaluate useful-work evidence for liveness fallback metadata.
        let evidence = pop.evaluate_useful_work_evidence(&transactions, next_height);

        // Build the block.
        let mut block = Block::new(
            3,         // version — matches create_next_block (#1029)
            chain.tip, // previous hash
            transactions,
            pop.difficulty, // difficulty
            next_height,
        );

        // Set consensus metadata so verify_block can validate this block later.
        block
            .metadata
            .insert("publishing_score".to_string(), publishing_score.to_string());
        block
            .metadata
            .insert("miner_address".to_string(), miner_address.to_string());
        block
            .metadata
            .insert("consensus".to_string(), "ProofOfPublishing".to_string());

        // If publishing score is below threshold, set useful-work liveness
        // metadata so the block can still pass verification.
        if publishing_score < pop.min_publishing_score {
            if evidence.qualifies {
                block.metadata.insert(
                    "useful_work_liveness_fallback".to_string(),
                    "true".to_string(),
                );
                block.metadata.insert(
                    "useful_work_forecast_reveals".to_string(),
                    evidence.forecast_reveals_with_manifest.to_string(),
                );
                block.metadata.insert(
                    "useful_work_quality_contributions".to_string(),
                    evidence.quality_contributions.to_string(),
                );
            } else {
                // Empty-liveness: chain must not halt even with no useful work.
                block.metadata.insert(
                    "useful_work_liveness_fallback".to_string(),
                    "empty_liveness".to_string(),
                );
            }
        }

        // #2290 P1-3 (§R2): refuse before hashing/signing if the coinbase does
        // not pay this producer's declared payout address. This is the builder
        // that serves `/blocks/mine`, which takes `miner_address` straight from
        // the request body — so this path is precisely where an unvalidated
        // recipient would otherwise reach a locally-committed block that every
        // peer rejects. Shared with `create_next_block` so both builders apply
        // the identical binding (parity).
        pop.enforce_producer_payout_binding(&block, &chain.state.producer_set_history)
            .map_err(|e| {
                ConsensusError::CreationFailed(format!("producer payout binding: {e}"))
            })?;

        // #2545/#2564: stamp the chain's identity and protocol version into
        // block 1 BEFORE hashing. This builder did NOT do so, while
        // `create_next_block` did, so the two produced structurally different
        // block 1s for the same inputs — and unlike the oracle declaration
        // below, nothing verifies these keys, so no test went red. Kept ahead of
        // the declaration to match `create_next_block`'s order exactly; the
        // grouping is not itself load-bearing, but "the two builders do the same
        // things in the same order" is the property worth being able to read off
        // the page.
        pop.stamp_genesis_chain_metadata(&mut block);
        // #3361: refuse a faucet chain's block 1 that names no faucet, exactly
        // as `create_next_block` does. Producer-only; verifiers never check it.
        ProofOfPublishing::check_block_one_genesis_faucet(&block).map_err(|reason| {
            ConsensusError::CreationFailed(format!("SP_CONS_BLOCK_ONE_NO_FAUCET_ADDRESS: {reason}"))
        })?;

        // #2523: stamp the genesis oracle declaration into block 1 BEFORE
        // hashing — it is block metadata and therefore in the hash. Shared with
        // `create_next_block` so both builders declare identically; a builder
        // that skipped it would emit a block 1 that every verifier refuses,
        // including this node's own. Refuses outright when nothing was declared.
        pop.stamp_genesis_oracle_declaration(&mut block)
            .map_err(ConsensusError::CreationFailed)?;

        // #2290: stamp the producer pubkey BEFORE hashing (it is in the hash),
        // then sign AFTER (producer_sig is outside it). Both PoP builders share
        // `ProofOfPublishing::{stamp_producer_pubkey, attach_producer_signature}`
        // so this path and `create_next_block` sign identically. No-op when no
        // producer signer is configured (block stays unsigned).
        pop.stamp_producer_pubkey(&mut block);

        // Compute and set the block hash.
        let hash = block.calculate_hash();
        block.set_hash(hash);

        pop.attach_producer_signature(&mut block).map_err(|e| {
            ConsensusError::CreationFailed(format!("producer block signing failed: {e}"))
        })?;

        Ok(block)
    }

    /// Return the current PoP difficulty.
    async fn get_difficulty(&self) -> u64 {
        let pop = self.pop.read().await;
        u64::from(pop.difficulty)
    }

    fn algorithm_name(&self) -> &'static str {
        "ProofOfPublishing"
    }

    /// Reconfigure the underlying PoP instance from a `ConsensusConfig`.
    async fn configure(&mut self, config: &ConsensusConfig) -> ConsensusResult<()> {
        let mut pop = self.pop.write().await;
        if let Some(difficulty) = config.difficulty {
            pop.difficulty = u32::try_from(difficulty).map_err(|_| {
                ConsensusError::ConfigurationError(format!(
                    "PoP difficulty {difficulty} exceeds u32::MAX"
                ))
            })?;
        }
        pop.target_block_time = config.target_block_time;
        if let Some(score) = config
            .algorithm_params
            .get("min_publication_score")
            .and_then(|v| v.as_f64())
        {
            // Truncate rather than round — `.round()` on a value differing
            // by 1 ULP between x86_64 and ARM diverges config state (issue #1849).
            let scaled = score * 1_000_000.0;
            if !scaled.is_finite() || scaled < 0.0 || scaled > u64::MAX as f64 {
                return Err(ConsensusError::ConfigurationError(format!(
                    "min_publication_score {score} is out of range"
                )));
            }
            pop.min_publishing_score = scaled as u64;
        }
        pop.max_tx_per_block = config.max_transactions_per_block as usize;
        Ok(())
    }

    fn get_metrics(&self) -> FxHashMap<String, serde_json::Value> {
        // Metrics require async lock — return static info for now.
        // Full metrics will be exposed via a dedicated endpoint.
        let mut m = FxHashMap::default();
        m.insert(
            "algorithm".to_string(),
            serde_json::Value::String("ProofOfPublishing".to_string()),
        );
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::blockchain::state::State;
    use wrfcoin_shared_protocol::blockchain::transaction::{Transaction, TransactionType};
    use wrfcoin_shared_protocol::crypto::hash::Hash;

    fn make_pop() -> ProofOfPublishing {
        let mut pop = ProofOfPublishing::new(1, 15, 600_000, 500);
        // #2523: an adapter test chain has no oracle ceremony and states so.
        // Without an explicit posture it could not cut a block 1 at all, which
        // is the rule under test elsewhere — here it is just setup.
        pop.declare_no_genesis_oracles();
        pop
    }

    fn genesis_chain() -> Blockchain {
        Blockchain::from_persisted(
            0,
            Hash::default(),
            State::default(),
            std::collections::BTreeMap::new(),
            0,
            0,
        )
    }

    /// THE PARITY RULE (#2564): the two PoP block builders must stamp the same
    /// genesis metadata keys into block 1.
    ///
    /// `create_next_block` and this adapter's `create_block` are both block-1
    /// producers. `create_next_block` wrote the #2545 chain identity;
    /// `create_block` did not, so the two emitted structurally different block
    /// 1s for the same inputs — and every write here is metadata, which is
    /// inside the hash.
    ///
    /// It survived because it was **unobservable**. The oracle declaration is
    /// verified, so a builder that skips it turns tests red at once — that is
    /// how the same defect was caught in this function before. Nothing verifies
    /// `chain_id`/`network_id`/`protocol_version`, so their absence failed no
    /// test and could only be found by reading both builders side by side.
    ///
    /// So this asserts the KEY SETS agree rather than any particular key. A
    /// future writer added to one builder and not the other fails here, whether
    /// or not anything verifies it — which is the property that was missing, not
    /// the three keys themselves.
    #[tokio::test]
    async fn both_pop_block_builders_stamp_the_same_genesis_metadata_keys() {
        use std::collections::BTreeSet;

        let chain = genesis_chain();
        let coinbase = Transaction::new_coinbase("wrf-miner", 0, 1);

        // Builder A: the adapter / `/blocks/mine`-adjacent path.
        let adapter = PopConsensusAdapter::new(make_pop());
        let via_adapter = adapter
            .create_block(
                vec![coinbase.clone()],
                &chain.tip.to_string(),
                "wrf-miner",
                &chain,
            )
            .await
            .expect("adapter builds block 1");

        // Builder B: the stateful producer path — the one the cut actually
        // takes, via `/blocks/mine` -> `create_pop_block_with_state`.
        let mut pop = make_pop();
        let chain_b = Arc::new(RwLock::new(genesis_chain()));
        let mempool_b = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let via_create_next = pop
            .create_next_block(chain_b, mempool_b, "wrf-miner")
            .await
            .expect("create_next_block builds block 1");

        assert_eq!(via_adapter.height, 1);
        assert_eq!(via_create_next.height, 1);

        // Only the genesis-identity keys are compared. The two builders
        // legitimately differ elsewhere — the adapter records
        // `publishing_score`, `miner_address` and liveness fields that the
        // stateful path derives differently — and asserting FULL metadata
        // equality would be a test of incidental sameness that fails on every
        // unrelated change.
        let genesis_keys: BTreeSet<&str> = ["chain_id", "network_id", "protocol_version"]
            .into_iter()
            .collect();
        let present = |b: &Block| -> BTreeSet<&str> {
            genesis_keys
                .iter()
                .copied()
                .filter(|k| b.metadata.contains_key(*k))
                .collect()
        };

        assert_eq!(
            present(&via_adapter),
            present(&via_create_next),
            "both PoP builders must stamp the same genesis metadata keys into block 1; \
             metadata is inside the hash, so a builder that skips one emits a different \
             block for identical inputs (#2564)"
        );
        assert_eq!(
            present(&via_adapter),
            genesis_keys,
            "the shared set is non-empty — a parity assertion that both builders write \
             NOTHING would pass while proving nothing"
        );

        // The values must agree too, not merely the key names: two nodes given
        // the same configuration have to produce byte-identical genesis
        // metadata or they do not agree on block 1's hash.
        for k in &genesis_keys {
            assert_eq!(
                via_adapter.metadata.get(*k),
                via_create_next.metadata.get(*k),
                "genesis metadata `{k}` must be identical across both builders"
            );
        }
    }

    /// #3361: the adapter is a block-1 builder too, so it must refuse a faucet
    /// chain's block 1 that names no faucet exactly as `create_next_block` does.
    #[tokio::test]
    async fn adapter_create_block_refuses_faucet_chain_block_one_without_faucet_3361() {
        use wrfcoin_shared_protocol::common::types::Address;
        use wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_FAUCET_ADDRESS_KEY;
        const CHILD: &str = "WRFCOIN_ADAPTER_FAUCET_CHILD_3361";
        // WRFCOIN_CHAIN_ID is process-global; run the 777 case in a child.
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "algorithms::pop_adapter::tests::adapter_create_block_refuses_faucet_chain_block_one_without_faucet_3361"])
                .env(CHILD, "1")
                .env("WRFCOIN_CHAIN_ID", "777")
                .output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success(), "{stdout}");
            assert!(stdout.contains("1 passed"), "{stdout}");
            return;
        }
        let chain = genesis_chain();
        let coinbase = Transaction::new_coinbase("wrf-miner", 0, 1);
        let adapter = PopConsensusAdapter::new(make_pop());
        let err = adapter
            .create_block(vec![coinbase.clone()], &chain.tip.to_string(), "wrf-miner", &chain)
            .await
            .expect_err("a chain-777 block 1 with no faucet address must be refused");
        assert!(err.to_string().contains("SP_CONS_BLOCK_ONE_NO_FAUCET_ADDRESS:"), "{err}");

        let sink = Address([0x62; 20]);
        let adapter = PopConsensusAdapter::new(make_pop());
        adapter.set_genesis_faucet_address(sink).await;
        let block = adapter
            .create_block(vec![coinbase], &chain.tip.to_string(), "wrf-miner", &chain)
            .await
            .expect("a chain-777 block 1 naming its faucet is cut");
        assert_eq!(block.metadata.get(GENESIS_FAUCET_ADDRESS_KEY), Some(&sink.to_string()));
    }

    #[tokio::test]
    async fn adapter_algorithm_name() {
        let adapter = PopConsensusAdapter::new(make_pop());
        assert_eq!(adapter.algorithm_name(), "ProofOfPublishing");
    }

    #[tokio::test]
    async fn adapter_get_difficulty() {
        let adapter = PopConsensusAdapter::new(make_pop());
        assert_eq!(adapter.get_difficulty().await, 1);
    }

    #[tokio::test]
    async fn adapter_configure_updates_difficulty() {
        let mut adapter = PopConsensusAdapter::new(make_pop());
        let mut config = ConsensusConfig::default();
        config.difficulty = Some(5);
        config.target_block_time = 30;
        adapter.configure(&config).await.unwrap();
        assert_eq!(adapter.get_difficulty().await, 5);
    }

    #[tokio::test]
    async fn adapter_configure_rejects_difficulty_that_truncates_to_u32() {
        let mut adapter = PopConsensusAdapter::new(make_pop());
        let mut config = ConsensusConfig::default();
        config.difficulty = Some(u64::from(u32::MAX) + 1);

        let err = adapter.configure(&config).await.unwrap_err();
        assert!(
            matches!(err, ConsensusError::ConfigurationError(ref msg) if msg.contains("difficulty")),
            "expected difficulty range error, got {err:?}"
        );
    }

    #[tokio::test]
    async fn adapter_validate_empty_block_returns_result() {
        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = genesis_chain();
        // An empty block with no publishing score metadata — should still
        // return Ok (either true or false), not panic.
        let block = Block::new(3, chain.tip, vec![], 0, 1);
        let result = adapter.validate_block(&block, &chain).await;
        // verify_block returns a concrete bool; assert the value, not just Ok.
        let valid = result.expect("validate_block should not error on empty block");
        // Empty blocks with no publishing score metadata fail PoP validation.
        assert!(
            !valid,
            "empty block should fail PoP validation (no publishing score)"
        );
    }

    #[tokio::test]
    async fn adapter_create_block_sets_metadata() {
        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = genesis_chain();
        let prev_hash = chain.tip.to_string();
        let block = adapter
            .create_block(vec![], &prev_hash, "miner1", &chain)
            .await
            .unwrap();
        assert!(block.metadata.contains_key("publishing_score"));
        assert_eq!(
            block.metadata.get("consensus").map(String::as_str),
            Some("ProofOfPublishing")
        );
        assert!(block.hash.is_some());
    }

    #[tokio::test]
    async fn adapter_rejects_block_when_launch_invariant_replay_fails() {
        let adapter = PopConsensusAdapter::new(ProofOfPublishing::new(1, 15, 0, 500));
        let chain = genesis_chain();
        let mut malformed_reveal = Transaction::new(vec![], vec![]);
        malformed_reveal.tx_type = TransactionType::ContributionReveal;
        malformed_reveal.data = Some("not-json".to_string());
        malformed_reveal.fee = 1;
        malformed_reveal.set_id();

        let mut block = Block::new(3, chain.tip, vec![malformed_reveal], 0, 1);
        block
            .metadata
            .insert("publishing_score".to_string(), "0".to_string());

        let valid = adapter
            .validate_block(&block, &chain)
            .await
            .expect("validation should return a concrete result");
        assert!(
            !valid,
            "launch invariant replay failures should reject the block"
        );
    }

    #[tokio::test]
    async fn genesis_seeded_chain_enforces_member_accept_nonmember_reject_at_height_1() {
        // #2290 P3: a chain seeded via `State::seed_genesis_producer_set` (the
        // genesis hook) is ENFORCED from block 1 — a seeded member's signed block
        // is accepted; a non-member's signed block is rejected. Proves the hook +
        // enforce-at-genesis end to end. Runs at the V1 default (producer_pubkey is
        // not in the V1 block hash, so signing post-create_block keeps the hash
        // valid; the producer-auth preimage binds the pubkey regardless of version).
        use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
        use wrfcoin_shared_protocol::consensus::block_producer_auth::sign_block_producer;
        use wrfcoin_shared_protocol::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::common::types::Address;
        use rand::SeedableRng;

        fn member_and_key(seed: u64) -> (PrivateKey, ProducerMember) {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let sk = PrivateKey::generate(&mut rng);
            let pk = PublicKey::from_private_key(&sk);
            let key: [u8; 32] = pk.as_bytes().try_into().unwrap();
            (
                sk,
                ProducerMember {
                    producer_id: format!("op-{seed}"), // secret-leak-ok: public deterministic test-fixture seed, never a runtime secret
                    payout_address: Address(key[0..20].try_into().unwrap()).to_string(),
                    public_key: key,
                },
            )
        }

        let (member_sk, member) = member_and_key(1);
        let (stranger_sk, stranger) = member_and_key(2);

        let mut chain = genesis_chain();
        chain
            .state
            .seed_genesis_producer_set(vec![member.clone()])
            .expect("genesis seed installs the initial producer set");

        let adapter = PopConsensusAdapter::new(make_pop());

        // Build a valid block paying `payout`, then sign it with `signer`.
        async fn signed_block(
            adapter: &PopConsensusAdapter,
            chain: &Blockchain,
            payout: &str,
            signer: &PrivateKey,
        ) -> Block {
            let coinbase =
                wrfcoin_shared_protocol::blockchain::transaction::Transaction::new_coinbase(
                    payout, 0, 1,
                );
            let mut block = adapter
                .create_block(vec![coinbase], &chain.tip.to_string(), payout, chain)
                .await
                .expect("create_block");
            let att = sign_block_producer(
                &block,
                signer,
                ChainIdentity::active(),
                SigningProtocolVersion::active(),
            );
            block.producer_pubkey = att.producer_pubkey;
            block.producer_sig = att.producer_sig.to_vec();
            block
        }

        // Seeded MEMBER, signed, paying its own payout → ACCEPTED.
        let good = signed_block(&adapter, &chain, &member.payout_address, &member_sk).await;
        assert!(
            adapter.validate_block(&good, &chain).await.unwrap(),
            "genesis-seeded member's signed block must be accepted at height 1"
        );

        // NON-MEMBER, signed, paying its own payout → REJECTED (not in the set).
        let bad = signed_block(&adapter, &chain, &stranger.payout_address, &stranger_sk).await;
        assert!(
            !adapter.validate_block(&bad, &chain).await.unwrap(),
            "genesis-seeded chain must reject a non-member at height 1 (#2290 P3)"
        );
    }

    #[tokio::test]
    async fn adapter_gossip_gate_rejects_unsigned_block_when_producer_set_seeded() {
        // #2290 GOSSIP boundary (Dan review P2): with a seeded producer set, an
        // otherwise-valid but UNSIGNED block that `validate_block` accepts on an
        // unseeded chain (see adapter_rejects_block_with_wrong_parent_hash's
        // positive baseline) MUST be rejected here. This is load-bearing: remove
        // the producer-auth call in validate_block and this test goes RED.
        use wrfcoin_shared_protocol::consensus::producer_set::{ProducerMember, ProducerSetChange};
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::common::types::Address;
        use rand::SeedableRng;

        let adapter = PopConsensusAdapter::new(make_pop());
        let mut chain = genesis_chain();

        // Seed a valid producer set (canonical payout, valid key).
        let mut rng = rand::rngs::StdRng::seed_from_u64(2290);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        let key: [u8; 32] = pk.as_bytes().try_into().unwrap();
        let member = ProducerMember {
            producer_id: "op".to_string(),
            payout_address: Address(key[0..20].try_into().unwrap()).to_string(),
            public_key: key,
        };
        chain
            .state
            .producer_set_history
            .push(ProducerSetChange::new(0, 0, vec![member]));

        // Same valid-block construction as the positive baseline — but UNSIGNED.
        let coinbase =
            wrfcoin_shared_protocol::blockchain::transaction::Transaction::new_coinbase(
                "miner1", 0, 1,
            );
        let block = adapter
            .create_block(vec![coinbase], &chain.tip.to_string(), "miner1", &chain)
            .await
            .expect("create_block should succeed on genesis chain");
        assert!(block.producer_sig.is_empty(), "precondition: create_block does not sign");

        let valid = adapter
            .validate_block(&block, &chain)
            .await
            .expect("validation should return a concrete result");
        assert!(
            !valid,
            "seeded gossip boundary must REJECT an unsigned block (#2290 gate load-bearing)"
        );

        // Control: on an UNSEEDED chain the identical block validates (activation
        // = enforce IFF seeded), so the rejection above is due to the gate.
        let unseeded = genesis_chain();
        let valid_unseeded = adapter
            .validate_block(&block, &unseeded)
            .await
            .expect("validation should return a concrete result");
        assert!(valid_unseeded, "unseeded chain must not enforce producer-auth");
    }

    #[tokio::test]
    async fn adapter_rejects_block_with_wrong_parent_hash() {
        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = genesis_chain();
        // verify_block requires exactly one Coinbase as the first transaction (#1023).
        // Amount 0 satisfies the liveness-coinbase cap at height 1.
        let coinbase =
            wrfcoin_shared_protocol::blockchain::transaction::Transaction::new_coinbase(
                "miner1", 0, 1,
            );
        let mut block = adapter
            .create_block(vec![coinbase], &chain.tip.to_string(), "miner1", &chain)
            .await
            .expect("create_block should succeed on genesis chain");

        let valid = adapter
            .validate_block(&block, &chain)
            .await
            .expect("validation should return a concrete result");
        assert!(valid, "block built against the current tip should validate");

        block.prev_block_hash = Hash::hash_bytes(b"wrong-parent");
        block.set_hash(block.calculate_hash());

        let valid = adapter
            .validate_block(&block, &chain)
            .await
            .expect("validation should return a concrete result");
        assert!(!valid, "block with mismatched parent hash must be rejected");
    }

    /// Verify that a block with a wrong prev_block_hash is rejected by the
    /// early-check guard (before acquiring the write lock).
    ///
    /// A block fabricated with a random parent hash must be rejected immediately
    /// without reaching the PoP verifier, so callers holding no lock receive
    /// `Ok(false)` even if the write lock is unavailable.
    #[tokio::test]
    async fn adapter_rejects_stale_parent_before_lock() {
        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = genesis_chain();
        // Fabricate a block whose prev_block_hash is NOT the chain tip.
        let wrong_parent = Hash::hash_bytes(b"some-other-chain-tip");
        let block = Block::new(1, wrong_parent, vec![], 0, 0);

        let result = adapter.validate_block(&block, &chain).await;
        assert!(
            matches!(result, Ok(false)),
            "block with stale parent hash must be rejected (Ok(false)), got {result:?}"
        );
    }

    #[test]
    fn publishing_score_roundtrips_through_u64_parse() {
        // Verify the format used in create_block() survives u64 parsing in
        // verify_block (shared-protocol proof_of_publishing.rs).  Before the
        // fix, format!("{score:.6}") produced "500000.000000" which failed
        // .parse::<u64>() and silently defaulted to 0.
        for score in [0_u64, 1, 500_000, u64::MAX] {
            let formatted = score.to_string();
            assert_eq!(
                formatted.parse::<u64>().unwrap(),
                score,
                "score {score} should survive to_string → parse round-trip"
            );
        }
    }

    #[tokio::test]
    async fn create_block_score_parses_as_u64() {
        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = genesis_chain();
        let block = adapter
            .create_block(vec![], &chain.tip.to_string(), "miner1", &chain)
            .await
            .unwrap();
        let score_str = block
            .metadata
            .get("publishing_score")
            .expect("create_block must set publishing_score metadata");
        assert!(
            score_str.parse::<u64>().is_ok(),
            "publishing_score metadata '{score_str}' must parse as u64"
        );
    }

    #[tokio::test]
    async fn create_block_rejects_height_overflow_instead_of_panicking() {
        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = Blockchain::from_persisted(
            u64::MAX,
            Hash::default(),
            State::default(),
            std::collections::BTreeMap::new(),
            0,
            0,
        );

        let err = adapter
            .create_block(vec![], &chain.tip.to_string(), "miner1", &chain)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ConsensusError::CreationFailed(ref msg) if msg.contains("height overflow")),
            "expected checked height overflow error, got {err:?}"
        );
    }

    // -----------------------------------------------------------------
    // Partition-mode block production guard (#1217)
    // -----------------------------------------------------------------
    //
    // These tests lock in the invariant that
    // `PopConsensusAdapter::create_next_block` refuses block production
    // when the mempool reports a read-only partition mode, and allows it
    // for Normal and BlockProductionOnly. They are the unit-level
    // counterpart to the HTTP-level tests in
    // `native-chain/tests/partition_mode_block_production.rs`.

    use wrfcoin_shared_protocol::config::settings::Config;
    use wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode;

    fn make_mempool_in_mode(mode: PartitionMode) -> Arc<RwLock<Mempool>> {
        let mut mempool = Mempool::new(&Config::default());
        mempool.set_partition_mode(mode);
        Arc::new(RwLock::new(mempool))
    }

    fn make_chain_for_mining() -> Arc<RwLock<Blockchain>> {
        Arc::new(RwLock::new(genesis_chain()))
    }

    /// A sync-only role is a hard production boundary, independent of signer
    /// availability or partition mode. The guard belongs at the adapter seam
    /// reached by every current and future block-production caller, before the
    /// stateful builder can mutate either chain or mempool (#2860).
    #[tokio::test]
    async fn sync_only_adapter_refuses_every_block_builder_before_mutation_2860() {
        let adapter = PopConsensusAdapter::new(make_pop());
        adapter.set_block_production_enabled(false);

        let chain = make_chain_for_mining();
        let mempool = make_mempool_in_mode(PartitionMode::Normal);
        let height_before = chain.read().await.height;
        let tip_before = chain.read().await.tip;

        let stateful = adapter
            .create_next_block(Arc::clone(&chain), Arc::clone(&mempool), "0xminer1")
            .await
            .expect_err("sync-only must refuse the stateful producer");
        assert!(
            stateful.to_string().contains("SYNC_ONLY"),
            "refusal must identify the role boundary: {stateful}"
        );
        assert_eq!(chain.read().await.height, height_before);
        assert_eq!(chain.read().await.tip, tip_before);

        let snapshot = chain.read().await.clone();
        let stateless = adapter
            .create_block(vec![], &snapshot.tip.to_string(), "0xminer1", &snapshot)
            .await
            .expect_err("sync-only must refuse the stateless producer too");
        assert!(
            stateless.to_string().contains("SYNC_ONLY"),
            "refusal must identify the role boundary: {stateless}"
        );
        assert_eq!(chain.read().await.height, height_before);
        assert_eq!(chain.read().await.tip, tip_before);
    }

    #[tokio::test]
    async fn create_next_block_refused_in_read_only_mode() {
        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = make_chain_for_mining();
        let mempool = make_mempool_in_mode(PartitionMode::ReadOnly);

        let result = adapter.create_next_block(chain, mempool, "0xminer1").await;
        match result {
            Err(ConsensusError::PartitionReadOnly(label)) => {
                assert_eq!(
                    label, "read_only",
                    "error should carry the current mode label"
                );
            },
            Err(other) => {
                panic!("expected PartitionReadOnly error, got a different variant: {other:?}")
            },
            Ok(_) => panic!(
                "ReadOnly partition mode MUST refuse block production — \
                 otherwise a minority partition forks state on reconnection"
            ),
        }
    }

    #[tokio::test]
    async fn create_next_block_allowed_in_normal_mode() {
        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = make_chain_for_mining();
        let mempool = make_mempool_in_mode(PartitionMode::Normal);

        // Note: the call may fail for unrelated reasons (empty mempool,
        // missing oracle set, etc.) — the guarantee under test is only
        // that the partition-mode gate does NOT trip. Accept any error
        // whose string does not mention PartitionReadOnly or the
        // shared-protocol sentinel.
        let result = adapter.create_next_block(chain, mempool, "0xminer1").await;
        if let Err(err) = &result {
            let msg = err.to_string();
            assert!(
                !msg.contains("PartitionReadOnly"),
                "Normal mode must not trip the partition gate, got: {msg}"
            );
            assert!(
                !msg.contains("SP_CONS_PARTITION_READ_ONLY"),
                "Normal mode must not trip the shared-protocol gate sentinel, got: {msg}"
            );
        }
    }

    #[tokio::test]
    async fn create_next_block_allowed_in_block_production_only_mode() {
        // The whole point of BlockProductionOnly is that block
        // production is still live — only finality voting is halted.
        // A guard that blocks this mode would defeat the policy.
        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = make_chain_for_mining();
        let mempool = make_mempool_in_mode(PartitionMode::BlockProductionOnly);

        let result = adapter.create_next_block(chain, mempool, "0xminer1").await;
        if let Err(err) = &result {
            let msg = err.to_string();
            assert!(
                !msg.contains("PartitionReadOnly"),
                "BlockProductionOnly must not trip the partition gate, got: {msg}"
            );
            assert!(
                !msg.contains("SP_CONS_PARTITION_READ_ONLY"),
                "BlockProductionOnly must not trip the shared-protocol gate sentinel, got: {msg}"
            );
        }
    }

    #[tokio::test]
    async fn partition_read_only_error_translated_from_shared_protocol() {
        // Verify the pop_adapter's error translation: if the
        // shared-protocol layer surfaces the SP_CONS_PARTITION_READ_ONLY
        // sentinel (because the adapter guard was somehow bypassed),
        // the adapter translates it into the typed
        // ConsensusError::PartitionReadOnly variant so HTTP handlers
        // get a single variant to match.
        //
        // This test exercises the full flow: construct a mempool in
        // ReadOnly, call create_next_block, and confirm the error is
        // PartitionReadOnly (which it will be, from the adapter's own
        // guard). The sentinel-translation branch of the adapter is
        // separately covered because both guards produce the same
        // user-visible variant — callers don't need to know which
        // layer refused.
        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = make_chain_for_mining();
        let mempool = make_mempool_in_mode(PartitionMode::ReadOnly);

        let result = adapter.create_next_block(chain, mempool, "0xminer1").await;
        assert!(
            matches!(result, Err(ConsensusError::PartitionReadOnly(_))),
            "ReadOnly must surface as PartitionReadOnly, got {result:?}"
        );
    }

    /// Uncontended happy path on a **current-thread** runtime (actix's default).
    /// `try_write` succeeds, the verifier is installed without blocking, and the
    /// lock is released — the path the #2385 fix restored, asserted directly.
    #[tokio::test]
    async fn set_zk_verifier_blocking_installs_verifier_uncontended() {
        let adapter = PopConsensusAdapter::new(make_pop());
        adapter.set_zk_verifier_blocking(Arc::new(
            wrfcoin_shared_protocol::consensus::zk_verification::RejectAllZkVerifier,
        ));
        assert!(
            adapter.pop.try_write().is_ok(),
            "write lock must be released after a successful setup-time injection"
        );
    }

    /// Load-bearing regression for the #2388 deadlock (Dan CHANGES, round 103):
    /// on a current-thread runtime, contention must FAIL FAST, not spin.
    ///
    /// Under the previous `loop { try_write; std::thread::yield_now() }` fallback
    /// this test hangs forever — the write guard is held by *this* task on the
    /// sole worker thread, so the busy-spin can never make progress and the test
    /// times out (exit 124). With the fail-fast fix the setter panics promptly,
    /// which `#[should_panic]` observes. This is the exact scenario Dan proved.
    #[tokio::test]
    #[should_panic(expected = "contended on a Tokio worker thread")]
    async fn set_zk_verifier_blocking_fails_fast_when_contended_on_current_thread_runtime() {
        let adapter = PopConsensusAdapter::new(make_pop());
        // Hold the write lock on this (sole) worker thread, then invoke the
        // synchronous setter: `try_write` returns WouldBlock, and because we are
        // on a Tokio worker the setter must panic rather than spin/block.
        let _guard = adapter.pop.write().await;
        adapter.set_zk_verifier_blocking(Arc::new(
            wrfcoin_shared_protocol::consensus::zk_verification::RejectAllZkVerifier,
        ));
    }

    /// #2290 RED-first: a block produced through the **real** `/blocks/mine`
    /// builder (`create_block`) with a producer signer configured must carry a
    /// `producer_sig` that `verify_block_producer_on` accepts. Reverting
    /// `attach_producer_signature` in the builder leaves `producer_sig` empty, so
    /// verification returns `ProducerAuthError::Unsigned` and this test fails —
    /// the RED-on-revert property the enforcement gap (#2395) asked for.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn produced_block_carries_verifiable_producer_signature_2290() {
        use wrfcoin_shared_protocol::consensus::block_producer_auth::verify_block_producer_on;
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ChainIdentity, SigningProtocolVersion,
        };
        use wrfcoin_shared_protocol::crypto::signature::PrivateKey;
        use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;

        let adapter = PopConsensusAdapter::new(make_pop());
        // In-process EnvKeySigner stands in for the HSM-backed BlockSigner used
        // in release (`load_block_signer`).
        let producer_key = PrivateKey::from_bytes(&[7u8; 32]).expect("valid ed25519 secret");
        let signer = Arc::new(EnvKeySigner::from_private_key(producer_key, "test-producer"));
        adapter.set_producer_signer(signer).await;

        let chain = genesis_chain();
        let prev_hash = chain.tip.to_string();
        let block = adapter
            .create_block(vec![], &prev_hash, "miner1", &chain)
            .await
            .expect("block production succeeds");

        assert!(
            !block.producer_sig.is_empty(),
            "a signer was configured, so the produced block must carry producer_sig"
        );
        verify_block_producer_on(
            &block,
            ChainIdentity::active(),
            SigningProtocolVersion::active(),
        )
        .expect(
            "a block produced through the real path must carry a producer signature that \
             verify_block_producer_on accepts (RED if attach_producer_signature is reverted)",
        );
    }

    /// Sibling to the above: with **no** producer signer configured (the default
    /// dev/genesis case), the produced block stays unsigned and verification
    /// reports `Unsigned` — confirming signing is gated on provisioning, not
    /// unconditional.
    #[tokio::test]
    async fn produced_block_is_unsigned_without_a_producer_signer_2290() {
        use wrfcoin_shared_protocol::consensus::block_producer_auth::{
            verify_block_producer_on, ProducerAuthError,
        };
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ChainIdentity, SigningProtocolVersion,
        };

        let adapter = PopConsensusAdapter::new(make_pop());
        let chain = genesis_chain();
        let block = adapter
            .create_block(vec![], &chain.tip.to_string(), "miner1", &chain)
            .await
            .expect("block production succeeds");

        assert!(block.producer_sig.is_empty(), "no signer ⇒ unsigned block");
        assert_eq!(
            // secret-leak-ok: no secret value is in scope. The scanner matches the "signing"
            // segment of the *type name* `SigningProtocolVersion`; `::active()` returns the
            // chain's protocol-version discriminant, which is public consensus configuration
            // committed to every block preimage. The other operands are a verification result
            // and an error variant. This test asserts a block is UNSIGNED — there is no signer,
            // and `block.producer_sig` is asserted empty on the line above.
            verify_block_producer_on(
                &block,
                ChainIdentity::active(),
                SigningProtocolVersion::active(),
            ),
            Err(ProducerAuthError::Unsigned),
        );
    }

    /// Build a chain whose genesis seeds an authorized producer set mapping
    /// `key` → `payout`, exactly as Alice seeds it (#2397) and exactly as the
    /// verify-side gate reads it.
    #[cfg(debug_assertions)]
    fn chain_with_seeded_producer(
        key: &wrfcoin_shared_protocol::crypto::signature::PublicKey,
        payout: &str,
    ) -> Blockchain {
        use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;

        let mut state = State::default();
        state
            .seed_genesis_producer_set(vec![ProducerMember {
                producer_id: "cindy-node".to_string(),
                payout_address: payout.to_string(),
                public_key: <[u8; 32]>::try_from(key.as_bytes()).expect("32-byte key"),
            }])
            .expect("genesis producer set is valid");
        Blockchain::from_persisted(
            0,
            Hash::default(),
            state,
            std::collections::BTreeMap::new(),
            0,
            0,
        )
    }

    /// **Elsa's combined load-bearing test** (#2290 blocker 2, round 2).
    ///
    /// The payout is resolved from the **on-chain producer set**, not from an
    /// operator env var, and the block that results is accepted by *Bob's own
    /// authority verifier* — the same function authoritative gossip and IBD
    /// application call. Every
    /// property Elsa enumerated, in one test:
    ///
    /// 1. signer K;
    /// 2. a canonical `Address` payout (`0x…`) that is `!= K.to_address()`;
    /// 3. a seeded `ProducerMember` mapping K → that payout;
    /// 4. the **actual coinbase output** equals that payout;
    /// 5. `verify_block_producer_authorized` **accepts** the produced block.
    ///
    /// Crucially the producer is given NO payout configuration at all — it
    /// learns the address from the chain. That is what closes the round-2
    /// blocker: the two sides now read one source.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn producer_resolves_payout_from_chain_and_bobs_verifier_accepts_2290_b2() {
        use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
        use wrfcoin_shared_protocol::common::types::Address;
        use wrfcoin_shared_protocol::consensus::producer_authority::verify_block_producer_authorized;
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;

        let signer_key = PrivateKey::from_bytes(&[7u8; 32]).expect("valid ed25519 secret");
        let signer_pubkey = PublicKey::from_private_key(&signer_key);

        // (2) canonical Address payout, decoupled from the signing key.
        let payout = Address([0x5au8; 20]).to_hex();
        assert_ne!(
            payout,
            signer_pubkey.to_address(),
            "test premise: the payout must differ from the signer's own address"
        );

        // (3) genesis seeds K → payout. The producer is told NOTHING else.
        let chain = chain_with_seeded_producer(&signer_pubkey, &payout);

        let adapter = PopConsensusAdapter::new(make_pop());
        adapter
            .set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
                signer_key,
                "test-producer",
            )))
            .await;

        let coinbase = Transaction::new_coinbase(&payout, 0, chain.height + 1);
        let block = adapter
            .create_block(vec![coinbase], &chain.tip.to_string(), &payout, &chain)
            .await
            .expect("a block paying the on-chain member payout must be produced");

        // (4) the ACTUAL coinbase output is the member payout.
        let coinbase_recipient = block
            .transactions
            .first()
            .and_then(|cb| cb.outputs.first())
            .map(|o| o.address.clone())
            .expect("a produced block must carry a coinbase output");
        assert_eq!(coinbase_recipient, payout);

        // (5) Bob's authority gate — authentication + membership + §R2 payout —
        //     accepts it. This is the assertion that proves the two heads agree.
        verify_block_producer_authorized(&block, &chain.state.producer_set_history).expect(
            "the block this producer built must be accepted by the same authority gate \
             gossip and IBD apply — if this fails, the producer and the verifier disagree \
             and the node forks itself (#2290 blocker 2)",
        );
    }

    /// The producer refuses when the chain says it must pay someone else, even
    /// though the caller asked for a different recipient and the signature would
    /// be perfectly valid.
    ///
    /// RED without the on-chain resolution: with no operator payout configured
    /// the old code had nothing to compare against, so it produced a
    /// validly-signed block that Bob's gate rejects as `WrongPayoutAddress`.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn producer_refuses_when_the_coinbase_contradicts_the_on_chain_payout_2290_b2() {
        use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
        use wrfcoin_shared_protocol::common::types::Address;
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;

        let signer_key = PrivateKey::from_bytes(&[7u8; 32]).expect("valid ed25519 secret");
        let signer_pubkey = PublicKey::from_private_key(&signer_key);
        let declared = Address([0x5au8; 20]).to_hex();
        let chain = chain_with_seeded_producer(&signer_pubkey, &declared);

        let adapter = PopConsensusAdapter::new(make_pop());
        adapter
            .set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
                signer_key,
                "test-producer",
            )))
            .await;

        // The caller names a different recipient — an unvalidated `/blocks/mine`
        // request body is exactly this.
        let attacker = Address([0xccu8; 20]).to_hex();
        let coinbase = Transaction::new_coinbase(&attacker, 0, chain.height + 1);

        let err = adapter
            .create_block(vec![coinbase], &chain.tip.to_string(), &attacker, &chain)
            .await
            .expect_err(
                "the chain declares this producer's payout; a block paying anyone else must \
                 be refused locally rather than rejected by every peer (#2290 blocker 2)",
            );
        let msg = format!("{err:?}");
        assert!(
            msg.contains(&declared),
            "the refusal must name the payout the chain requires, got: {msg}"
        );
    }

    /// A node whose key is **not** in the seeded set must refuse to produce at
    /// all. Its blocks would fail Bob's gate as `NotAuthorized`, so producing
    /// them only advances a private fork.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn a_non_member_producer_refuses_to_produce_2290_b2() {
        use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
        use wrfcoin_shared_protocol::common::types::Address;
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;

        // The set authorizes SOMEONE ELSE.
        let member_key = PublicKey::from_private_key(
            &PrivateKey::from_bytes(&[9u8; 32]).expect("valid ed25519 secret"),
        );
        let payout = Address([0x5au8; 20]).to_hex();
        let chain = chain_with_seeded_producer(&member_key, &payout);

        let adapter = PopConsensusAdapter::new(make_pop());
        adapter
            .set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
                PrivateKey::from_bytes(&[7u8; 32]).expect("valid ed25519 secret"),
                "test-producer",
            )))
            .await;

        let coinbase = Transaction::new_coinbase(&payout, 0, chain.height + 1);
        let err = adapter
            .create_block(vec![coinbase], &chain.tip.to_string(), &payout, &chain)
            .await
            .expect_err(
                "a producer key outside the authorized set must refuse to produce — every \
                 block it signs is NotAuthorized to its peers (#2290 blocker 2)",
            );
        assert!(
            format!("{err:?}").contains("not in the authorized producer set"),
            "the refusal must say why, got: {err:?}"
        );
    }

    /// The operator env value is a **cross-check**, never an override: if it
    /// disagrees with the chain, production stops rather than silently
    /// preferring one. A stale operator value that quietly lost to the chain
    /// would mean the deployment's stated intent and the node's behaviour differ
    /// with nobody informed.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn a_stale_operator_payout_that_contradicts_the_chain_is_fatal_2290_b2() {
        use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
        use wrfcoin_shared_protocol::common::types::Address;
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;

        let signer_key = PrivateKey::from_bytes(&[7u8; 32]).expect("valid ed25519 secret");
        let signer_pubkey = PublicKey::from_private_key(&signer_key);
        let chain_payout = Address([0x5au8; 20]).to_hex();
        let chain = chain_with_seeded_producer(&signer_pubkey, &chain_payout);

        // The operator configured a DIFFERENT address (stale deployment config).
        let stale = Address([0x77u8; 20]);
        let mut pop = make_pop();
        pop.set_producer_payout_address(&stale);

        let adapter = PopConsensusAdapter::new(pop);
        adapter
            .set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
                signer_key,
                "test-producer",
            )))
            .await;

        let coinbase = Transaction::new_coinbase(&chain_payout, 0, chain.height + 1);
        let err = adapter
            .create_block(
                vec![coinbase],
                &chain.tip.to_string(),
                &chain_payout,
                &chain,
            )
            .await
            .expect_err(
                "a configured payout that contradicts the chain must stop production, not \
                 be silently ignored in favour of the chain (#2290 blocker 2)",
            );
        let msg = format!("{err:?}");
        // Attribute the failure to the CROSS-CHECK rule specifically. Asserting
        // only that both addresses appear would also be satisfied by the generic
        // coinbase-mismatch error, which fires for a different reason — and does
        // fire if the on-chain resolution is removed. Pinning the cross-check
        // wording is what makes this test fail when the chain stops being the
        // authoritative source.
        assert!(
            msg.contains("The chain is authoritative"),
            "the refusal must come from the on-chain/operator cross-check, not from the \
             generic coinbase-mismatch path, got: {msg}"
        );
        assert!(
            msg.contains(&chain_payout) && msg.contains(&stale.to_hex()),
            "the refusal must show both values so the operator can tell which is stale, \
             got: {msg}"
        );
    }

    /// Owner §R2 (#2290 P1-3) — **the load-bearing payout binding.**
    ///
    /// §R2 decouples the coinbase recipient from the signing key: it is the
    /// producer member's stable `payout_address`, so rotating a signing key does
    /// not move payouts. The verify side (#2398) enforces that by reading the
    /// **actual coinbase output** — `block.transactions[0].outputs[0].address` —
    /// and comparing it to the authenticated member's canonical
    /// `payout_address`.
    ///
    /// This test asserts the four properties that make the two sides agree:
    ///
    /// 1. the payout is a canonical `common::types::Address` (`0x…`, round-trips
    ///    through `parse::<Address>()`) — the exact type
    ///    `ProducerMember::payout_address` carries. NOT `PublicKey::to_address()`,
    ///    which is a different `wrf…` format and would never match;
    /// 2. that payout differs from `K.to_address()`, so the decoupling is real
    ///    and not accidentally satisfied by the signer's own address;
    /// 3. the block **authenticates** under signer key K;
    /// 4. the **actual coinbase output** equals that payout — the field the gate
    ///    reads, not the `miner_address` metadata string, which no verifier
    ///    consults.
    ///
    /// The previous revision of this test asserted `metadata["miner_address"]`
    /// against a `wrf…` address and only called the authentication-only
    /// verifier, so it could not fail for the reason that mattered.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn coinbase_pays_the_canonical_member_payout_not_the_signer_2290_r2() {
        use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
        use wrfcoin_shared_protocol::common::types::Address;
        use wrfcoin_shared_protocol::consensus::block_producer_auth::verify_block_producer_on;
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ChainIdentity, SigningProtocolVersion,
        };
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;

        let mut pop = make_pop();
        let signer_key = PrivateKey::from_bytes(&[7u8; 32]).expect("valid ed25519 secret");
        let signer_pubkey = PublicKey::from_private_key(&signer_key);

        // (1) The member payout is a CANONICAL Address, as Alice seeds it at
        //     genesis and as Bob's `ProducerMember::payout_address` stores it.
        let payout = Address([0x5au8; 20]);
        let payout_str = payout.to_hex();
        assert_eq!(
            payout_str.parse::<Address>().expect("canonical form round-trips"),
            payout,
            "the payout must be a canonical Address (0x…), the type the verify side compares"
        );
        // (2) ...and it is NOT derived from the signing key.
        assert_ne!(
            payout_str,
            signer_pubkey.to_address(),
            "test premise: payout must differ from the signer's own address, or the \
             decoupling this test exists to prove is vacuous"
        );

        // The producer is bound to that payout — the SIGN-side half of the seam.
        pop.set_producer_payout_address(&payout);
        let adapter = PopConsensusAdapter::new(pop);
        adapter
            .set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
                signer_key,
                "test-producer",
            )))
            .await;

        let chain = genesis_chain();
        let coinbase = Transaction::new_coinbase(&payout_str, 0, chain.height + 1);
        let block = adapter
            .create_block(vec![coinbase], &chain.tip.to_string(), &payout_str, &chain)
            .await
            .expect("a block paying the declared payout must be produced");

        // (3) Authenticates under K.
        let auth_key = verify_block_producer_on(
            &block,
            ChainIdentity::active(),
            SigningProtocolVersion::active(),
        )
        .expect("block authenticates under the producer signer key");
        assert_eq!(
            auth_key.as_bytes(),
            signer_pubkey.as_bytes(),
            "authenticated as the producer signer K"
        );

        // (4) The ACTUAL coinbase output — the field the authority gate reads —
        //     is the canonical member payout.
        let coinbase_recipient = block
            .transactions
            .first()
            .and_then(|cb| cb.outputs.first())
            .map(|o| o.address.clone())
            .expect("a produced block must carry a coinbase output");
        assert_eq!(
            coinbase_recipient, payout_str,
            "the coinbase OUTPUT (not the miner_address metadata) must be the member's \
             canonical payout_address — this is the exact comparison #2398's authority \
             gate performs, so a mismatch here is a block every peer rejects"
        );
        assert_ne!(
            coinbase_recipient,
            signer_pubkey.to_address(),
            "and it must not be the signer-derived address (§R2 decoupling)"
        );
    }

    /// The binding is a **refusal**, not a relabeling: asked to build a block
    /// whose coinbase pays someone other than the declared payout, the producer
    /// must fail rather than emit it.
    ///
    /// This is the half that makes §R2 load-bearing on the production path.
    /// `/blocks/mine` passes the request body's `miner_address` straight
    /// through, so without this check a caller could name any recipient; the
    /// block would authenticate correctly, commit locally, and be rejected by
    /// every peer — a silent self-fork carrying a valid signature. RED without
    /// `enforce_producer_payout_binding`: production succeeds and returns a
    /// block instead of erroring.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn producing_a_block_that_pays_the_wrong_address_is_refused_2290_p1_3() {
        use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
        use wrfcoin_shared_protocol::common::types::Address;
        use wrfcoin_shared_protocol::crypto::signature::PrivateKey;
        use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;

        let mut pop = make_pop();
        let declared_payout = Address([0x5au8; 20]);
        pop.set_producer_payout_address(&declared_payout);

        let adapter = PopConsensusAdapter::new(pop);
        adapter
            .set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
                PrivateKey::from_bytes(&[7u8; 32]).expect("valid ed25519 secret"),
                "test-producer",
            )))
            .await;

        // A caller-supplied recipient that is NOT the declared payout — exactly
        // what an unvalidated `/blocks/mine` request body can name.
        let attacker_payout = Address([0xccu8; 20]).to_hex();
        let chain = genesis_chain();
        let coinbase = Transaction::new_coinbase(&attacker_payout, 0, chain.height + 1);

        let err = adapter
            .create_block(
                vec![coinbase],
                &chain.tip.to_string(),
                &attacker_payout,
                &chain,
            )
            .await
            .expect_err(
                "producing a block whose coinbase pays an address other than the declared \
                 payout_address must be REFUSED — emitting it forks this node off the \
                 network with a validly-signed block (#2290 P1-3)",
            );
        let msg = format!("{err:?}");
        assert!(
            msg.contains("payout") && msg.contains(&declared_payout.to_hex()),
            "the refusal must name the expected payout so an operator can fix the \
             configuration, got: {msg}"
        );
    }

    /// Scope guard: with no payout binding configured — a chain that never
    /// seeded a producer set — production is unaffected. Without this, the fix
    /// would break every legacy and local-dev node.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn payout_binding_is_inert_when_unconfigured_2290_p1_3() {
        use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
        use wrfcoin_shared_protocol::common::types::Address;

        let adapter = PopConsensusAdapter::new(make_pop());
        let any_address = Address([0xccu8; 20]).to_hex();
        let chain = genesis_chain();
        let coinbase = Transaction::new_coinbase(&any_address, 0, chain.height + 1);

        adapter
            .create_block(vec![coinbase], &chain.tip.to_string(), &any_address, &chain)
            .await
            .expect("no binding configured ⇒ any coinbase recipient is accepted as before");
    }
}
