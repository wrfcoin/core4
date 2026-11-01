use crate::blockchain::transaction::{Transaction, TransactionType};
use crate::common::constants::MEMPOOL_CAPACITY;
use crate::config::settings::Config;
use crate::consensus::partition_policy::PartitionMode;
use crate::crypto::hash::Hash;
use crate::crypto::signature::Signature;
use crate::monitoring::{OrderingMempoolEntry, OrderingMempoolSnapshot};
use crate::security::key_registry::{SigningKeyRegistry, SigningRole};
use anyhow::{Result, anyhow};
use log::{debug, warn};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cmp::Reverse;
use std::collections::{BTreeMap, VecDeque};

use crate::accounts::abstraction::UserOperation;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_COMMIT_REVEAL_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_ORACLE_ATTESTATION_PAYLOAD_BYTES: usize = 32 * 1024;

fn is_all_zero_signature_hex(signature_hex: &str) -> bool {
    Signature::from_hex(signature_hex)
        .map(|signature| signature.as_bytes().iter().all(|byte| *byte == 0))
        .unwrap_or(false)
}

/// Transaction priority for mempool ordering
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TxPriority {
    /// High priority transactions
    High,

    /// Medium priority transactions
    Medium,

    /// Low priority transactions
    Low,
}

/// In-memory pool of unconfirmed transactions
///
/// `Clone` is derived so a caller can stage speculative work (e.g. building a
/// candidate block) against a detached copy and only apply the resulting
/// removals to the live pool once the candidate has passed every gate — see
/// `/blocks/mine` in `launch_api` (#2290). Every field is a plain owned
/// collection or `Copy` scalar, so the clone is a genuine deep copy: mutating
/// the clone cannot be observed through the original. The only shared handles
/// are the `Arc<Transaction>` values, which are immutable once admitted.
#[derive(Clone)]
pub struct Mempool {
    /// Unconfirmed transactions by ID — Arc avoids deep clones on retrieval
    transactions: FxHashMap<Hash, Arc<Transaction>>,

    /// Fee-rate ordered index: BTreeMap<(Reverse(fee_rate), Reverse(abs_fee), txid), ()>
    /// Primary key: descending fee_rate; secondary: descending absolute fee (breaks ties caused
    /// by integer-division rounding so higher-fee txs always precede lower-fee txs at the same
    /// fee_rate); tertiary: txid (deterministic ordering for equal fee and fee_rate).
    fee_order: BTreeMap<(Reverse<u64>, Reverse<u64>, Hash), ()>,

    /// Per-transaction sort-key cache — (fee_rate, abs_fee) needed to reconstruct the BTreeMap
    /// key for O(log n) removal.
    tx_fee_rates: FxHashMap<Hash, (u64, u64)>,

    /// Transaction priority overrides
    priorities: FxHashMap<Hash, TxPriority>,

    /// Set of transactions that depend on other transactions
    dependencies: FxHashMap<Hash, FxHashSet<Hash>>, // txid -> set of txids it depends on

    /// Reverse dependency index: txid -> set of txids that depend on it.
    ///
    /// This is the exact inverse of `dependencies` and exists purely to make
    /// removal O(d) instead of O(m): when a transaction is removed it must be
    /// dropped from every dependency set that referenced it. Scanning all of
    /// `dependencies.values_mut()` per removal made eviction O(m×n)
    /// (#1976 — 10k mempool × 100 evictions = 1M ops/block, DoS-able); this
    /// index lets us touch only the actual dependents. It is an internal
    /// performance structure: never serialized, never on the wire, and it does
    /// not change which transactions are evicted or their ordering.
    dependents: FxHashMap<Hash, FxHashSet<Hash>>, // txid -> set of txids that depend on it

    /// Set of double-spend attempts (transactions spending the same input)
    double_spends: FxHashMap<(Hash, u32), Vec<Hash>>, // (prev_txid, prev_vout) -> vec of txids spending it

    /// When transactions were added to mempool
    timestamps: FxHashMap<Hash, Instant>,

    /// Maximum time to keep a transaction in mempool
    max_age: Duration,

    /// Maximum size of the mempool in bytes
    max_size: usize,

    /// Current size of the mempool in bytes
    current_size: usize,

    /// Configuration
    config: Config,

    /// Replay keys for launch-critical protocol payloads.
    protocol_replay_keys: FxHashSet<String>,

    /// Per-sender unconfirmed-transaction count (#2039 anti-spam), keyed on the
    /// sender address hex. Maintained in lock-step with `transactions`:
    /// incremented on admission, decremented on every removal path. Used to
    /// enforce `config.mempool_max_txs_per_sender` at admission so one funded
    /// key cannot monopolize the pool.
    per_sender_count: FxHashMap<String, usize>,

    /// #2168 user-operation lane: account-abstraction operations, held
    /// SEPARATELY from `transactions` and bounded independently.
    ///
    /// Two pools rather than one because the story's requirement is mutual
    /// non-starvation: user operations must not be able to fill the byte
    /// budget ordinary transactions rely on, and ordinary transactions must
    /// not be able to crowd user operations out. Sharing either the map or
    /// the byte counter would make one of those impossible.
    user_operations: FxHashMap<[u8; 32], UserOperation>,

    /// Admission order for the user-op lane, so eviction is oldest-first and
    /// deterministic rather than hash-map order.
    user_op_order: VecDeque<[u8; 32]>,

    /// Independent capacity bound for the user-op lane, in operations.
    user_op_capacity: usize,

    /// Current network-partition operating mode (#1217).
    ///
    /// Defaults to [`PartitionMode::Normal`]. When the partition
    /// orchestrator detects the local node sits in a minority partition
    /// it calls [`Mempool::set_partition_mode`] with
    /// [`PartitionMode::ReadOnly`] and [`Self::add_transaction`] begins
    /// rejecting all new writes. This is the mempool-side enforcement of
    /// the #1217 minority-safety rule defined in
    /// [`crate::consensus::partition_policy`].
    partition_mode: PartitionMode,
}

/// Default capacity of the #2168 user-operation lane, in operations.
///
/// Deliberately a COUNT, not a byte budget: the transaction pool's bound is in
/// bytes, and sharing that unit would let one lane consume the other's
/// headroom — the starvation the story rules out. 4096 is roughly two blocks'
/// worth of operations at any plausible per-block count, so an honest bundler
/// is never throttled while a flooder is.
pub const DEFAULT_USER_OP_LANE_CAPACITY: usize = 4096;

impl Mempool {
    // ---------------------------------------------------------------
    // #2168 user-operation lane.
    // ---------------------------------------------------------------

    /// Admit a user operation, keyed by its domain-separated hash.
    ///
    /// Returns `false` when the operation is already present (replay within
    /// the pool) — never an error, because a duplicate arrival is ordinary
    /// gossip, not a fault. At capacity the OLDEST operation is evicted, so a
    /// full lane still accepts new work rather than going deaf.
    pub fn add_user_operation(&mut self, hash: [u8; 32], op: UserOperation) -> bool {
        if self.user_operations.contains_key(&hash) {
            return false;
        }
        while self.user_operations.len() >= self.user_op_capacity {
            match self.user_op_order.pop_front() {
                Some(oldest) => {
                    self.user_operations.remove(&oldest);
                },
                // Order and map disagree: drop the whole lane rather than
                // spin. Unreachable while both are maintained together.
                None => {
                    self.user_operations.clear();
                    break;
                },
            }
        }
        self.user_operations.insert(hash, op);
        self.user_op_order.push_back(hash);
        true
    }

    /// Remove a user operation (it was included, or expired).
    pub fn remove_user_operation(&mut self, hash: &[u8; 32]) -> Option<UserOperation> {
        let removed = self.user_operations.remove(hash);
        if removed.is_some() {
            self.user_op_order.retain(|queued| queued != hash);
        }
        removed
    }

    pub fn user_operation_count(&self) -> usize {
        self.user_operations.len()
    }

    pub fn user_op_capacity(&self) -> usize {
        self.user_op_capacity
    }

    /// Override the lane bound. A capacity of zero disables the lane.
    pub fn set_user_op_capacity(&mut self, capacity: usize) {
        self.user_op_capacity = capacity;
        while self.user_operations.len() > self.user_op_capacity {
            match self.user_op_order.pop_front() {
                Some(oldest) => {
                    self.user_operations.remove(&oldest);
                },
                None => {
                    self.user_operations.clear();
                    break;
                },
            }
        }
    }

    pub fn contains_user_operation(&self, hash: &[u8; 32]) -> bool {
        self.user_operations.contains_key(hash)
    }

    /// Up to `limit` operations in admission order, for block building.
    pub fn user_operations_for_block(&self, limit: usize) -> Vec<(&[u8; 32], &UserOperation)> {
        self.user_op_order
            .iter()
            .take(limit)
            .filter_map(|hash| self.user_operations.get_key_value(hash))
            .collect()
    }

    /// Creates a new mempool.
    ///
    /// `max_age` and `max_size` are read from `Config::mempool_max_age_secs`
    /// and `Config::mempool_max_size_bytes` (defaults: 4 h / 50 MB).
    pub fn new(config: &Config) -> Self {
        Self {
            transactions: FxHashMap::default(),
            user_operations: FxHashMap::default(),
            user_op_order: VecDeque::new(),
            user_op_capacity: DEFAULT_USER_OP_LANE_CAPACITY,
            fee_order: BTreeMap::new(),
            tx_fee_rates: FxHashMap::default(),
            priorities: FxHashMap::default(),
            dependencies: FxHashMap::default(),
            dependents: FxHashMap::default(),
            double_spends: FxHashMap::default(),
            timestamps: FxHashMap::default(),
            max_age: Duration::from_secs(config.mempool_max_age_secs),
            max_size: config.mempool_max_size_bytes,
            current_size: 0,
            config: config.clone(),
            protocol_replay_keys: FxHashSet::default(),
            per_sender_count: FxHashMap::default(),
            partition_mode: PartitionMode::default(),
        }
    }

    /// Sender key for the per-sender admission cap (#2039). Uses the hex sender
    /// address; a single, central definition so admission and every removal
    /// path agree on the key.
    fn sender_key(tx: &Transaction) -> String {
        tx.sender_address.to_hex()
    }

    /// The mempool's **single** definition of "fee rate": milli-units of fee per
    /// byte, `fee * 1000 / size`.
    ///
    /// The ×1000 scale preserves the per-byte signal that a raw `fee / size`
    /// integer-divides to 0 for any fee below the tx size (#2026). Both the
    /// fee-order sort key (at insertion) AND the #2039 minimum-fee floor (at
    /// admission) call this one function, so a configured `mempool_min_fee_rate`
    /// is expressed in the same units the pool orders by.
    ///
    /// Before #2061 the floor computed an unscaled `fee / size` while the sort
    /// used milli-units, so once #2060 landed a configured floor was interpreted
    /// 1000× off relative to ordering — a below-ordering-priority tx could clear
    /// the floor, or vice versa. Sharing this helper is what keeps them in step.
    ///
    /// `saturating_mul` guards the (astronomical) `fee * 1000` overflow; `.max(1)`
    /// guards a zero tx size.
    pub(crate) fn fee_rate_milli(fee: u64, tx_size: usize) -> u64 {
        const FEE_RATE_MILLI_SCALE: u64 = 1000;
        fee.saturating_mul(FEE_RATE_MILLI_SCALE) / (tx_size as u64).max(1)
    }

    /// Decrement a sender's unconfirmed-tx count (#2039), removing the entry at
    /// zero so the map does not grow unbounded. Called from every removal path
    /// in lock-step with `transactions.remove` to keep the count exact.
    fn decr_sender(&mut self, sender_key: &str) {
        if let Some(count) = self.per_sender_count.get_mut(sender_key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.per_sender_count.remove(sender_key);
            }
        }
    }

    /// Current partition operating mode (#1217).
    pub fn partition_mode(&self) -> PartitionMode {
        self.partition_mode
    }

    /// Update the partition operating mode.
    ///
    /// Called by the partition orchestrator when the local node's stake
    /// fraction crosses a quorum boundary (detection happens in
    /// `consensus-engines::coordination::adaptive_fault_tolerance`).
    /// Transitions are logged so operators can audit when the mempool
    /// entered or left ReadOnly during an incident.
    pub fn set_partition_mode(&mut self, mode: PartitionMode) {
        if self.partition_mode != mode {
            warn!(
                "mempool partition mode transition: {} → {}",
                self.partition_mode.label(),
                mode.label()
            );
        }
        self.partition_mode = mode;
    }

    /// Cheap admission policy shared by HTTP ingress and the mempool. No
    /// signatures or pool mutations occur here; configuration belongs to this pool.
    pub fn check_transaction_admission_cost(&self, tx: &Transaction) -> Result<usize> {
        // Check transaction size
        let tx_size = tx.size();
        if tx_size > self.config.max_transaction_size {
            return Err(anyhow!("Transaction exceeds maximum size"));
        }

        // General admission pricing, shared by all kinds. The default zero
        // unit price preserves existing behavior; this never debits balances.
        let minimum_verify_fee = crate::consensus::verify_cost::minimum_fee_for(
            tx, &crate::consensus::verify_cost::VerifyPricing::default(), self.config.mempool_verify_fee_per_unit);
        if tx.fee < minimum_verify_fee { return Err(anyhow!("transaction fee below verification-work admission minimum")); }
        // #2039: minimum-fee floor. Reject below-floor transactions before any
        // heavier validation or mempool mutation. The fee_rate here uses the
        // SAME `fee_rate_milli` helper as the sort key below (#2061) — the floor
        // and the ordering must agree on units, or a configured floor is read
        // 1000× off from ordering once #2060's milli-unit sort landed. Disabled
        // by default (`mempool_min_fee_rate == 0`) so testnet faucet-funded
        // zero-fee earn transactions are still admitted; raise operationally
        // once faucet output is coupled to a fee. The configured value is in
        // milli-units of fee per byte.
        if self.config.mempool_min_fee_rate > 0 {
            let fee_rate = Self::fee_rate_milli(tx.fee, tx_size);
            if fee_rate < self.config.mempool_min_fee_rate {
                return Err(anyhow!(
                    "transaction rejected: fee rate {fee_rate} below minimum {}",
                    self.config.mempool_min_fee_rate
                ));
            }
        }

        Ok(tx_size)
    }

    /// Adds a transaction to the mempool
    pub fn add_transaction(&mut self, tx: Transaction) -> Result<()> {
        self.add_transaction_with_signing_keys(tx, None)
    }

    /// Adds a transaction to the mempool with access to chain-managed signing keys.
    pub fn add_transaction_with_signing_keys(
        &mut self,
        tx: Transaction,
        signing_keys: Option<&SigningKeyRegistry>,
    ) -> Result<()> {
        self.add_transaction_with_signing_keys_at_height(tx, signing_keys, None)
    }

    /// Node bond admission needs a canonical height; legacy callers retain
    /// exactly their existing admission behavior and refuse Node intents.
    pub fn add_transaction_with_signing_keys_at_height(
        &mut self, tx: Transaction, signing_keys: Option<&SigningKeyRegistry>, current_height: Option<u64>,
    ) -> Result<()> {
        // Partition-mode gate (#1217). A node in a minority partition
        // must refuse new writes because any block it produces with
        // those transactions cannot reach BFT finality and will be
        // reverted on reconnection. Applied before any other validation
        // so minority nodes do zero work on rejected writes.
        //
        // The gate is uniform across callers: HTTP submission, P2P
        // relay, layer-2 rollup/operator paths, and PoP reward
        // ingestion all land here and all are refused identically.
        if !self.partition_mode.accepts_mempool_writes() {
            return Err(anyhow!(
                "Mempool is read-only: node is in minority partition (mode={})",
                self.partition_mode.label()
            ));
        }

        // Ensure transaction has an ID
        let tx_id = tx.id.ok_or_else(|| anyhow!("Transaction has no ID"))?;

        // Check if transaction is already in mempool
        if self.transactions.contains_key(&tx_id) {
            return Err(anyhow!("Transaction already in mempool"));
        }

        // Basic validation
        if !tx.validate_structure() {
            return Err(anyhow!("Invalid transaction structure"));
        }

        // #1762: shielded deposit/withdrawal are disabled at the consensus
        // layer. The shielded pool verifies no value-backing / ownership proof
        // (withdrawal credits an arbitrary recipient an arbitrary amount with
        // `authorization_proof` unverified — an unbacked-mint vector), so the
        // surface is rejected at admission and in `verify_block` until a proper
        // commitment<->amount<->owner proof lands (#1762 follow-up).
        if matches!(
            tx.tx_type,
            TransactionType::ShieldedDeposit | TransactionType::ShieldedWithdrawal
        ) {
            return Err(anyhow!(
                "shielded deposit/withdrawal are disabled pending value-backing proof verification (#1762)"
            ));
        }

        // Node policy only (Refs #3407, #3408, #3430). Historical blocks retain
        // their semantics. B3 cannot verify confidential range proofs; signed
        // staking also lacks persistent nonce and producer/peer agreement.
        if super::local_admission_disabled(tx.tx_type) {
            return Err(anyhow!(
                "{} is disabled for local admission pending safe authorization \
                 and verifier readiness (#3407, #3408, #3430)",
                tx.tx_type
            ));
        }

        // #2024: `GovernanceProposal` is disabled at the consensus layer.
        //
        // The arm in `apply_protocol_transaction` verifies NO signature. It
        // forwards `proposer: tx.sender_address` -- a plain, unauthenticated
        // struct field -- into `CreateGovernanceProposal`, which debits that
        // account by `deposit`. Nothing upstream binds that field for this type:
        // the admission and `verify_block` signature gates are both scoped to
        // `Transfer`, `validate_structure` checks shape only, and the outpoint
        // index is mempool-local dedup that never checks that an input exists or
        // is owned. So a proposal signed by nobody moves a victim's funds.
        //
        // This fence is deliberately the #1762 shape: reject the surface at BOTH
        // boundaries (here and in `verify_block`) so producer, P2P applier and
        // WAL replay reach the same verdict and cannot fork.
        //
        // WHAT THIS FENCE DOES NOT DO: it does not authorize anything. It is not
        // an authorization model and must not be read as one. A from-bound
        // authorization (the `verify_transparent_transfer_authorization` shape)
        // is still required before this surface can be re-enabled; this only
        // ensures the unauthenticated surface is unreachable in the meantime.
        if tx.tx_type == TransactionType::GovernanceProposal {
            return Err(anyhow!(
                "GovernanceProposal is disabled pending from-bound proposer authorization \
                 (#2024): the proposer field is unauthenticated and the deposit debits it"
            ));
        }

        // #3565: NO per-station admission rule. The mempool cannot tell an
        // owner's reveal from anyone else's (a reveal carries no signature and
        // the mempool has no station registry), so refusing "a second pending
        // reveal for station S" let any party block S's real reveal by
        // submitting a junk one first. One-per-block is enforced where the
        // owner binding can be checked: `classify_block_transaction` refuses a
        // block with two, `State` refuses a commit the owner did not sign, and
        // the producer defers extra reveals to later blocks instead of evicting
        // them. Junk reveals with no valid commit are evicted at assembly, as
        // on main.

        let tx_size = self.check_transaction_admission_cost(&tx)?;

        // Admission checks authorization only. Provability (`responsible()`), the challenge deadline and duplicates
        // are enforced by `node_fraud::apply`; the operator route runs them as an advisory dry-run at the edge.
        if tx.tx_type == TransactionType::NodeFraudReport {
            let height = current_height.ok_or_else(|| anyhow!("SP_NODE_FRAUD_HEIGHT_REQUIRED"))?;
            if !crate::crypto::chain_binding::feature_active(crate::crypto::chain_binding::ConsensusFeature::NodeShareSettlement, height) { return Err(anyhow!("SP_NODE_FRAUD_INACTIVE")); }
            let p = crate::consensus::node_fraud::parse(&tx).ok_or_else(|| anyhow!("SP_NODE_FRAUD_PAYLOAD"))?;
            let c = crate::governance::schedule_extension::schedule_extension_authority().ok_or_else(|| anyhow!("SP_NODE_FRAUD_AUTHORITY"))?.context();
            let genesis: [u8;32] = hex::decode(c.genesis_hash)?.try_into().map_err(|_| anyhow!("SP_NODE_FRAUD_GENESIS"))?;
            crate::consensus::node_fraud::verify_authorization(&p, &crate::crypto::chain_binding::ChainIdentity::new(c.chain_id,c.network_id), &genesis).map_err(|_| anyhow!("SP_NODE_FRAUD_AUTHORIZATION"))?;
        }
        if tx.tx_type == TransactionType::NodeServiceBatch {
            use crate::consensus::node_receipt_registry::NodeReceiptSubmission;
            let height = current_height.ok_or_else(|| anyhow!("SP_NODE_RECEIPT_HEIGHT_REQUIRED"))?;
            if !crate::crypto::chain_binding::feature_active(crate::crypto::chain_binding::ConsensusFeature::NodeShareSettlement, height) { return Err(anyhow!("SP_NODE_RECEIPT_INACTIVE")); }
            let p = NodeReceiptSubmission::parse(&tx).ok_or_else(|| anyhow!("SP_NODE_RECEIPT_PAYLOAD"))?;
            let c = crate::governance::schedule_extension::schedule_extension_authority().ok_or_else(|| anyhow!("SP_NODE_RECEIPT_AUTHORITY"))?.context();
            let genesis: [u8;32] = hex::decode(c.genesis_hash)?.try_into().map_err(|_| anyhow!("SP_NODE_RECEIPT_GENESIS"))?;
            p.verify_authorization(&crate::crypto::chain_binding::ChainIdentity::new(c.chain_id,c.network_id),&genesis).map_err(|_| anyhow!("SP_NODE_RECEIPT_AUTHORIZATION"))?;
        }
        if tx.tx_type == TransactionType::NodeBondLifecycle {
            use crate::consensus::node_bond_registry::NodeBondLifecycle;
            if !crate::crypto::chain_binding::feature_active(crate::crypto::chain_binding::ConsensusFeature::NodeShareSettlement, current_height.ok_or_else(|| anyhow!("SP_NODE_BOND_HEIGHT_REQUIRED"))?) {
                return Err(anyhow!("SP_NODE_BOND_INACTIVE"));
            }
            let p = NodeBondLifecycle::parse(&tx).ok_or_else(|| anyhow!("SP_NODE_BOND_PAYLOAD"))?;
            let c = crate::governance::schedule_extension::schedule_extension_authority().ok_or_else(|| anyhow!("SP_NODE_BOND_AUTHORITY"))?.context();
            let genesis: [u8; 32] = hex::decode(c.genesis_hash)?.try_into().map_err(|_| anyhow!("SP_NODE_BOND_GENESIS"))?;
            p.verify(&crate::crypto::chain_binding::ChainIdentity::new(c.chain_id, c.network_id), &genesis, c.allowlist).map_err(|_| anyhow!("SP_NODE_BOND_AUTHORIZATION"))?;
        }
        if tx.tx_type == TransactionType::SessionKeyAction {
            use crate::accounts::session_keys::{
                SessionKeyActionPayload, session_keys_active, verify_stateless,
            };
            let height = current_height.ok_or_else(|| anyhow!("SP_SESSION_KEYS_HEIGHT_REQUIRED"))?;
            // Admitted for the block it would be mined in (tip + 1).
            if !session_keys_active(height.saturating_add(1)) {
                return Err(anyhow!("SP_SESSION_KEYS_INACTIVE"));
            }
            let action = SessionKeyActionPayload::parse(&tx)
                .ok_or_else(|| anyhow!("SP_SESSION_KEYS_PAYLOAD"))?;
            let chain_id = &crate::crypto::chain_binding::ChainIdentity::active().chain_id;
            verify_stateless(&action, chain_id)
                .map_err(|e| anyhow!("SP_SESSION_KEYS_AUTHORIZATION: {e}"))?;
        }
        if tx.tx_type == TransactionType::SlashEvidenceReport {
            return Err(anyhow!("SP_SLASH_EVIDENCE_NOT_SUPPORTED"));
        }

        // #3277 S4: live feature-schedule extensions. ADMISSION POLICY, not
        // validity: a block carrying an extension this refuses is still valid
        // (design §1.3). Refused here: an unparsable payload, and any
        // extension naming a feature this binary does not implement or does
        // not mark schedulable (`check_admission_policy`). The state-free
        // rules (chain and genesis binding, shape, signatures) are checked
        // against this node's authority; a node without one can apply no
        // extension, so it admits none. The state-dependent rules are checked
        // when the producer stages the tx (`prune_invalid_launch_transactions`
        // runs the fail-closed apply arm).
        if tx.tx_type == TransactionType::FeatureScheduleExtension {
            use crate::governance::schedule_extension as ext;
            let signed = tx.parse_feature_schedule_extension().ok_or_else(|| {
                anyhow!("feature schedule extension rejected: unparsable payload (#3277)")
            })?;
            // #3361 F3: the faucet binding is admitted only on the chain
            // instance its row names — this node's block 1, via its authority.
            let node_genesis = ext::schedule_extension_authority().map(|a| a.context().genesis_hash);
            ext::check_admission_policy(&signed.extension, node_genesis).map_err(|e| {
                anyhow!("feature schedule extension rejected by admission policy (#3277): {e}")
            })?;
            let authority = ext::schedule_extension_authority().ok_or_else(|| {
                anyhow!(
                    "feature schedule extension rejected: this node has no schedule-extension \
                     authority installed (#3277)"
                )
            })?;
            ext::verify_feature_schedule_extension(&signed, &authority.context())
                .map_err(|e| anyhow!("feature schedule extension rejected (#3277): {e}"))?;
        }

        // #2700: total-occupancy cap. `MEMPOOL_CAPACITY` is declared in
        // `common/constants.rs` and cited by docs/security/PROTOCOL_SPEC.md as a
        // protocol bound, but until now no code consumed it — the pool was bounded
        // in BYTES (`mempool_max_size_bytes`, checked at insert) and per SENDER
        // (below), and never in total transaction COUNT. A flood of many small
        // transactions from many senders could therefore exceed the advertised
        // 50_000-entry ceiling without ever tripping either existing bound.
        //
        // Checked before any mutation, alongside the other admission caps, so a
        // rejected transaction leaves no trace in the pool.
        if self.transactions.len() >= MEMPOOL_CAPACITY {
            return Err(anyhow!(
                "transaction rejected: mempool is at capacity ({MEMPOOL_CAPACITY} transactions)"
            ));
        }

        // #2039: per-sender admission cap. Bounds how much of the pool a single
        // (spoofable) sender key can occupy so one funded key cannot monopolize
        // the mempool. Checked before any mutation; the counter is incremented
        // only after the transaction is actually stored, and decremented on
        // every removal path, so a rejected tx never leaks a slot.
        let sender_key = Self::sender_key(&tx);
        if self.per_sender_count.get(&sender_key).copied().unwrap_or(0)
            >= self.config.mempool_max_txs_per_sender
        {
            return Err(anyhow!(
                "transaction rejected: sender has reached the mempool per-sender limit ({})",
                self.config.mempool_max_txs_per_sender
            ));
        }

        // Commit/reveal payloads are constrained separately from full transaction size.
        if matches!(
            tx.tx_type,
            TransactionType::ContributionCommit
                | TransactionType::ContributionReveal
                | TransactionType::ForecastCommit
                | TransactionType::ForecastReveal
        ) {
            let payload_size = tx.data.as_ref().map(|d| d.len()).unwrap_or(0);
            if payload_size > MAX_COMMIT_REVEAL_PAYLOAD_BYTES {
                return Err(anyhow!(
                    "Commit/reveal payload exceeds {MAX_COMMIT_REVEAL_PAYLOAD_BYTES} bytes"
                ));
            }
        }
        if tx.tx_type == TransactionType::OracleAttestation {
            let payload_size = tx.data.as_ref().map(|d| d.len()).unwrap_or(0);
            if payload_size > MAX_ORACLE_ATTESTATION_PAYLOAD_BYTES {
                return Err(anyhow!(
                    "Oracle attestation payload exceeds {MAX_ORACLE_ATTESTATION_PAYLOAD_BYTES} bytes"
                ));
            }
        }

        // #1727 defense-in-depth: refuse unsigned/forged transparent transfers
        // at admission so the producer never packs a block its peers reject. A
        // transparent transfer's authorization is the `from`-bound signature
        // embedded in its payload (see `verify_transparent_transfer_authorization`),
        // not its per-input signatures — which never bind to `from`.
        //
        // #2169: on a chain that schedules `passkey_p256_auth` (in genesis or
        // through an applied governance schedule extension), a WebAuthn P-256
        // passkey assertion bound to `from` is also admitted;
        // the exact activation height is enforced by the producer self-screen,
        // block validation and state apply, which know their height.
        if tx.has_transparent_transfer_payload()
            && !tx.verify_transparent_transfer_authorization_for_admission()
        {
            return Err(anyhow!(
                "transparent transfer rejected: missing or invalid from-account authorization signature"
            ));
        }

        // #2024: a non-transparent `Transfer` is authorized by its per-input
        // ed25519 signatures. Block validation verifies these
        // (`classify_block_transaction` in proof_of_publishing.rs, gated on
        // `tx_type == Transfer`), but admission did not — so a Transfer carrying
        // garbage or absent input signatures was admitted and gossiped, only to
        // be rejected at block time. Mirror the block-time gate here (the
        // transparent branch is already covered above) so admission and block
        // validation agree. Scoped to Transfer deliberately: every other tx type
        // is either coinbase/mint, payload/ZK/ring/registry-authed, or inert in
        // the state machine — applying `verify_input_signatures` to them would
        // wrongly reject legitimate txs that carry no per-input signatures.
        if tx.tx_type == TransactionType::Transfer
            && !tx.is_coinbase()
            && !tx.has_transparent_transfer_payload()
        {
            if let Err(reason) = tx.verify_input_signatures() {
                return Err(anyhow!("transfer rejected: invalid input signature: {reason}"));
            }
        }

        // #1763 defense-in-depth: refuse unauthorized Stake/Unstake at admission
        // so the producer never packs a block its peers reject. A staking
        // mutation's authorization is the `account`-bound signature embedded in
        // its payload (`verify_stake_authorization`); pre-#1763 it had none and
        // anyone could lock/release any account's balance from a spoofed
        // `sender_address`. Fail-closed: a Stake/Unstake without a valid,
        // action- and nonce-bound authorization is rejected.
        if matches!(tx.tx_type, TransactionType::Stake | TransactionType::Unstake)
            && !tx.verify_stake_authorization()
        {
            return Err(anyhow!(
                "stake transaction rejected: missing or invalid account authorization signature"
            ));
        }

        let protocol_replay_key = self.validate_launch_payload_and_replay_key(&tx, signing_keys)?;
        // #3290: once the forecaster key binding is in force at the candidate
        // inclusion height (the block after the canonical tip), refuse a
        // forecast commit whose sender is not the address of its
        // authorization key, whether it came from the route or from gossip.
        // With no height, require the binding whenever the feature is
        // scheduled at all: a bound commit is valid under both rules, so this
        // only ever refuses commits that would be invalid from activation.
        // Block validity is `verify_commit_authorization_at_height` in the
        // shared block classifier; this is the admission mirror of it.
        if tx.tx_type == TransactionType::ForecastCommit {
            use crate::crypto::chain_binding::{ConsensusFeature, feature_active, feature_scheduled};
            let binding_required = match current_height {
                Some(height) => {
                    feature_active(ConsensusFeature::ForecasterKeyBinding, height.saturating_add(1))
                },
                None => feature_scheduled(ConsensusFeature::ForecasterKeyBinding),
            };
            if binding_required && !tx.forecast_commit_key_bound() {
                return Err(anyhow!(
                    "SP_FORECASTER_KEY_UNBOUND: forecast commit sender must be the address of its auth_pubkey (#3290)"
                ));
            }
        }
        if let Some(key) = &protocol_replay_key {
            if self.protocol_replay_keys.contains(key) {
                return Err(anyhow!("Launch protocol replay detected"));
            }
        }

        // Check for double spends within the mempool.
        //
        // Two-pass: verify ALL inputs before mutating `double_spends`. A
        // single-pass loop that writes to `double_spends` before completing
        // all checks leaves stale entries when a later input triggers a
        // double-spend rejection — the already-visited outpoints end up in
        // `double_spends` with a tx_id that was never inserted into
        // `transactions`, permanently blocking future transactions from
        // spending those outpoints.
        //
        // Runs BEFORE any capacity work (#2555): every rejection check in this
        // function is read-only against the live pool, so a rejected
        // submission — the attacker-controlled case — costs no eviction and
        // mutates nothing.
        let mut intra_tx_outpoints: FxHashSet<(Hash, u32)> = FxHashSet::default();
        for input in &tx.inputs {
            let outpoint = (input.txid, input.vout);
            if !intra_tx_outpoints.insert(outpoint) {
                return Err(anyhow!("Transaction has duplicate inputs"));
            }
            if let Some(spending_txs) = self.double_spends.get(&outpoint) {
                if !spending_txs.is_empty() {
                    return Err(anyhow!("Double spend detected in mempool"));
                }
            }
        }

        // Capacity is the ONLY remaining fallible step, and it is split into a
        // read-only PLAN and an infallible APPLY so an Err leaves every pool
        // index byte-identical (#2555) — no staged clone of the pool, no
        // journal: nothing is mutated until nothing can fail.
        if self.current_size + tx_size > self.max_size {
            let victims = self.plan_capacity_evictions(&tx, tx_size)?;
            for victim in &victims {
                self.remove_transaction(victim);
            }
        }

        // Dependencies are computed AFTER the capacity decision, from the
        // post-eviction map — and `plan_capacity_evictions` additionally
        // refuses to evict the incoming transaction's own in-pool ancestors —
        // so an admitted child can never hold a dependency edge to a
        // transaction the same admission just removed.
        let mut depends_on = FxHashSet::default();
        for input in &tx.inputs {
            if self.transactions.contains_key(&input.txid) {
                depends_on.insert(input.txid);
            }
        }

        // All checks passed; record outpoints as spent.
        for input in &tx.inputs {
            self.double_spends
                .entry((input.txid, input.vout))
                .or_default()
                .push(tx_id);
        }

        // Fee rate = milli-units of fee per byte (#2026), via the shared
        // `fee_rate_milli` helper — the SAME function the #2039 floor calls
        // above (#2061), so ordering and the admission floor cannot drift apart
        // in units. `abs_fee` remains the secondary tie-break.
        let fee_rate = Self::fee_rate_milli(tx.fee, tx_size);
        let abs_fee = tx.fee;

        // Store transaction — Arc avoids deep clone on every retrieval
        self.transactions.insert(tx_id, Arc::new(tx));
        self.timestamps.insert(tx_id, Instant::now());
        self.current_size += tx_size;
        // #2039: count this sender's slot (decremented on every removal path).
        *self.per_sender_count.entry(sender_key).or_insert(0) += 1;
        if let Some(key) = protocol_replay_key {
            self.protocol_replay_keys.insert(key);
        }

        // O(log n) ordered insert: primary=fee_rate desc, secondary=abs_fee desc (avoids
        // priority inversion when integer division collapses nearby fees to same rate).
        self.fee_order.insert((Reverse(fee_rate), Reverse(abs_fee), tx_id), ());
        self.tx_fee_rates.insert(tx_id, (fee_rate, abs_fee));

        // Store dependencies and maintain the reverse index in lock-step so
        // removal can drop this tx from its dependents in O(d) (#1976).
        if !depends_on.is_empty() {
            for dep_id in &depends_on {
                self.dependents.entry(*dep_id).or_default().insert(tx_id);
            }
            self.dependencies.insert(tx_id, depends_on);
        }

        debug!(
            "Added transaction to mempool: {tx_id} (size: {tx_size} bytes, fee rate: {fee_rate})"
        );

        Ok(())
    }

    /// Removes a transaction from the mempool
    #[allow(clippy::excessive_nesting)]
    pub fn remove_transaction(&mut self, tx_id: &Hash) -> bool {
        if let Some(tx) = self.transactions.remove(tx_id) {
            // Update mempool size
            self.current_size = self.current_size.saturating_sub(tx.size());
            // #2039: keep the per-sender count exact on removal.
            self.decr_sender(&Self::sender_key(&tx));

            // Remove from fee-rate ordered index (O(log n))
            if let Some((fee_rate, abs_fee)) = self.tx_fee_rates.remove(tx_id) {
                self.fee_order.remove(&(Reverse(fee_rate), Reverse(abs_fee), *tx_id));
            }

            // Remove from priorities
            self.priorities.remove(tx_id);

            // Remove timestamp
            self.timestamps.remove(tx_id);

            // Remove outpoints from double spends
            for input in &tx.inputs {
                let outpoint = (input.txid, input.vout);
                if let Some(spending_txs) = self.double_spends.get_mut(&outpoint) {
                    spending_txs.retain(|id| id != tx_id);
                    if spending_txs.is_empty() {
                        self.double_spends.remove(&outpoint);
                    }
                }
            }

            // Remove this tx's own forward-dependency set and, for each tx it
            // depended on, drop the back-edge from that tx's dependents set so
            // the reverse index stays an exact inverse of `dependencies`.
            if let Some(deps) = self.dependencies.remove(tx_id) {
                for dep_id in &deps {
                    if let Some(dependent_set) = self.dependents.get_mut(dep_id) {
                        dependent_set.remove(tx_id);
                        if dependent_set.is_empty() {
                            self.dependents.remove(dep_id);
                        }
                    }
                }
            }

            if let Ok(Some(key)) = self.launch_replay_key(&tx) {
                self.protocol_replay_keys.remove(&key);
            }

            // Remove this transaction from every dependency set that referenced
            // it. The reverse index (#1976) gives us the exact dependents in
            // O(d) instead of rescanning all dependency sets — same end state,
            // but no longer O(m) per removal (which made eviction O(m×n)).
            if let Some(dependents) = self.dependents.remove(tx_id) {
                for dependent_id in &dependents {
                    if let Some(deps) = self.dependencies.get_mut(dependent_id) {
                        deps.remove(tx_id);
                        if deps.is_empty() {
                            self.dependencies.remove(dependent_id);
                        }
                    }
                }
            }

            debug!("Removed transaction from mempool: {tx_id}");
            true
        } else {
            false
        }
    }

    /// Remove a batch of confirmed transactions by ID, clearing their
    /// replay-protection keys so they cannot accumulate unboundedly (#1847).
    pub fn drain_confirmed_block_txs(&mut self, tx_ids: &[Hash]) {
        for id in tx_ids {
            self.remove_transaction(id);
        }
    }

    /// Gets a transaction from the mempool by ID
    pub fn get_transaction(&self, tx_id: &Hash) -> Option<Arc<Transaction>> {
        self.transactions.get(tx_id).cloned()
    }

    /// Gets all transactions in the mempool
    pub fn get_all_transactions(&self) -> Vec<Arc<Transaction>> {
        self.transactions.values().cloned().collect()
    }

    /// Iterates transactions without cloning the underlying transaction payloads.
    pub fn iter_transactions(&self) -> impl Iterator<Item = &Arc<Transaction>> {
        self.transactions.values()
    }

    /// Iterates transactions in descending fee-rate order (highest first),
    /// yielding references without deep-cloning the payloads.
    pub fn iter_by_fee_order(&self) -> impl Iterator<Item = &Arc<Transaction>> {
        self.fee_order
            .keys()
            .filter_map(move |(_, _, tx_id)| self.transactions.get(tx_id))
    }

    /// Gets transactions of a specific type, up to the specified limit
    pub fn get_transactions_by_type(
        &self,
        tx_type: TransactionType,
        limit: usize,
    ) -> Vec<Arc<Transaction>> {
        self.iter_transactions()
            .filter(|tx| tx.tx_type == tx_type)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Buckets transactions by the requested set of types in a single pass.
    ///
    /// Behaviour-preserving optimisation for the PoP block builder, which
    /// previously called [`Self::get_transactions_by_type`] once per type —
    /// each call re-scanning the entire mempool (O(n) × number of types).
    /// This performs one O(n) scan, routing each matching transaction into the
    /// bucket for its type while respecting a per-type `limit`.
    ///
    /// The returned buckets are byte-for-byte identical to calling
    /// `get_transactions_by_type(ty, limit)` independently for each type:
    /// - the source iteration order is `iter_transactions()` (HashMap values
    ///   order, stable across reads while the pool is unmutated), and a single
    ///   pass preserves that relative order within each bucket;
    /// - each bucket independently stops collecting once it reaches `limit`,
    ///   matching the per-type `.take(limit)` semantics.
    ///
    /// `types` must not contain duplicates; the returned `Vec` is aligned
    /// positionally with `types` (index `i` holds the bucket for `types[i]`).
    pub fn get_transactions_grouped_by_types(
        &self,
        types: &[TransactionType],
        limit: usize,
    ) -> Vec<Vec<Arc<Transaction>>> {
        let mut buckets: Vec<Vec<Arc<Transaction>>> = vec![Vec::new(); types.len()];

        // Once every requested bucket is full, further scanning cannot change
        // the result, so we can stop early.
        let mut remaining_open = types.len();

        for tx in self.iter_transactions() {
            if remaining_open == 0 {
                break;
            }
            // Find the bucket index for this transaction's type (if requested).
            if let Some(idx) = types.iter().position(|t| *t == tx.tx_type) {
                let bucket = &mut buckets[idx];
                if bucket.len() < limit {
                    bucket.push(tx.clone());
                    if bucket.len() == limit {
                        remaining_open -= 1;
                    }
                }
            }
        }

        buckets
    }

    /// Gets all transactions from the mempool with no size limit
    /// This is mainly used by API handlers
    pub fn get_transactions(&self) -> Vec<Arc<Transaction>> {
        self.get_all_transactions()
    }

    /// Gets transactions for a block, up to the specified size limit
    /// This version is used by the consensus modules
    #[allow(clippy::excessive_nesting)]
    pub fn get_transactions_with_limit(&self, size_limit: usize) -> Vec<Transaction> {
        let mut result = Vec::new();
        let mut total_size = 0;
        let mut included_ids = FxHashSet::default();

        // First pass: include transactions in descending fee-rate order, respecting dependencies.
        // fee_order keys are (Reverse<fee_rate>, Reverse<abs_fee>, txid): highest-fee-rate first,
        // ties broken by highest absolute fee.
        for (_, _, tx_id) in self.fee_order.keys() {
            if self.can_include_transaction(tx_id, &included_ids) {
                if let Some(arc_tx) = self.transactions.get(tx_id) {
                    let tx_size = arc_tx.size();

                    // Check if adding this would exceed the size limit
                    if total_size + tx_size > size_limit {
                        continue;
                    }

                    result.push((**arc_tx).clone());
                    included_ids.insert(*tx_id);
                    total_size += tx_size;
                }
            }
        }

        result
    }

    /// Returns the number of transactions in the mempool
    pub fn size(&self) -> usize {
        self.transactions.len()
    }

    /// Returns the id of every transaction currently held.
    ///
    /// Reads the real map keys (not `Transaction::id`) so the result is exactly
    /// the set `remove_transaction` accepts. Used to diff a staged pool against
    /// its origin and replay only the removals onto the live pool (#2290).
    pub fn transaction_ids(&self) -> Vec<Hash> {
        // determinism-ok: collected FxHashMap keys are sorted before this method returns
        let mut ids: Vec<Hash> = self.transactions.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Capture local arrival order before candidate assembly (#2199). This is
    /// monitoring evidence only; no peer is assumed to have the same view.
    pub fn ordering_snapshot(&self) -> OrderingMempoolSnapshot {
        // determinism-ok: this is local telemetry only; the HashMap view is sorted by timestamp and tx id before ranks are assigned.
        let mut entries: Vec<(Hash, Instant, u64, usize)> = self.transactions.iter().filter_map(|(id, tx)| {
            self.timestamps.get(id).map(|timestamp| (*id, *timestamp, tx.fee, tx.size()))
        }).collect();
        entries.sort_unstable_by_key(|(id, timestamp, _, _)| (*timestamp, *id));
        OrderingMempoolSnapshot { entries: entries.into_iter().enumerate().map(|(rank, (tx_id, _, fee, size))| OrderingMempoolEntry { tx_id, fee, size, arrival_rank: rank as u64 }).collect() }
    }

    /// Returns true when `tx_id` is currently held.
    pub fn contains(&self, tx_id: &Hash) -> bool {
        self.transactions.contains_key(tx_id)
    }

    /// Returns the total size of the mempool in bytes
    pub fn total_size(&self) -> usize {
        self.current_size
    }

    /// Returns the count of pending transactions in the mempool
    pub async fn get_pending_transactions_count(&self) -> Result<usize> {
        Ok(self.transactions.len())
    }

    /// Sets the priority of a transaction
    pub fn set_priority(&mut self, tx_id: &Hash, priority: TxPriority) {
        self.priorities.insert(*tx_id, priority);
    }

    /// Cleans up old transactions from the mempool.
    ///
    /// Batch-removes all expired transactions in one pass, avoiding the O(n*m)
    /// cost of calling `remove_transaction` individually (each call scans all
    /// `dependencies.values_mut()`). Instead we collect expired IDs, strip them
    /// from every data structure once, and do a single pass over dependencies.
    pub fn clean_old_transactions(&mut self) -> usize {
        let now = Instant::now();
        let expired: Vec<Hash> = self
            .timestamps
            .iter()
            .filter(|(_, ts)| now.duration_since(**ts) > self.max_age)
            .map(|(id, _)| *id)
            .collect();

        if expired.is_empty() {
            return 0;
        }

        let expired_set: FxHashSet<Hash> = expired.iter().copied().collect();
        let count = expired.len();

        for tx_id in &expired {
            if let Some(tx) = self.transactions.remove(tx_id) {
                self.current_size = self.current_size.saturating_sub(tx.size());
                // #2039: keep the per-sender count exact on removal.
                self.decr_sender(&Self::sender_key(&tx));

                // Remove from fee-rate index.
                if let Some((fee_rate, abs_fee)) = self.tx_fee_rates.remove(tx_id) {
                    self.fee_order.remove(&(Reverse(fee_rate), Reverse(abs_fee), *tx_id));
                }

                self.priorities.remove(tx_id);
                self.timestamps.remove(tx_id);

                // Remove outpoints from double-spends.
                for input in &tx.inputs {
                    let outpoint = (input.txid, input.vout);
                    if let Some(spending_txs) = self.double_spends.get_mut(&outpoint) {
                        spending_txs.retain(|id| id != tx_id);
                        if spending_txs.is_empty() {
                            self.double_spends.remove(&outpoint);
                        }
                    }
                }

                self.dependencies.remove(tx_id);

                if let Ok(Some(key)) = self.launch_replay_key(&tx) {
                    self.protocol_replay_keys.remove(&key);
                }
            }
        }

        // Single pass: remove all expired IDs from remaining dependency sets.
        self.dependencies.retain(|_, deps| {
            deps.retain(|id| !expired_set.contains(id));
            !deps.is_empty()
        });

        // Keep the reverse index (#1976) consistent after the batch strip.
        self.rebuild_dependents_index();

        count
    }

    /// Purges transactions that have been in the mempool longer than `max_age_secs` seconds.
    /// Returns the number of transactions removed.
    pub fn purge_older_than_secs(&mut self, max_age_secs: u64) -> usize {
        let cutoff = std::time::Duration::from_secs(max_age_secs);
        let now = Instant::now();
        let expired: Vec<Hash> = self
            .timestamps
            .iter()
            .filter(|(_, ts)| now.duration_since(**ts) > cutoff)
            .map(|(id, _)| *id)
            .collect();

        if expired.is_empty() {
            return 0;
        }

        let expired_set: FxHashSet<Hash> = expired.iter().copied().collect();
        let count = expired.len();

        for tx_id in &expired {
            if let Some(tx) = self.transactions.remove(tx_id) {
                self.current_size = self.current_size.saturating_sub(tx.size());
                // #2039: keep the per-sender count exact on removal.
                self.decr_sender(&Self::sender_key(&tx));
                if let Some((fee_rate, abs_fee)) = self.tx_fee_rates.remove(tx_id) {
                    self.fee_order.remove(&(Reverse(fee_rate), Reverse(abs_fee), *tx_id));
                }
                self.priorities.remove(tx_id);
                self.timestamps.remove(tx_id);
                for input in &tx.inputs {
                    let outpoint = (input.txid, input.vout);
                    if let Some(spending_txs) = self.double_spends.get_mut(&outpoint) {
                        spending_txs.retain(|id| id != tx_id);
                        if spending_txs.is_empty() {
                            self.double_spends.remove(&outpoint);
                        }
                    }
                }
                self.dependencies.remove(tx_id);
                if let Ok(Some(key)) = self.launch_replay_key(&tx) {
                    self.protocol_replay_keys.remove(&key);
                }
            }
        }

        self.dependencies.retain(|_, deps| {
            deps.retain(|id| !expired_set.contains(id));
            !deps.is_empty()
        });

        // Keep the reverse index (#1976) consistent after the batch strip.
        self.rebuild_dependents_index();

        log::warn!(
            "Operator mempool purge: removed {} transactions older than {}s",
            count,
            max_age_secs
        );
        count
    }

    /// Removes all pending transactions from the mempool.
    /// Returns the number of transactions removed.
    pub fn clear_all(&mut self) -> usize {
        let count = self.transactions.len();
        self.transactions.clear();
        self.current_size = 0;
        self.fee_order.clear();
        self.tx_fee_rates.clear();
        self.priorities.clear();
        self.timestamps.clear();
        self.double_spends.clear();
        self.dependencies.clear();
        self.dependents.clear();
        self.protocol_replay_keys.clear();
        // #2061: clear the per-sender admission counters too. Every per-tx
        // removal path decrements these via `decr_sender`, but a bulk
        // `clear_all` bypasses those paths — so without this a sender that was
        // at `mempool_max_txs_per_sender` stayed counted (and rejected) after an
        // operator cleared an already-empty mempool, with no transactions left
        // to explain the rejection.
        self.per_sender_count.clear();
        log::warn!("Operator mempool clear: removed all {} pending transactions", count);
        count
    }

    /// Evicts transactions to make room for a new one of the given size
    /// Read-only capacity planner for admission (#2555).
    ///
    /// Computes the exact set of transactions whose removal makes room for
    /// `tx` (expired entries first — mirroring `clean_old_transactions`'s
    /// criteria — then lowest-fee-rate victims), WITHOUT mutating anything.
    /// The caller applies the returned list only after every fallible step
    /// has succeeded, so a capacity `Err` leaves the pool byte-identical.
    ///
    /// Two protections the mutating eviction path never had:
    /// - the incoming transaction's own in-pool ancestors are never selected,
    ///   so admission cannot orphan the dependency edges it is about to create;
    /// - infeasibility is detected before any removal instead of after a
    ///   partial one.
    fn plan_capacity_evictions(&self, tx: &Transaction, needed_size: usize) -> Result<Vec<Hash>> {
        if needed_size > self.max_size {
            return Err(anyhow!("Transaction too large for mempool"));
        }

        let protected: FxHashSet<Hash> = tx
            .inputs
            .iter()
            .map(|input| input.txid)
            .filter(|txid| self.transactions.contains_key(txid))
            .collect();

        // determinism-ok: capacity planning is local mempool housekeeping and never enters block bytes, state roots, or consensus decisions.
        let now = Instant::now();
        let mut planned: Vec<Hash> = Vec::new();
        let mut planned_set: FxHashSet<Hash> = FxHashSet::default();
        let mut freed_size = 0usize;

        // Expired entries are housekeeping and always go first, protected or
        // not — an expired ancestor could never be minable with its child
        // anyway, and admission would re-reject the child structurally if it
        // mattered. Same criteria as `clean_old_transactions`.
        // determinism-ok: expiry candidates are accumulated as a set-like housekeeping list; eviction order is finalized by later fee/order planning.
        for (tx_id, ts) in &self.timestamps {
            if now.duration_since(*ts) > self.max_age {
                if let Some(entry) = self.transactions.get(tx_id) {
                    if planned_set.insert(*tx_id) {
                        planned.push(*tx_id);
                        freed_size += entry.size();
                    }
                }
            }
        }

        if self.current_size.saturating_sub(freed_size) + needed_size <= self.max_size {
            return Ok(planned);
        }

        // Lowest fee rate first (fee_order is descending), skipping high
        // priority, already-planned, and the incoming tx's own ancestors.
        for (_, _, tx_id) in self.fee_order.keys().rev() {
            if planned_set.contains(tx_id) || protected.contains(tx_id) {
                continue;
            }
            if let Some(TxPriority::High) = self.priorities.get(tx_id) {
                continue;
            }
            let Some(entry) = self.transactions.get(tx_id) else {
                continue;
            };
            planned_set.insert(*tx_id);
            planned.push(*tx_id);
            freed_size += entry.size();
            // Stop as soon as the incoming transaction FITS — headroom the
            // pool already had counts (see evict_transactions's note on the
            // over-eviction this form prevents).
            if self.current_size.saturating_sub(freed_size) + needed_size <= self.max_size {
                return Ok(planned);
            }
        }

        Err(anyhow!("Unable to make room for transaction"))
    }

    /// Rebuilds the `dependents` reverse index from `dependencies`.
    ///
    /// Used by the batch-removal paths (`clean_old_transactions`,
    /// `purge_older_than_secs`), which strip expired ids from `dependencies`
    /// in a single `retain` pass; rebuilding the inverse from the surviving
    /// forward edges afterwards is simpler and just as cheap as patching it
    /// incrementally, and keeps the two maps in exact lock-step (#1976).
    fn rebuild_dependents_index(&mut self) {
        self.dependents.clear();
        for (tx_id, deps) in &self.dependencies {
            for dep_id in deps {
                self.dependents.entry(*dep_id).or_default().insert(*tx_id);
            }
        }
    }

    /// Checks if a transaction can be included based on dependencies
    fn can_include_transaction(&self, tx_id: &Hash, included: &FxHashSet<Hash>) -> bool {
        // Check if it has dependencies
        if let Some(deps) = self.dependencies.get(tx_id) {
            // All dependencies must already be included
            for dep_id in deps {
                if !included.contains(dep_id) {
                    return false;
                }
            }
        }

        true
    }

    fn validate_launch_payload_and_replay_key(
        &self,
        tx: &Transaction,
        signing_keys: Option<&SigningKeyRegistry>,
    ) -> Result<Option<String>> {
        match tx.tx_type {
            TransactionType::ContributionCommit | TransactionType::ForecastCommit => {
                let payload = tx
                    .parse_commit_payload()
                    .ok_or_else(|| anyhow!("Malformed commit payload"))?;
                if payload.commit_id.trim().is_empty() {
                    return Err(anyhow!("commit_id is required"));
                }
                if payload.station_id.trim().is_empty() {
                    return Err(anyhow!("station_id is required"));
                }
                if payload.contribution_type.trim().is_empty() {
                    return Err(anyhow!("contribution_type is required"));
                }
                // Fail-closed sender auth (#1848): reject commits without a valid
                // authorization so forged payloads are stopped at admission.
                if !tx.verify_commit_authorization() {
                    return Err(anyhow!("commit sender authorization failed"));
                }
                Ok(Some(format!("commit:{}", payload.commit_id)))
            },
            TransactionType::ContributionReveal | TransactionType::ForecastReveal => {
                let payload = tx
                    .parse_reveal_payload()
                    .ok_or_else(|| anyhow!("Malformed reveal payload"))?;
                if payload.commit_id.trim().is_empty() {
                    return Err(anyhow!("commit_id is required"));
                }
                if payload.station_id.trim().is_empty() {
                    return Err(anyhow!("station_id is required"));
                }
                if payload.nonce.trim().is_empty() {
                    return Err(anyhow!("nonce is required"));
                }
                if !tx.has_valid_reveal_commit() {
                    return Err(anyhow!("Reveal payload does not match commit hash"));
                }
                Ok(Some(format!("reveal:{}", payload.commit_id)))
            },
            TransactionType::OracleAttestation => {
                let payload = tx
                    .parse_oracle_attestation_payload()
                    .ok_or_else(|| anyhow!("Malformed oracle attestation payload"))?;
                if payload.commit_id.trim().is_empty() {
                    return Err(anyhow!("commit_id is required"));
                }
                if payload.oracle_id.trim().is_empty() {
                    return Err(anyhow!("oracle_id is required"));
                }
                let signature_hex = payload
                    .signature
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow!("signature is required"))?;
                if is_all_zero_signature_hex(signature_hex) {
                    return Err(anyhow!("oracle attestation signature verification failed"));
                }
                // Fail-closed admission (#1723).
                //
                // Previously this fell back to `payload.has_valid_signature()`
                // whenever the signing-key registry was absent. That fallback is
                // *self-certifying*: it verifies the signature against the public
                // key embedded in the attestation's own `oracle_id`, so any actor
                // can mint a keypair, set `oracle_id` to their own pubkey, sign,
                // and pass admission — a reward-forgery surface (oracle
                // attestations gate rewards, #724/#1708). The all-zero guard above
                // only blocks the trivial case, not a forged non-zero signature.
                //
                // We now require the trusted signing-key registry to be present
                // AND the oracle identity to be *explicitly registered* for the
                // Oracle role (no legacy identity-as-key fallback). Legitimate
                // oracles are always registered: every `oracle_set` member is
                // seeded into the registry via `State::ensure_legacy_oracle_keys`,
                // and registering an oracle key requires `oracle_set` membership.
                // This mirrors the apply-time reward gate, which only counts
                // attestations from authorized oracles.
                let registry = signing_keys.ok_or_else(|| {
                    anyhow!("oracle attestation signature verification failed")
                })?;
                if !registry.has_identity_role(&payload.oracle_id, SigningRole::Oracle) {
                    return Err(anyhow!("oracle attestation signature verification failed"));
                }
                let message = payload.signing_message();
                let has_valid_signature = registry.verify_signature_for_identity_role(
                    &payload.oracle_id,
                    SigningRole::Oracle,
                    payload.timestamp,
                    message.as_bytes(),
                    signature_hex,
                );
                if !has_valid_signature {
                    return Err(anyhow!("oracle attestation signature verification failed"));
                }
                if let Some(confidence) = payload.confidence {
                    if !(0.0..=1.0).contains(&confidence) {
                        return Err(anyhow!("confidence must be between 0 and 1"));
                    }
                }
                Ok(Some(format!(
                    "attestation:{}:{}",
                    payload.commit_id, payload.oracle_id
                )))
            },
            // #1763: a Stake/Unstake authorization is bound to
            // `account|amount|action|nonce`; key replay protection on the same
            // tuple so a captured authorization cannot be re-admitted. The
            // authorization itself is verified above in `add_transaction_with_
            // signing_keys`; here we only derive the dedup key.
            TransactionType::Stake | TransactionType::Unstake => {
                let payload = tx
                    .parse_stake_auth_payload()
                    .ok_or_else(|| anyhow!("Malformed stake authorization payload"))?;
                let action = if tx.tx_type == TransactionType::Unstake {
                    "unstake"
                } else {
                    "stake"
                };
                Ok(Some(format!(
                    "{action}:{}:{}:{}",
                    payload.account, payload.amount, tx.nonce
                )))
            },
            // #2023: dedup transparent Transfers on (from, nonce). Without a
            // replay key a valid transfer can be resubmitted unbounded —
            // production builds a fresh timestamped input txid per submission, so
            // neither the "already in mempool" check nor the outpoint
            // double-spend index fires, and every copy is admitted and gossiped
            // (relay amplification). The from-account authorization is verified
            // in `add_transaction_with_signing_keys`; here we only derive the
            // dedup key, keyed identically to `launch_replay_key` so insert and
            // removal stay symmetric. A bare/legacy Transfer without a
            // transparent payload gets no key (unchanged behavior).
            TransactionType::Transfer => Ok(tx
                .parse_transparent_transfer_payload()
                .map(|payload| format!("transfer:{}:{}", payload.from, tx.nonce))),
            _ => Ok(None),
        }
    }

    fn launch_replay_key(&self, tx: &Transaction) -> Result<Option<String>> {
        match tx.tx_type {
            TransactionType::ContributionCommit | TransactionType::ForecastCommit => tx
                .parse_commit_payload()
                .map(|payload| format!("commit:{}", payload.commit_id))
                .map(Some)
                .ok_or_else(|| anyhow!("Malformed commit payload")),
            TransactionType::ContributionReveal | TransactionType::ForecastReveal => tx
                .parse_reveal_payload()
                .map(|payload| format!("reveal:{}", payload.commit_id))
                .map(Some)
                .ok_or_else(|| anyhow!("Malformed reveal payload")),
            TransactionType::OracleAttestation => tx
                .parse_oracle_attestation_payload()
                .map(|payload| format!("attestation:{}:{}", payload.commit_id, payload.oracle_id))
                .map(Some)
                .ok_or_else(|| anyhow!("Malformed oracle attestation payload")),
            TransactionType::Stake | TransactionType::Unstake => {
                let action = if tx.tx_type == TransactionType::Unstake {
                    "unstake"
                } else {
                    "stake"
                };
                tx.parse_stake_auth_payload()
                    .map(|payload| {
                        format!("{action}:{}:{}:{}", payload.account, payload.amount, tx.nonce)
                    })
                    .map(Some)
                    .ok_or_else(|| anyhow!("Malformed stake authorization payload"))
            },
            // #2023: mirror the `transfer:{from}:{nonce}` key from
            // `validate_launch_payload_and_replay_key` so removal paths drop the
            // same key that admission inserted.
            TransactionType::Transfer => Ok(tx
                .parse_transparent_transfer_payload()
                .map(|payload| format!("transfer:{}:{}", payload.from, tx.nonce))),
            _ => Ok(None),
        }
    }
}

impl Mempool {
    /// Backdates a transaction's timestamp for testing purge-by-age logic.
    #[doc(hidden)]
    pub fn set_transaction_timestamp_for_test(&mut self, tx_id: &Hash, ts: Instant) -> bool {
        if let Some(entry) = self.timestamps.get_mut(tx_id) {
            *entry = ts;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::transaction::{
        CommitPayload, OracleAttestationPayload, StakeAuthPayload, TransactionInput,
        TransactionOutput,
    };
    use crate::common::types::Address;
    use crate::crypto::hash::hash_string;
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn privacy_admission_refuses_both_entry_points_without_pool_mutation() {
        for kind in [TransactionType::RewardClaim, TransactionType::ConfidentialTransfer] {
            for with_keys in [false, true] {
                let tx = build_launch_tx(kind, "{}".into(), "privacy-admission");
                let mut pool = Mempool::new(&Config::default());
                let result = if with_keys {
                    pool.add_transaction_with_signing_keys(
                        tx.clone(),
                        Some(&SigningKeyRegistry::default()),
                    )
                } else {
                    pool.add_transaction(tx.clone())
                };
                let err = result.expect_err("privacy transactions must be refused by local policy");
                assert!(err.to_string().contains("#3408"), "{err}");
                assert_eq!(pool.size(), 0);
                assert_eq!(pool.current_size, 0);
                assert!(pool.double_spends.is_empty());
                assert!(pool.protocol_replay_keys.is_empty());
                assert!(!pool.remove_transaction(&tx.id.unwrap()));
            }
        }
    }

    /// #3277 S4: this lib test process never installs a schedule-extension
    /// authority, which is every node before S5. The policy refusals come
    /// first (they read only the artifact); a well-formed, schedulable
    /// extension is then refused for want of an authority — a node that could
    /// apply no extension admits none.
    #[test]
    fn feature_schedule_extension_admission_without_an_authority_3277() {
        use crate::governance::schedule_extension::{
            FeatureScheduleExtension, ScheduleEntry, SignedFeatureScheduleExtension,
            schedule_extension_authority, sign_feature_schedule_extension_with_key,
        };
        assert!(schedule_extension_authority().is_none(), "lib tests install none");
        let key = PrivateKey::generate(&mut StdRng::seed_from_u64(3277));
        let build = |feature: &str| {
            let extension = FeatureScheduleExtension {
                chain_id: "777".into(),
                network_id: "testnet".into(),
                genesis_hash: "ab".repeat(32),
                sequence: 0,
                entries: vec![ScheduleEntry::new(feature, 5_000)],
                version_steps: Vec::new(),
                min_protocol_version: 3,
                release_commit: "c4d0a11".into(),
            };
            let signatures = vec![sign_feature_schedule_extension_with_key(&extension, &key)
            .expect("governance signs a non-held extension")];
            Transaction::new_feature_schedule_extension(&SignedFeatureScheduleExtension {
                extension,
                signatures,
            })
        };
        let mut mempool = Mempool::new(&Config::default());
        let err = mempool.add_transaction(build("paymaster")).unwrap_err().to_string();
        assert!(err.contains("admission policy") && err.contains("not schedulable"), "{err}");
        let err = mempool.add_transaction(build("a_b4_feature")).unwrap_err().to_string();
        assert!(err.contains("not implemented by this binary"), "{err}");
        let err = mempool.add_transaction(build("verify_unit_pricing")).unwrap_err().to_string();
        assert!(err.contains("no schedule-extension authority"), "{err}");
        let mut garbage = build("verify_unit_pricing");
        garbage.data = Some("nope".into());
        garbage.set_id();
        let err = mempool.add_transaction(garbage).unwrap_err().to_string();
        assert!(err.contains("unparsable payload"), "{err}");
        assert_eq!(mempool.size(), 0);
    }

    /// #2024: a `GovernanceProposal` naming an arbitrary `sender_address` must
    /// NOT be admitted.
    ///
    /// Nothing authenticates that field for this type: admission's signature
    /// gate is scoped to `Transfer`, `validate_structure` checks shape only, and
    /// the outpoint index is mempool-local dedup that never checks ownership.
    /// `CreateGovernanceProposal` then debits `proposer` -- so admitting this
    /// relays a transaction that moves a victim's funds.
    #[test]
    fn forged_governance_proposal_rejected_at_admission_2024() {
        let payload = crate::blockchain::transaction::GovernanceProposalPayload {
            proposal_id: "attacker-proposal-2024".to_string(),
            parameter: "emission_rate".to_string(),
            value: "1".to_string(),
            voting_deadline: None,
            deposit: Some(5_000),
        };
        let mut tx = build_launch_tx(
            TransactionType::GovernanceProposal,
            serde_json::to_string(&payload).unwrap(),
            "forged-gov-2024",
        );
        // The victim. No key of theirs is used anywhere in this transaction.
        tx.sender_address = Address([9u8; 20]);
        tx.set_id();

        let mut mp = Mempool::new(&Config::default());
        let err = mp
            .add_transaction(tx)
            .expect_err("an unauthenticated GovernanceProposal must not be admitted");
        assert!(
            err.to_string().contains("#2024"),
            "the refusal must be attributable to the #2024 fence, got: {err}"
        );
    }

    fn build_launch_tx(tx_type: TransactionType, payload: String, tag: &str) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("input-{tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-{tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );

        tx.tx_type = tx_type;
        tx.data = Some(payload);
        tx.set_id();
        tx
    }

    fn build_valid_payload(tx_type: TransactionType, padding_len: usize, tag: &str) -> String {
        match tx_type {
            TransactionType::ContributionCommit | TransactionType::ForecastCommit => {
                let contribution_type = if tx_type == TransactionType::ContributionCommit {
                    "contribution"
                } else {
                    "forecast"
                };
                let seed = tag.bytes().fold(42u64, |acc, b| acc.saturating_add(b as u64));
                let mut rng = StdRng::seed_from_u64(seed);
                let private_key = PrivateKey::generate(&mut rng);
                let mut payload = CommitPayload {
                    commit_id: format!("commit-{tag}"),
                    station_id: format!("station-{}{}", tag, "a".repeat(padding_len)),
                    timestamp: 1_700_000_000_i64,
                    contribution_type: contribution_type.to_string(),
                    auth_pubkey: None,
                    auth_signature: None,
                };
                payload.sign_authorization(&private_key);
                serde_json::to_string(&payload).expect("CommitPayload serialization failed")
            },
            TransactionType::ContributionReveal | TransactionType::ForecastReveal => {
                let contribution_type = if tx_type == TransactionType::ContributionReveal {
                    "contribution"
                } else {
                    "forecast"
                };
                let station_id = format!("station-{tag}");
                let nonce = format!("nonce-{tag}");
                let data = serde_json::json!({ "blob": "a".repeat(padding_len) });
                let commit_id = Transaction::compute_commit_id(
                    &data,
                    &nonce,
                    &station_id,
                    1_700_000_000_i64,
                    contribution_type,
                );
                serde_json::json!({
                    "commit_id": commit_id,
                    "station_id": station_id,
                    "timestamp": 1_700_000_000_i64,
                    "contribution_type": contribution_type,
                    "data": data,
                    "nonce": nonce
                })
                .to_string()
            },
            TransactionType::OracleAttestation => {
                let mut rng = StdRng::seed_from_u64(
                    10_000
                        + tag
                            .bytes()
                            .fold(0u64, |acc, b| acc.saturating_add(b as u64)),
                );
                let private_key = PrivateKey::generate(&mut rng);
                let oracle_id = PublicKey::from_private_key(&private_key).to_string();
                let commit_id = format!("commit-{tag}");
                let approved = true;
                let timestamp = 1_700_000_000_i64;
                let payload = OracleAttestationPayload {
                    commit_id: commit_id.clone(),
                    oracle_id: oracle_id.clone(),
                    approved,
                    signature: None,
                    confidence: Some(0.9),
                    timestamp,
                    nonce: None,
                };
                let message = payload.signing_message();
                let signature = private_key.sign(message.as_bytes()).to_string();
                serde_json::json!({
                    "commit_id": commit_id,
                    "oracle_id": oracle_id,
                    "approved": approved,
                    "signature": signature,
                    "confidence": 0.9,
                    "timestamp": timestamp,
                    "padding": "a".repeat(padding_len)
                })
                .to_string()
            },
            _ => unreachable!("invalid launch tx type for test payload"),
        }
    }

    /// Build a minimal transaction of an arbitrary type with a unique id.
    fn build_typed_tx(tx_type: TransactionType, tag: &str) -> Arc<Transaction> {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("typed-input-{tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-typed-{tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = tx_type;
        tx.set_id();
        Arc::new(tx)
    }

    /// `get_transactions_grouped_by_types` must return buckets byte-for-byte
    /// identical to calling `get_transactions_by_type` once per type — same
    /// transactions, same order within each type, same per-type cap (#1978).
    #[test]
    fn grouped_by_types_matches_per_type_scans() {
        let mut mempool = Mempool::new(&Config::default());

        // A representative mix of the five PoP priority classes plus a couple of
        // unrelated regular transfers that must never leak into the buckets.
        let priority_types = [
            TransactionType::ContributionCommit,
            TransactionType::ForecastCommit,
            TransactionType::OracleAttestation,
            TransactionType::ContributionReveal,
            TransactionType::ForecastReveal,
        ];

        // Insert several txs of each priority type, interleaved with regulars, so
        // the HashMap holds a heterogeneous mix.
        for i in 0..7 {
            for ty in priority_types {
                let tx = build_typed_tx(ty, &format!("{ty:?}-{i}"));
                mempool
                    .transactions
                    .insert(tx.id.expect("id set"), tx);
            }
            let regular = build_typed_tx(TransactionType::Transfer, &format!("transfer-{i}"));
            mempool
                .transactions
                .insert(regular.id.expect("id set"), regular);
        }

        // Compare across a range of limits, including 0, partial, and saturating.
        for limit in [0usize, 1, 3, 7, 100] {
            let grouped = mempool.get_transactions_grouped_by_types(&priority_types, limit);
            assert_eq!(grouped.len(), priority_types.len());
            for (idx, ty) in priority_types.iter().enumerate() {
                let expected = mempool.get_transactions_by_type(*ty, limit);
                let expected_ids: Vec<_> = expected.iter().map(|t| t.id).collect();
                let actual_ids: Vec<_> = grouped[idx].iter().map(|t| t.id).collect();
                assert_eq!(
                    actual_ids, expected_ids,
                    "bucket for {ty:?} at limit {limit} must match per-type scan exactly"
                );
                // No regular/transfer tx should appear in any bucket.
                assert!(
                    grouped[idx].iter().all(|t| t.tx_type == *ty),
                    "bucket for {ty:?} contains a foreign tx type"
                );
            }
        }
    }

    /// Extract the `oracle_id` from a serialized oracle-attestation payload.
    fn oracle_id_from_payload(payload_json: &str) -> String {
        let value: serde_json::Value =
            serde_json::from_str(payload_json).expect("payload must be valid JSON");
        value["oracle_id"]
            .as_str()
            .expect("oracle_id must be present")
            .to_string()
    }

    /// Build a signing-key registry that explicitly registers `oracle_id` for
    /// the Oracle role (legacy identity-as-key mode), mirroring how production
    /// seeds `oracle_set` members via `State::ensure_legacy_oracle_keys`.
    fn registry_with_oracle(oracle_id: &str) -> SigningKeyRegistry {
        let mut registry = SigningKeyRegistry::default();
        registry
            .ensure_legacy_identity_key(oracle_id, SigningRole::Oracle, 0)
            .expect("oracle registration should succeed");
        registry
    }

    #[test]
    fn allows_valid_launch_payloads_below_limits() {
        let mut mempool = Mempool::new(&Config::default());
        let tx_types = [
            TransactionType::ContributionCommit,
            TransactionType::ContributionReveal,
            TransactionType::ForecastCommit,
            TransactionType::ForecastReveal,
            TransactionType::OracleAttestation,
        ];

        for (idx, tx_type) in tx_types.iter().enumerate() {
            let payload = build_valid_payload(*tx_type, 1024, &format!("limit-{idx}"));
            let tx = build_launch_tx(*tx_type, payload.clone(), &format!("limit-{idx}"));
            let result = if *tx_type == TransactionType::OracleAttestation {
                // Oracle attestations now require the signing-key registry to be
                // present and the oracle to be a registered operator (#1723).
                let registry = registry_with_oracle(&oracle_id_from_payload(&payload));
                mempool.add_transaction_with_signing_keys(tx, Some(&registry))
            } else {
                mempool.add_transaction(tx)
            };
            assert!(
                result.is_ok(),
                "expected valid {tx_type:?} payload to be accepted, got {result:?}"
            );
        }
    }

    #[test]
    fn rejects_launch_payloads_above_limit() {
        let mut mempool = Mempool::new(&Config::default());
        let tx_types = [
            TransactionType::ContributionCommit,
            TransactionType::ContributionReveal,
            TransactionType::ForecastCommit,
            TransactionType::ForecastReveal,
            TransactionType::OracleAttestation,
        ];

        for (idx, tx_type) in tx_types.iter().enumerate() {
            let oversized_payload_len = if *tx_type == TransactionType::OracleAttestation {
                MAX_ORACLE_ATTESTATION_PAYLOAD_BYTES + 1
            } else {
                MAX_COMMIT_REVEAL_PAYLOAD_BYTES + 1
            };
            let tx = build_launch_tx(
                *tx_type,
                "a".repeat(oversized_payload_len),
                &format!("overflow-{idx}"),
            );
            let err = mempool
                .add_transaction(tx)
                .expect_err("payload above limit must be rejected");
            let message = err.to_string();
            assert!(
                message.contains("payload exceeds"),
                "unexpected error message for {tx_type:?}: {message}"
            );
        }
    }

    #[test]
    fn rejects_malformed_launch_payloads() {
        let mut mempool = Mempool::new(&Config::default());
        let tx_types = [
            TransactionType::ContributionCommit,
            TransactionType::ContributionReveal,
            TransactionType::ForecastCommit,
            TransactionType::ForecastReveal,
            TransactionType::OracleAttestation,
        ];

        for (idx, tx_type) in tx_types.iter().enumerate() {
            let tx = build_launch_tx(
                *tx_type,
                "not-json".to_string(),
                &format!("malformed-{idx}"),
            );
            let err = mempool
                .add_transaction(tx)
                .expect_err("malformed payload must be rejected");
            assert!(
                err.to_string().to_lowercase().contains("malformed"),
                "expected malformed payload error for {tx_type:?}, got {err}"
            );
        }
    }

    #[test]
    fn rejects_replay_oracle_attestation_by_commit_and_oracle() {
        let mut mempool = Mempool::new(&Config::default());
        // Same tag => same deterministic oracle keypair for both attestations.
        let payload = build_valid_payload(TransactionType::OracleAttestation, 0, "dup");
        let registry = registry_with_oracle(&oracle_id_from_payload(&payload));
        let first = build_launch_tx(TransactionType::OracleAttestation, payload.clone(), "dup-1");
        let second = build_launch_tx(TransactionType::OracleAttestation, payload, "dup-2");

        mempool
            .add_transaction_with_signing_keys(first, Some(&registry))
            .unwrap();
        let err = mempool
            .add_transaction_with_signing_keys(second, Some(&registry))
            .expect_err("duplicate attestation replay key must be rejected");
        assert!(err.to_string().contains("Launch protocol replay detected"));
    }

    fn build_regular_tx_with_fee(fee: u64, tag: &str) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("fee-input-{tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-fee-{tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.fee = fee;
        tx.set_id();
        tx
    }

    #[test]
    fn fee_rate_ordering_highest_first() {
        let mut mempool = Mempool::new(&Config::default());

        let low_fee = build_regular_tx_with_fee(100, "low");
        let mid_fee = build_regular_tx_with_fee(5_000, "mid");
        let high_fee = build_regular_tx_with_fee(50_000, "high");

        let low_id = low_fee.id.unwrap();
        let mid_id = mid_fee.id.unwrap();
        let high_id = high_fee.id.unwrap();

        // Add in random order
        mempool.add_transaction(low_fee).unwrap();
        mempool.add_transaction(high_fee).unwrap();
        mempool.add_transaction(mid_fee).unwrap();

        // Verify fee-rate ordering: highest first via get_transactions_with_limit
        let all_txs = mempool.get_transactions_with_limit(usize::MAX);
        assert_eq!(all_txs.len(), 3);
        assert_eq!(*all_txs[0].id.as_ref().unwrap(), high_id);
        assert_eq!(*all_txs[1].id.as_ref().unwrap(), mid_id);
        assert_eq!(*all_txs[2].id.as_ref().unwrap(), low_id);
    }

    #[test]
    fn transaction_ids_are_sorted_before_returning() {
        let mut mempool = Mempool::new(&Config::default());
        let first = build_regular_tx_with_fee(100, "ids-first");
        let second = build_regular_tx_with_fee(200, "ids-second");
        let mut expected = vec![first.id.unwrap(), second.id.unwrap()];
        expected.sort_unstable();

        mempool.add_transaction(second).unwrap();
        mempool.add_transaction(first).unwrap();

        assert_eq!(mempool.transaction_ids(), expected);
    }

    #[test]
    fn get_transactions_with_limit_respects_fee_order() {
        let mut mempool = Mempool::new(&Config::default());

        let low_fee = build_regular_tx_with_fee(100, "limit-low");
        let high_fee = build_regular_tx_with_fee(50_000, "limit-high");

        let high_id = high_fee.id.unwrap();

        mempool.add_transaction(low_fee).unwrap();
        mempool.add_transaction(high_fee).unwrap();

        // Request with a size limit that only fits one transaction
        let single_tx_size = mempool.transactions.get(&high_id).unwrap().size();
        let result = mempool.get_transactions_with_limit(single_tx_size);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id.as_ref().unwrap(), &high_id);
    }

    #[test]
    fn get_transaction_returns_shared_arc() {
        let mut mempool = Mempool::new(&Config::default());
        let tx = build_regular_tx_with_fee(2_500, "arc-storage");
        let tx_id = tx.id.unwrap();

        mempool.add_transaction(tx).unwrap();

        let stored = mempool
            .transactions
            .get(&tx_id)
            .expect("transaction should be stored in the mempool");
        assert_eq!(Arc::strong_count(stored), 1);

        let retrieved = mempool
            .get_transaction(&tx_id)
            .expect("stored transaction should be retrievable");
        assert_eq!(retrieved.id, Some(tx_id));
        assert!(Arc::ptr_eq(stored, &retrieved));
        assert_eq!(Arc::strong_count(stored), 2);
    }

    #[test]
    fn get_all_transactions_returns_shared_arcs() {
        let mut mempool = Mempool::new(&Config::default());
        let tx = build_regular_tx_with_fee(7_500, "all-transactions");
        let tx_id = tx.id.unwrap();

        mempool.add_transaction(tx).unwrap();

        let stored = mempool
            .transactions
            .get(&tx_id)
            .expect("stored transaction should remain available");
        let all = mempool.get_all_transactions();
        assert_eq!(all.len(), 1);
        assert!(Arc::ptr_eq(stored, &all[0]));
        assert_eq!(Arc::strong_count(stored), 2);
    }

    #[test]
    fn zero_fee_transaction_gets_lowest_priority() {
        let mut mempool = Mempool::new(&Config::default());

        let zero_fee = build_regular_tx_with_fee(0, "zero");
        let some_fee = build_regular_tx_with_fee(1_000, "some");

        let zero_id = zero_fee.id.unwrap();
        let some_id = some_fee.id.unwrap();

        mempool.add_transaction(zero_fee).unwrap();
        mempool.add_transaction(some_fee).unwrap();

        // some_fee should be first (higher fee rate), zero_fee last
        let all_txs = mempool.get_transactions_with_limit(usize::MAX);
        assert_eq!(all_txs.len(), 2);
        assert_eq!(*all_txs[0].id.as_ref().unwrap(), some_id);
        assert_eq!(*all_txs[1].id.as_ref().unwrap(), zero_id);
        assert_eq!(all_txs[1].fee, 0);
    }

    #[test]
    fn rejects_oracle_attestation_without_signature() {
        let mut mempool = Mempool::new(&Config::default());
        let mut rng = StdRng::seed_from_u64(99);
        let private_key = PrivateKey::generate(&mut rng);
        let oracle_id = PublicKey::from_private_key(&private_key).to_string();
        let payload = serde_json::json!({
            "commit_id": "commit-missing-sig",
            "oracle_id": oracle_id,
            "approved": true,
            "signature": null,
            "confidence": 0.9,
            "timestamp": 1_700_000_000_i64
        })
        .to_string();

        let tx = build_launch_tx(TransactionType::OracleAttestation, payload, "missing-sig");
        let err = mempool
            .add_transaction(tx)
            .expect_err("unsigned oracle attestation must be rejected");
        assert!(err.to_string().contains("signature is required"));
    }

    #[test]
    fn reject_invalid_oracle_attestation_signature() {
        let mut mempool = Mempool::new(&Config::default());
        let mut rng = StdRng::seed_from_u64(100);
        let private_key = PrivateKey::generate(&mut rng);
        let oracle_id = PublicKey::from_private_key(&private_key).to_string();
        let payload = serde_json::json!({
            "commit_id": "commit-invalid-sig",
            "oracle_id": oracle_id,
            "approved": true,
            "signature": "0".repeat(128),
            "confidence": 0.9,
            "timestamp": 1_700_000_000_i64
        })
        .to_string();

        let tx = build_launch_tx(TransactionType::OracleAttestation, payload, "invalid-sig");
        let err = mempool
            .add_transaction(tx)
            .expect_err("invalid oracle attestation signature must be rejected");
        assert!(
            err.to_string()
                .contains("oracle attestation signature verification failed"),
            "unexpected error: {err}"
        );
        assert!(
            mempool.get_all_transactions().is_empty(),
            "invalid oracle attestation must not enter the mempool"
        );
    }

    /// Build a cryptographically valid (self-signed) oracle attestation tx and
    /// return both the tx and the oracle's public id. The signature is a real
    /// ed25519 signature over the canonical signing message, signed by the key
    /// whose public half equals `oracle_id` — i.e. exactly the kind of
    /// self-certifying attestation the old fallback would have admitted.
    fn signed_oracle_attestation(seed: u64, commit_tag: &str, tx_tag: &str) -> (Transaction, String) {
        let mut rng = StdRng::seed_from_u64(seed);
        let private_key = PrivateKey::generate(&mut rng);
        let oracle_id = PublicKey::from_private_key(&private_key).to_string();
        let commit_id = format!("commit-{commit_tag}");
        let timestamp = 1_700_000_000_i64;
        let payload = OracleAttestationPayload {
            commit_id: commit_id.clone(),
            oracle_id: oracle_id.clone(),
            approved: true,
            signature: None,
            confidence: Some(0.9),
            timestamp,
            nonce: None,
        };
        let signature = private_key.sign(payload.signing_message().as_bytes()).to_string();
        let json = serde_json::json!({
            "commit_id": commit_id,
            "oracle_id": oracle_id,
            "approved": true,
            "signature": signature,
            "confidence": 0.9,
            "timestamp": timestamp,
        })
        .to_string();
        (build_launch_tx(TransactionType::OracleAttestation, json, tx_tag), oracle_id)
    }

    #[test]
    fn oracle_attestation_rejected_when_registry_absent() {
        // #1723: fail-closed. A cryptographically valid *self-signed* attestation
        // must NOT be admitted when the signing-key registry is absent — the old
        // `unwrap_or_else(|| payload.has_valid_signature())` fallback would have
        // admitted this forgeable, self-certified attestation.
        let mut mempool = Mempool::new(&Config::default());
        let (tx, _oracle_id) = signed_oracle_attestation(7001, "no-registry", "no-registry");

        let err = mempool
            .add_transaction(tx)
            .expect_err("oracle attestation must be rejected when registry is absent");
        assert!(
            err.to_string()
                .contains("oracle attestation signature verification failed"),
            "unexpected error: {err}"
        );
        assert!(
            mempool.get_all_transactions().is_empty(),
            "self-certified attestation must not enter the mempool without a registry"
        );
    }

    #[test]
    fn oracle_attestation_rejected_when_oracle_unregistered() {
        // #1723: even WITH a registry present, an attestation whose oracle_id is
        // not an explicitly registered operator must be rejected. This closes the
        // legacy identity-as-key self-cert path: the attacker mints a keypair,
        // sets oracle_id to their own pubkey, and signs a valid signature — but is
        // not in the registry, so admission fails closed.
        let mut mempool = Mempool::new(&Config::default());
        let empty_registry = SigningKeyRegistry::default();
        let (tx, _oracle_id) = signed_oracle_attestation(7002, "unregistered", "unregistered");

        let err = mempool
            .add_transaction_with_signing_keys(tx, Some(&empty_registry))
            .expect_err("attestation from unregistered oracle must be rejected");
        assert!(
            err.to_string()
                .contains("oracle attestation signature verification failed"),
            "unexpected error: {err}"
        );
        assert!(mempool.get_all_transactions().is_empty());
    }

    #[test]
    fn oracle_attestation_accepted_when_registered_and_signed() {
        // #1723: the legitimate path still works — a registered oracle with a
        // valid signature is admitted.
        let mut mempool = Mempool::new(&Config::default());
        let (tx, oracle_id) = signed_oracle_attestation(7003, "registered", "registered");
        let registry = registry_with_oracle(&oracle_id);

        mempool
            .add_transaction_with_signing_keys(tx, Some(&registry))
            .expect("registered + validly signed attestation must be admitted");
        assert_eq!(mempool.get_all_transactions().len(), 1);
    }

    #[test]
    fn oracle_attestation_rejected_when_registered_but_signature_forged() {
        // #1723: a registered oracle id with a signature that does NOT verify
        // against the registered key (here, signed by a different key while
        // claiming a registered oracle_id) must be rejected.
        let mut mempool = Mempool::new(&Config::default());

        // The registered (victim) oracle identity.
        let mut victim_rng = StdRng::seed_from_u64(7004);
        let victim_id = PublicKey::from_private_key(&PrivateKey::generate(&mut victim_rng)).to_string();
        let registry = registry_with_oracle(&victim_id);

        // Attacker signs with their own key but claims the victim's oracle_id.
        let mut attacker_rng = StdRng::seed_from_u64(7005);
        let attacker_key = PrivateKey::generate(&mut attacker_rng);
        let commit_id = "commit-forged".to_string();
        let timestamp = 1_700_000_000_i64;
        let payload = OracleAttestationPayload {
            commit_id: commit_id.clone(),
            oracle_id: victim_id.clone(),
            approved: true,
            signature: None,
            confidence: Some(0.9),
            timestamp,
            nonce: None,
        };
        let forged_sig = attacker_key.sign(payload.signing_message().as_bytes()).to_string();
        let json = serde_json::json!({
            "commit_id": commit_id,
            "oracle_id": victim_id,
            "approved": true,
            "signature": forged_sig,
            "confidence": 0.9,
            "timestamp": timestamp,
        })
        .to_string();
        let tx = build_launch_tx(TransactionType::OracleAttestation, json, "forged");

        let err = mempool
            .add_transaction_with_signing_keys(tx, Some(&registry))
            .expect_err("forged signature for a registered oracle must be rejected");
        assert!(
            err.to_string()
                .contains("oracle attestation signature verification failed"),
            "unexpected error: {err}"
        );
        assert!(mempool.get_all_transactions().is_empty());
    }

    #[test]
    fn default_config_mempool_limits() {
        let config = Config::default();
        assert_eq!(config.mempool_max_age_secs, 4 * 3600, "default max_age should be 4 hours");
        assert_eq!(
            config.mempool_max_size_bytes,
            50 * 1024 * 1024,
            "default max_size should be 50 MB"
        );

        let mempool = Mempool::new(&config);
        assert_eq!(mempool.max_age, Duration::from_secs(4 * 3600));
        assert_eq!(mempool.max_size, 50 * 1024 * 1024);
    }

    #[test]
    fn clean_old_transactions_evicts_expired() {
        let mut config = Config::default();
        // Use a very short max_age so we can test eviction without sleeping.
        config.mempool_max_age_secs = 0;
        let mut mempool = Mempool::new(&config);

        // max_age = 0 means every transaction is immediately expired.
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("evict-input-1"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 100,
                address: "wrf-evict-test".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.set_id();
        let tx_id = tx.id.unwrap();

        mempool.add_transaction(tx).expect("should accept tx");
        assert_eq!(mempool.size(), 1);

        let evicted = mempool.clean_old_transactions();
        assert_eq!(evicted, 1, "expired transaction should be evicted");
        assert_eq!(mempool.size(), 0);
        assert!(mempool.get_transaction(&tx_id).is_none());
    }

    #[test]
    fn config_mempool_limits_are_respected() {
        let mut config = Config::default();
        config.mempool_max_age_secs = 1;
        config.mempool_max_size_bytes = 10 * 1024 * 1024; // 10 MB
        let mempool = Mempool::new(&config);

        assert_eq!(mempool.max_age, Duration::from_secs(1));
        assert_eq!(mempool.max_size, 10 * 1024 * 1024);
    }

    // ── #2039: minimum-fee floor + per-sender admission cap ────────────────

    /// The min-fee floor rejects below-floor transactions and admits
    /// at/above-floor ones.
    #[test]
    fn mempool_min_fee_floor_rejects_below_admits_above() {
        // The floor is in milli-units of fee per byte (`fee * 1000 / size`) —
        // the SAME units as the sort key (#2061). Compute each tx's actual rate
        // via the shared helper and set the floor strictly between them, so the
        // test is robust to the exact serialized size rather than assuming it.
        let below = build_regular_tx_with_fee(100, "floor-below");
        let above = build_regular_tx_with_fee(100_000, "floor-above");
        let below_rate = Mempool::fee_rate_milli(below.fee, below.size());
        let above_rate = Mempool::fee_rate_milli(above.fee, above.size());
        assert!(
            below_rate < above_rate,
            "sanity: the two fees must separate in milli-units ({below_rate} vs {above_rate})"
        );

        let config = Config {
            // Floor just above the low-fee tx's rate: it is rejected, the
            // high-fee tx (>= floor) is admitted. `below_rate + 1 <= above_rate`
            // holds because fee 100 vs 100_000 dominates any size difference.
            mempool_min_fee_rate: below_rate + 1,
            ..Config::default()
        };
        let mut mp = Mempool::new(&config);

        let err = mp
            .add_transaction(below)
            .expect_err("below-floor transaction must be rejected");
        assert!(
            err.to_string().contains("below minimum"),
            "unexpected error: {err}"
        );

        mp.add_transaction(above)
            .expect("above-floor transaction must be admitted");
    }

    /// **#2061 Blocker 2:** the admission floor and the fee-order sort key must
    /// compute the fee rate in the SAME units. `#2060` made the sort key
    /// milli-units (`fee * 1000 / size`); if the floor stayed unscaled a
    /// configured floor would be read 1000× off from ordering. This pins the
    /// boundary in milli-units and asserts the stored sort key equals the exact
    /// value the floor compared against.
    #[test]
    fn floor_and_sort_share_fee_rate_units() {
        let tx = build_regular_tx_with_fee(50_000, "unit-consistency");
        let id = tx.id.expect("tx has an id");
        let rate = Mempool::fee_rate_milli(tx.fee, tx.size());
        assert!(rate > 0, "sanity: pick a fee that yields a non-zero milli rate");

        // floor == rate: admitted (the check rejects only when rate < floor).
        let mut admit = Mempool::new(&Config {
            mempool_min_fee_rate: rate,
            ..Config::default()
        });
        admit
            .add_transaction(tx.clone())
            .expect("a tx whose rate equals the floor must be admitted");
        // …and the sort key stored for that admitted tx is the SAME rate the
        // floor used — proving both sides go through `fee_rate_milli`.
        let (stored_rate, _abs_fee) = admit
            .tx_fee_rates
            .get(&id)
            .copied()
            .expect("admitted tx has a stored sort key");
        assert_eq!(
            stored_rate, rate,
            "the fee-order sort key must equal the floor's fee rate — one helper (#2061)"
        );

        // floor == rate + 1: the same tx is now below the floor → rejected.
        let mut reject = Mempool::new(&Config {
            mempool_min_fee_rate: rate + 1,
            ..Config::default()
        });
        let err = reject
            .add_transaction(tx)
            .expect_err("a tx whose rate is below the floor must be rejected");
        assert!(
            err.to_string().contains("below minimum"),
            "unexpected error: {err}"
        );
    }

    /// The floor is disabled by default (mempool_min_fee_rate == 0) so
    /// faucet-funded zero-fee testnet transactions are still admitted (#2039).
    #[test]
    fn mempool_zero_fee_admitted_when_floor_disabled_by_default() {
        let mut mp = Mempool::new(&Config::default());
        let zero = build_regular_tx_with_fee(0, "zero-default");
        mp.add_transaction(zero)
            .expect("zero-fee tx must be admitted with the floor disabled by default");
    }

    /// The per-sender cap bounds one sender; a different sender is unaffected.
    #[test]
    fn mempool_per_sender_cap_enforced() {
        let config = Config {
            mempool_max_txs_per_sender: 2,
            ..Config::default()
        };
        let mut mp = Mempool::new(&config);

        // build_regular_tx_with_fee leaves the default (zero) sender_address, so
        // these three share one sender key.
        mp.add_transaction(build_regular_tx_with_fee(10, "cap-a"))
            .expect("1st from sender admitted");
        mp.add_transaction(build_regular_tx_with_fee(10, "cap-b"))
            .expect("2nd from sender admitted");
        let err = mp
            .add_transaction(build_regular_tx_with_fee(10, "cap-c"))
            .expect_err("3rd from the same sender must be rejected");
        assert!(
            err.to_string().contains("per-sender limit"),
            "unexpected error: {err}"
        );

        // A different sender is unaffected by the first sender's cap.
        let mut other = build_regular_tx_with_fee(10, "cap-other");
        other.sender_address = Address([7u8; 20]);
        other.set_id();
        mp.add_transaction(other)
            .expect("a different sender must still be admitted");
    }

    /// Removing a transaction frees its sender's slot (#2039 — the count is
    /// decremented on every removal path, so it does not leak).
    #[test]
    fn mempool_per_sender_cap_slot_freed_on_removal() {
        let config = Config {
            mempool_max_txs_per_sender: 1,
            ..Config::default()
        };
        let mut mp = Mempool::new(&config);

        let first = build_regular_tx_with_fee(10, "free-1");
        let first_id = first.id.unwrap();
        mp.add_transaction(first).expect("1st admitted");
        mp.add_transaction(build_regular_tx_with_fee(10, "free-2"))
            .expect_err("2nd rejected while at cap");

        assert!(mp.remove_transaction(&first_id), "first tx must be removed");
        mp.add_transaction(build_regular_tx_with_fee(10, "free-3"))
            .expect("slot freed after removal → must be admitted");
    }

    /// **#2061 Blocker 1:** `clear_all` must also reset the per-sender
    /// admission counters. Every per-tx removal path decrements them via
    /// `decr_sender`, but a bulk `clear_all` bypasses those paths — so before
    /// the fix a sender that had hit `mempool_max_txs_per_sender` stayed counted
    /// and rejected even after an operator cleared the mempool, with no
    /// transactions left to explain the rejection.
    #[test]
    fn clear_all_resets_the_per_sender_cap() {
        let config = Config {
            mempool_max_txs_per_sender: 1,
            ..Config::default()
        };
        let mut mp = Mempool::new(&config);

        mp.add_transaction(build_regular_tx_with_fee(10, "clr-1"))
            .expect("1st admitted");
        mp.add_transaction(build_regular_tx_with_fee(10, "clr-2"))
            .expect_err("2nd rejected while the sender is at its cap");

        // Operator clears the mempool.
        assert_eq!(mp.clear_all(), 1, "one tx should have been cleared");

        // The sender must be admissible again — an empty pool cannot legitimately
        // reject on a per-sender cap. Before #2061 this stayed rejected.
        mp.add_transaction(build_regular_tx_with_fee(10, "clr-3"))
            .expect("after clear_all the sender's slot must be freed");
    }

    // ── Adversarial input validation tests (#1200) ────────────────────────
    //
    // These tests verify the mempool rejects malicious or adversarial inputs
    // gracefully — no panics, no crashes, correct error messages. They form
    // the pre-flight check before the 48h soak test.

    #[test]
    fn adversarial_duplicate_commit_replay_rejected() {
        // Issue #1200 case 2: submitting two commits with the same commit_id
        // from the same station. The second must be rejected with a replay error.
        let mut mempool = Mempool::new(&Config::default());

        let payload = build_valid_payload(TransactionType::ContributionCommit, 0, "dup-commit");
        let first = build_launch_tx(
            TransactionType::ContributionCommit,
            payload.clone(),
            "dup-commit-1",
        );
        let second = build_launch_tx(
            TransactionType::ContributionCommit,
            payload,
            "dup-commit-2",
        );

        mempool.add_transaction(first).expect("first commit should succeed");
        let err = mempool
            .add_transaction(second)
            .expect_err("duplicate commit must be rejected");
        assert!(
            err.to_string().contains("replay"),
            "expected replay error, got: {err}"
        );
    }

    #[test]
    fn unsigned_commit_rejected_signed_commit_accepted_regression_1848() {
        // Regression for #1848: commits without sender auth must be rejected at
        // mempool admission so unsigned P2P commits can never reach the state
        // machine.
        let mut mempool = Mempool::new(&Config::default());

        // Unsigned payload (auth_pubkey=None, auth_signature=None).
        let unsigned_payload = serde_json::to_string(&CommitPayload {
            commit_id: "c-1848-test".to_string(),
            station_id: "station-1848".to_string(),
            timestamp: 1_700_000_000,
            contribution_type: "contribution".to_string(),
            auth_pubkey: None,
            auth_signature: None,
        }).unwrap();
        let unsigned_tx = build_launch_tx(
            TransactionType::ContributionCommit,
            unsigned_payload,
            "unsigned-1848",
        );
        let err = mempool.add_transaction(unsigned_tx)
            .expect_err("unsigned commit must be rejected");
        assert!(
            err.to_string().contains("authorization"),
            "expected authorization error, got: {err}"
        );

        // Signed payload — must be accepted.
        let signed_payload = build_valid_payload(TransactionType::ContributionCommit, 0, "signed-1848");
        let signed_tx = build_launch_tx(TransactionType::ContributionCommit, signed_payload, "signed-1848");
        mempool.add_transaction(signed_tx).expect("signed commit must be accepted");
    }

    #[test]
    fn adversarial_duplicate_reveal_replay_rejected() {
        // Same as above but for reveal transactions.
        let mut mempool = Mempool::new(&Config::default());

        let payload = build_valid_payload(TransactionType::ContributionReveal, 0, "dup-reveal");
        let first = build_launch_tx(
            TransactionType::ContributionReveal,
            payload.clone(),
            "dup-reveal-1",
        );
        let second = build_launch_tx(
            TransactionType::ContributionReveal,
            payload,
            "dup-reveal-2",
        );

        mempool.add_transaction(first).expect("first reveal should succeed");
        let err = mempool
            .add_transaction(second)
            .expect_err("duplicate reveal must be rejected");
        assert!(
            err.to_string().contains("replay"),
            "expected replay error, got: {err}"
        );
    }

    #[test]
    fn adversarial_double_spend_same_input() {
        // Issue #1200 case 2 variant: two regular transactions spending
        // the same (txid, vout) pair.
        let mut mempool = Mempool::new(&Config::default());

        let shared_txid = hash_string("shared-utxo");
        let make_tx = |tag: &str| {
            let mut tx = Transaction::new(
                vec![TransactionInput {
                    txid: shared_txid,
                    vout: 0,
                    signature: None,
                    public_key: None,
                    coinbase_data: None,
                }],
                vec![TransactionOutput {
                    amount: 100,
                    address: format!("wrf-ds-{tag}"),
                    script: "OP_TRUE".to_string(),
                    nft_data: None,
                }],
            );
            tx.set_id();
            tx
        };

        mempool.add_transaction(make_tx("a")).expect("first spend should succeed");
        let err = mempool
            .add_transaction(make_tx("b"))
            .expect_err("double spend must be rejected");
        assert!(
            err.to_string().contains("Double spend"),
            "expected double-spend error, got: {err}"
        );
    }

    #[test]
    fn adversarial_double_spend_higher_fee_cannot_bypass_rejection() {
        // Fee manipulation edge case (#1607): an attacker submits a second
        // transaction with the same input and a much higher fee to try to
        // displace the first pending spend. Mempool must still reject it.
        let mut mempool = Mempool::new(&Config::default());
        let shared_txid = hash_string("shared-utxo-fee-bypass");

        let mut first_tx = Transaction::new(
            vec![TransactionInput {
                txid: shared_txid,
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 100,
                address: "wrf-fee-bypass-a".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        first_tx.fee = 1;
        first_tx.set_id();
        let first_id = first_tx.id.expect("first tx should have id");

        let mut replacement_tx = Transaction::new(
            vec![TransactionInput {
                txid: shared_txid,
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 100,
                address: "wrf-fee-bypass-b".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        replacement_tx.fee = 1_000_000;
        replacement_tx.set_id();

        mempool
            .add_transaction(first_tx)
            .expect("first spend should succeed");
        let err = mempool
            .add_transaction(replacement_tx)
            .expect_err("higher-fee double spend must still be rejected");

        assert!(
            err.to_string().contains("Double spend"),
            "expected double-spend rejection, got: {err}"
        );
        assert_eq!(mempool.size(), 1, "mempool should still hold only the first tx");
        assert!(
            mempool.get_transaction(&first_id).is_some(),
            "original transaction should remain in mempool"
        );
    }

    #[test]
    fn adversarial_mempool_full_evicts_lowest_fee() {
        // Issue #1200 case 5: fill mempool to its size limit, then submit
        // one more transaction. The lowest-fee transaction should be evicted
        // to make room, and the new higher-fee transaction accepted.
        let mut config = Config::default();
        // Use a tiny mempool so we can fill it with a few transactions.
        config.mempool_max_size_bytes = 2048;
        let mut mempool = Mempool::new(&config);

        // Insert a low-fee transaction to fill most of the pool.
        let low = build_regular_tx_with_fee(10, "evict-low");
        let low_id = low.id.unwrap();
        mempool.add_transaction(low).expect("low-fee tx should succeed");
        assert!(mempool.get_transaction(&low_id).is_some());

        // Insert enough transactions to fill the rest.
        let med = build_regular_tx_with_fee(100, "evict-med");
        mempool.add_transaction(med).expect("medium-fee tx should succeed");

        // Now submit a high-fee transaction. The pool is full, so the
        // lowest-fee entry should be evicted.
        let high = build_regular_tx_with_fee(10_000, "evict-high");
        let high_id = high.id.unwrap();
        let result = mempool.add_transaction(high);

        // Either it succeeds (eviction worked) or it returns an error
        // (pool couldn't free enough space). Both are acceptable — the
        // important thing is NO PANIC.
        if result.is_ok() {
            assert!(
                mempool.get_transaction(&high_id).is_some(),
                "high-fee tx should be in pool after eviction"
            );
            // low-fee tx should have been evicted
            assert!(
                mempool.get_transaction(&low_id).is_none(),
                "lowest-fee tx should have been evicted"
            );
        } else {
            let err = result.unwrap_err();
            assert!(
                err.to_string().contains("Unable to make room")
                    || err.to_string().contains("too large"),
                "expected capacity error, got: {err}"
            );
        }
    }

    #[test]
    fn adversarial_mempool_full_rejects_when_no_eviction_possible() {
        // Fill the mempool with high-priority transactions, then try to add
        // one more. Since all existing transactions are High priority (not
        // evictable), the new transaction should be rejected.
        let mut config = Config::default();
        config.mempool_max_size_bytes = 2048; // Tiny pool
        let mut mempool = Mempool::new(&config);

        // Fill with a single high-priority transaction.
        let filler = build_regular_tx_with_fee(50_000, "filler-hp");
        let filler_id = filler.id.unwrap();
        mempool.add_transaction(filler).unwrap();
        mempool.set_priority(&filler_id, TxPriority::High);

        // Now try to add another — eviction can't remove the high-priority tx.
        let newcomer = build_regular_tx_with_fee(100, "newcomer");
        let result = mempool.add_transaction(newcomer);
        // Either it's rejected cleanly, or it fits in remaining space.
        // The key assertion: no panic or crash.
        if let Err(err) = &result {
            let msg = err.to_string();
            assert!(
                msg.contains("Unable to make room") || msg.contains("too large"),
                "expected capacity error, got: {msg}"
            );
        }
    }

    #[test]
    fn adversarial_invalid_structure_no_outputs() {
        // Issue #1200 case 3 variant: structurally invalid transaction
        // (no outputs) must be rejected, not cause a panic.
        let mut mempool = Mempool::new(&Config::default());

        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("bad-struct-input"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![], // No outputs
        );
        tx.set_id();

        let err = mempool
            .add_transaction(tx)
            .expect_err("tx with no outputs must be rejected");
        assert!(
            err.to_string().contains("Invalid transaction structure"),
            "expected structure error, got: {err}"
        );
    }

    #[test]
    fn adversarial_duplicate_tx_rejected() {
        // Submitting the exact same transaction twice must be rejected.
        let mut mempool = Mempool::new(&Config::default());

        let tx = build_regular_tx_with_fee(1_000, "dup-exact");
        let tx_clone = tx.clone();

        mempool.add_transaction(tx).expect("first submission should succeed");
        let err = mempool
            .add_transaction(tx_clone)
            .expect_err("duplicate transaction must be rejected");
        assert!(
            err.to_string().contains("already in mempool"),
            "expected 'already in mempool' error, got: {err}"
        );
    }

    #[test]
    fn adversarial_commit_with_empty_required_fields() {
        // Commit with blank commit_id, station_id, or contribution_type.
        let mut mempool = Mempool::new(&Config::default());

        let cases = [
            (
                serde_json::json!({
                    "commit_id": "",
                    "station_id": "station-1",
                    "timestamp": 1_700_000_000_i64,
                    "contribution_type": "contribution"
                }),
                "commit_id is required",
            ),
            (
                serde_json::json!({
                    "commit_id": "commit-abc",
                    "station_id": "",
                    "timestamp": 1_700_000_000_i64,
                    "contribution_type": "contribution"
                }),
                "station_id is required",
            ),
            (
                serde_json::json!({
                    "commit_id": "commit-abc",
                    "station_id": "station-1",
                    "timestamp": 1_700_000_000_i64,
                    "contribution_type": "  "
                }),
                "contribution_type is required",
            ),
        ];

        for (idx, (payload, expected_msg)) in cases.iter().enumerate() {
            let tx = build_launch_tx(
                TransactionType::ContributionCommit,
                payload.to_string(),
                &format!("empty-field-{idx}"),
            );
            let err = mempool
                .add_transaction(tx)
                .expect_err(&format!("case {idx} should be rejected"));
            assert!(
                err.to_string().contains(expected_msg),
                "case {idx}: expected '{expected_msg}', got '{err}'"
            );
        }
    }

    // ── Partition-mode gate (#1217) ────────────────────────────────────

    /// Build a minimal structurally-valid transaction for gate tests.
    ///
    /// Uses the existing `build_launch_tx` helper with a valid
    /// ContributionCommit payload so the tx passes every non-partition
    /// check. Confirms the partition gate is the *first* failure point.
    fn gate_test_tx(tag: &str) -> Transaction {
        let payload = build_valid_payload(TransactionType::ContributionCommit, 0, tag);
        build_launch_tx(TransactionType::ContributionCommit, payload, tag)
    }

    #[test]
    fn mempool_defaults_to_normal_partition_mode() {
        let mempool = Mempool::new(&Config::default());
        assert_eq!(
            mempool.partition_mode(),
            PartitionMode::Normal,
            "freshly-constructed mempool must be in Normal mode for backward \
             compatibility with callers that don't wire up the orchestrator"
        );
    }

    #[test]
    fn add_transaction_accepted_under_normal_mode() {
        let mut mempool = Mempool::new(&Config::default());
        mempool.set_partition_mode(PartitionMode::Normal);
        let tx = gate_test_tx("normal-gate");
        mempool
            .add_transaction(tx)
            .expect("Normal mode must accept valid writes");
    }

    #[test]
    fn add_transaction_accepted_under_block_production_only_mode() {
        // Middle-partition nodes can still produce (and therefore still
        // ingest into) blocks — just no finality votes. The mempool
        // must remain open for them.
        let mut mempool = Mempool::new(&Config::default());
        mempool.set_partition_mode(PartitionMode::BlockProductionOnly);
        let tx = gate_test_tx("middle-gate");
        mempool
            .add_transaction(tx)
            .expect("BlockProductionOnly mode must still accept writes");
    }

    #[test]
    fn add_transaction_refused_under_read_only_mode() {
        // The core #1217 invariant: a minority partition must refuse
        // new writes so it does not produce state that will be reverted
        // on reconnection.
        let mut mempool = Mempool::new(&Config::default());
        mempool.set_partition_mode(PartitionMode::ReadOnly);
        let tx = gate_test_tx("read-only-gate");
        let err = mempool
            .add_transaction(tx)
            .expect_err("ReadOnly mode must refuse new writes");
        let msg = err.to_string();
        assert!(
            msg.contains("read-only"),
            "error must mention read-only state, got: {msg}"
        );
        assert!(
            msg.contains("minority"),
            "error must mention minority partition, got: {msg}"
        );
    }

    #[test]
    fn read_only_gate_runs_before_other_validation() {
        // A transaction that would otherwise fail validation for a
        // different reason (empty field, replay, double-spend, etc.)
        // should still be rejected with the partition-mode message when
        // the node is in ReadOnly. This confirms the gate is the first
        // check and the minority node does zero validation work on
        // doomed writes.
        let mut mempool = Mempool::new(&Config::default());
        mempool.set_partition_mode(PartitionMode::ReadOnly);

        // Intentionally-broken payload: missing station_id field.
        let broken_payload = serde_json::json!({
            "commit_id": "commit-broken",
            "timestamp": 1_700_000_000_i64,
            "contribution_type": "contribution"
        })
        .to_string();
        let tx = build_launch_tx(
            TransactionType::ContributionCommit,
            broken_payload,
            "broken-under-readonly",
        );

        let err = mempool
            .add_transaction(tx)
            .expect_err("ReadOnly should reject regardless of other errors");
        // The partition-mode message must win over whatever
        // validation would have said.
        assert!(
            err.to_string().contains("read-only"),
            "ReadOnly gate must run first, got: {err}"
        );
    }

    #[test]
    fn partition_mode_transitions_logged_but_setter_is_idempotent() {
        let mut mempool = Mempool::new(&Config::default());
        // Setting the same mode repeatedly must not flip-flop state.
        mempool.set_partition_mode(PartitionMode::Normal);
        assert_eq!(mempool.partition_mode(), PartitionMode::Normal);
        mempool.set_partition_mode(PartitionMode::Normal);
        assert_eq!(mempool.partition_mode(), PartitionMode::Normal);

        mempool.set_partition_mode(PartitionMode::ReadOnly);
        assert_eq!(mempool.partition_mode(), PartitionMode::ReadOnly);
        mempool.set_partition_mode(PartitionMode::ReadOnly);
        assert_eq!(mempool.partition_mode(), PartitionMode::ReadOnly);

        // Recovery path: minority → middle → normal as partition heals.
        mempool.set_partition_mode(PartitionMode::BlockProductionOnly);
        assert_eq!(mempool.partition_mode(), PartitionMode::BlockProductionOnly);
        mempool.set_partition_mode(PartitionMode::Normal);
        assert_eq!(mempool.partition_mode(), PartitionMode::Normal);
    }

    // ── #1598: mempool replay protection — duplicate nonces under contention ──
    //
    // In the UTXO-style transaction model a "(sender, nonce)" slot is realised
    // as the input outpoint a transaction spends: two transactions from the same
    // sender reusing the same nonce necessarily reference the same predecessor
    // output. The mempool's `double_spends` index is therefore the surface that
    // enforces replay protection at admission time. These tests pin the
    // exactly-one-accepted invariant under a burst of conflicting submissions,
    // confirm no pending slot is leaked, and exercise the concurrent contention
    // path for panics/deadlocks. This surface is independent of the producer/
    // applier state-mutation unification (#1682) — it lives entirely in mempool
    // admission, before any block is built.

    /// Build a structurally-valid regular transaction that spends a specific
    /// input outpoint (`outpoint_tag`) while producing a distinct output
    /// (`out_tag`). Distinct outputs give each transaction a distinct tx_id,
    /// so the mempool cannot reject duplicates via the "already in mempool"
    /// id check — the conflict must be caught by double-spend / replay
    /// detection on the shared outpoint.
    fn build_tx_spending_outpoint(outpoint_tag: &str, out_tag: &str) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("nonce-slot-{outpoint_tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                // Unique amount + address per variant → unique tx_id.
                amount: 1,
                address: format!("wrf-replay-{out_tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.fee = 1_000;
        tx.set_id();
        tx
    }

    /// #2700 — `MEMPOOL_CAPACITY` must actually reject, not merely be declared.
    ///
    /// Before this ceiling was wired the pool was bounded in BYTES
    /// (`mempool_max_size_bytes`) and per SENDER (`mempool_max_txs_per_sender`)
    /// and never in total COUNT, so the 50_000-entry ceiling the protocol spec
    /// advertises was enforced by nothing. This asserts the BOUNDARY, not just
    /// that a rejection happens somewhere: the 50_000th admission must succeed
    /// and the 50_001st must fail, with the refusal attributable to capacity.
    ///
    /// The other two caps are deliberately raised out of the way so that what
    /// rejects here can only be the count ceiling. A test that let the byte cap
    /// or the per-sender cap fire first would pass while proving nothing.
    #[test]
    fn mempool_capacity_rejects_the_transaction_past_the_ceiling() {
        let mut config = Config::default();
        config.mempool_max_size_bytes = usize::MAX;
        config.mempool_max_txs_per_sender = usize::MAX;
        config.mempool_min_fee_rate = 0;
        let mut mempool = Mempool::new(&config);

        for i in 0..MEMPOOL_CAPACITY {
            let tx = build_tx_spending_outpoint(&format!("cap-{i}"), &format!("cap-{i}"));
            mempool
                .add_transaction(tx)
                .unwrap_or_else(|e| panic!("admission {i} below the ceiling must succeed, got: {e}"));
        }
        assert_eq!(
            mempool.size(),
            MEMPOOL_CAPACITY,
            "the pool must hold exactly the ceiling before the next admission"
        );

        // The 50_001st. This is the assertion the ceiling exists for.
        let overflow = build_tx_spending_outpoint("cap-overflow", "cap-overflow");
        let err = mempool
            .add_transaction(overflow)
            .expect_err("a transaction past MEMPOOL_CAPACITY must be rejected");
        assert!(
            err.to_string().contains("mempool is at capacity"),
            "the refusal must be attributable to the capacity ceiling, not an \
             unrelated cap; got: {err}"
        );

        // A rejected transaction must leave no trace — the cap is checked before
        // any mutation, so occupancy is unchanged.
        assert_eq!(
            mempool.size(),
            MEMPOOL_CAPACITY,
            "a rejected admission must not change occupancy"
        );
    }

    /// #2700 — pin the **operative** transaction-size limit so it cannot drift.
    ///
    /// READ THE NAME CAREFULLY: this test guards `config.max_transaction_size`
    /// (100 KB), **not** `MAX_TRANSACTION_DATA_SIZE` (2 MiB). The declared
    /// constant is INERT — no code consults it — and a test named after it would
    /// be exactly the false coverage #2700 exists to name: green, and guarding
    /// nothing.
    ///
    /// What is actually load-bearing is the config cap, which is ~20x tighter
    /// and fires first at the same call site. It is easy to miss because it does
    /// not mention `data`: it measures `tx.size()`, the whole serialized
    /// transaction, payload included. Nothing else in the tree pins its default,
    /// so a change from 100 KB to something far looser would pass CI silently
    /// while widening the only live bound on submitted transaction size.
    #[test]
    fn operative_transaction_size_cap_is_100kb_and_is_what_rejects() {
        // 1. Pin the default. This is the drift guard.
        let config = Config::default();
        assert_eq!(
            config.max_transaction_size, 100_000,
            "the operative transaction-size cap changed. This is the only live bound on \
             submitted transaction size (MAX_TRANSACTION_DATA_SIZE is inert, #2700). If the \
             change is intended, update docs/security/PROTOCOL_SPEC.md and RESOURCE_CEILINGS.md, \
             which both quote 100 KB"
        );

        // 2. Prove it is what rejects an oversized transaction — and prove the
        //    declared ceiling is NOT, by using a payload that exceeds the config
        //    cap while sitting far below MAX_TRANSACTION_DATA_SIZE. If the
        //    declared ceiling were the operative one this would be admitted.
        assert!(
            config.max_transaction_size < crate::common::constants::MAX_TRANSACTION_DATA_SIZE,
            "this test's premise is that the config cap is the tighter of the two"
        );
        let mut mempool = Mempool::new(&config);
        let mut over = build_tx_spending_outpoint("op-cap-over", "op-cap-over");
        over.data = Some("x".repeat(config.max_transaction_size));
        over.set_id();
        let err = mempool
            .add_transaction(over)
            .expect_err("a transaction past the operative cap must be rejected");
        assert!(
            err.to_string().contains("Transaction exceeds maximum size"),
            "the refusal must come from the operative size cap, got: {err}"
        );

        // 3. Negative control. Without this the test would pass just as well if
        //    admission rejected everything, and would pin nothing.
        let mut under = build_tx_spending_outpoint("op-cap-under", "op-cap-under");
        under.data = Some("x".repeat(1_024));
        under.set_id();
        mempool
            .add_transaction(under)
            .expect("a small transaction must still be admitted");
    }

    #[test]
    fn duplicate_nonce_burst_accepts_exactly_one() {
        // 100 distinct transactions all spending the same outpoint (i.e. the
        // same (sender, nonce) slot) arrive in the same window. Exactly one
        // must be accepted; the remaining 99 must be cleanly rejected.
        let mut mempool = Mempool::new(&Config::default());

        let mut accepted = 0usize;
        let mut rejected = 0usize;
        for i in 0..100 {
            let tx = build_tx_spending_outpoint("burst", &format!("v{i}"));
            match mempool.add_transaction(tx) {
                Ok(()) => accepted += 1,
                Err(e) => {
                    rejected += 1;
                    assert!(
                        e.to_string().contains("Double spend detected"),
                        "duplicate-nonce rejection must be the double-spend path, got: {e}"
                    );
                },
            }
        }

        assert_eq!(accepted, 1, "exactly one transaction may claim the nonce slot");
        assert_eq!(rejected, 99, "every conflicting submission must be rejected");
        assert_eq!(
            mempool.size(),
            1,
            "mempool must hold exactly one tx — no leaked pending slots after the burst"
        );
    }

    #[test]
    fn identical_transaction_replay_is_rejected_idempotently() {
        // The exact-replay path: the same transaction (same tx_id) submitted
        // twice must be rejected the second time with the id-collision error,
        // and the mempool size must not grow.
        let mut mempool = Mempool::new(&Config::default());
        let tx = build_tx_spending_outpoint("identical", "only");
        let replay = tx.clone();

        mempool
            .add_transaction(tx)
            .expect("first submission must be accepted");
        let err = mempool
            .add_transaction(replay)
            .expect_err("identical replay must be rejected");
        assert!(
            err.to_string().contains("already in mempool"),
            "exact replay must be caught by the id-collision check, got: {err}"
        );
        assert_eq!(mempool.size(), 1, "exact replay must not grow the mempool");
    }

    #[test]
    fn nonce_slot_is_freed_after_removal_no_leak() {
        // After the winning transaction is removed (e.g. mined into a block or
        // expired), the outpoint/nonce slot must be released so a subsequent
        // transaction can legitimately spend it. This guards against a leaked
        // double-spend entry that would permanently brick the slot.
        let mut mempool = Mempool::new(&Config::default());

        let first = build_tx_spending_outpoint("freed", "first");
        let first_id = first.id.unwrap();
        mempool.add_transaction(first).expect("first accepted");

        // While the first occupies the slot, a conflicting tx is rejected.
        let blocked = build_tx_spending_outpoint("freed", "blocked");
        assert!(
            mempool.add_transaction(blocked).is_err(),
            "slot must be occupied while the first tx is resident"
        );

        // Remove the winner; the slot must free up.
        assert!(mempool.remove_transaction(&first_id), "winner must be removed");
        assert_eq!(mempool.size(), 0, "mempool must be empty after removal");

        let successor = build_tx_spending_outpoint("freed", "successor");
        mempool
            .add_transaction(successor)
            .expect("slot must be reusable after the prior occupant is removed");
        assert_eq!(mempool.size(), 1, "successor must occupy the freed slot");
    }

    #[test]
    fn concurrent_duplicate_nonce_contention_accepts_exactly_one() {
        // Drive the contention path concurrently: N threads each try to claim
        // the same nonce slot with a distinct transaction. The exactly-one
        // invariant must hold and the run must complete without panic or
        // deadlock. (add_transaction is &mut self, so a shared Mutex serialises
        // the writers — this asserts determinism under interleaved access, the
        // realistic shape for a retry-storm / flapping-sensor burst.)
        use std::sync::{Arc, Mutex};
        use std::thread;

        let mempool = Arc::new(Mutex::new(Mempool::new(&Config::default())));
        let threads = 16usize;

        let handles: Vec<_> = (0..threads)
            .map(|i| {
                let mempool = Arc::clone(&mempool);
                thread::spawn(move || {
                    let tx = build_tx_spending_outpoint("concurrent", &format!("t{i}"));
                    let mut guard = mempool.lock().expect("mempool lock must not be poisoned");
                    guard.add_transaction(tx).is_ok()
                })
            })
            .collect();

        let accepted = handles
            .into_iter()
            .map(|h| h.join().expect("worker thread must not panic"))
            .filter(|ok| *ok)
            .count();

        assert_eq!(
            accepted, 1,
            "exactly one concurrent writer may win the nonce slot"
        );
        assert_eq!(
            mempool.lock().unwrap().size(),
            1,
            "mempool must hold exactly one tx after concurrent contention"
        );
    }

    // ── #1607: fee priority-inversion stress tests ────────────────────────
    //
    // These tests cover the two remaining acceptance criteria from issue #1607
    // that were de-scoped from #1751 (nonce/replay surface) to keep it focused:
    //   1. 200-tx fee ordering stress — no priority inversion under burst load
    //   2. Dust-fee starvation — a high-fee tx must not be buried by a dust burst
    //
    // The higher-fee double-spend / RBF edge is already covered by
    // `adversarial_double_spend_higher_fee_cannot_bypass_rejection` (merged #1656).

    #[test]
    fn fee_ordering_stress_200_txs_no_priority_inversion() {
        // Issue #1607 AC: 200 txs with fees spanning 3 orders of magnitude
        // (500..=100_000) submitted in pseudo-random (hash-sorted) order.
        // get_transactions_with_limit must return them in non-increasing fee_rate
        // order. Ties in fee_rate are broken by non-increasing absolute fee
        // (guaranteed by the secondary Reverse<abs_fee> BTreeMap key added in this
        // fix). We do NOT assert absolute-fee ordering across the full 200-tx
        // result because JSON serialization of the transaction id hash (a
        // variable-length [u8;32] array) makes serialized sizes vary per tx,
        // so fee_rate is not strictly proportional to absolute fee.
        let mut mempool = Mempool::new(&Config::default());
        let n: usize = 200;

        let mut txs: Vec<Transaction> = (0..n)
            .map(|i| build_regular_tx_with_fee((i as u64 + 1) * 500, &format!("stress-{i:03}")))
            .collect();

        // Deterministic non-monotonic submission order: sort by tx_id hash.
        txs.sort_by_key(|tx| tx.id.unwrap());

        for tx in &txs {
            mempool
                .add_transaction(tx.clone())
                .expect("stress tx must be accepted");
        }
        assert_eq!(mempool.size(), n, "all {n} txs must be in mempool");

        let result = mempool.get_transactions_with_limit(usize::MAX);
        assert_eq!(result.len(), n, "all {n} txs must be returned");

        // Compute (fee_rate, abs_fee) for each result tx — mirror the BTreeMap key
        // (milli-units/byte scaling per #2026).
        let key = |tx: &Transaction| -> (u64, u64) {
            let size = tx.size() as u64;
            let rate = tx.fee.saturating_mul(1000) / size.max(1);
            (rate, tx.fee)
        };

        // Verify the result is in non-increasing (fee_rate, abs_fee) order —
        // that is, no tx at position i has a strictly greater key than position i-1.
        for i in 1..result.len() {
            let prev = key(&result[i - 1]);
            let cur = key(&result[i]);
            assert!(
                prev >= cur,
                "priority inversion at position {i}: \
                 (rate={},fee={}) > (rate={},fee={}) at position {}",
                cur.0,
                cur.1,
                prev.0,
                prev.1,
                i - 1
            );
        }
    }

    #[test]
    fn dust_fee_burst_does_not_starve_high_fee_tx() {
        // Issue #1607 AC: a burst of 100 dust-fee (fee=1) transactions submitted
        // before a single high-fee (fee=100_000) transaction must not cause the
        // high-fee tx to appear after any dust tx in the inclusion order.
        let mut mempool = Mempool::new(&Config::default());

        // Flood the mempool with dust-fee txs first.
        for i in 0..100usize {
            let tx = build_regular_tx_with_fee(1, &format!("dust-{i:03}"));
            mempool
                .add_transaction(tx)
                .expect("dust tx must be accepted");
        }

        // Then submit the high-fee tx — it arrives last but must be served first.
        let high = build_regular_tx_with_fee(100_000, "high-starvation");
        let high_id = high.id.unwrap();
        mempool
            .add_transaction(high)
            .expect("high-fee tx must be accepted");

        assert_eq!(mempool.size(), 101, "mempool must hold all 101 txs");

        let result = mempool.get_transactions_with_limit(usize::MAX);
        assert_eq!(result.len(), 101, "all 101 txs must be returned");

        // High-fee tx must be first — dust burst must not cause starvation.
        assert_eq!(
            result[0].id.unwrap(),
            high_id,
            "high-fee tx must be first in inclusion order; dust burst must not cause starvation"
        );

        // All remaining txs must be dust-fee.
        for tx in &result[1..] {
            assert_eq!(tx.fee, 1, "remaining txs after high-fee must all be dust-fee");
        }
    }

    /// Regression test for #1847: `drain_confirmed_block_txs` removes the
    /// transaction and its replay-protection key so replay keys do not
    /// accumulate unboundedly when blocks are applied via the P2P path.
    #[test]
    fn drain_confirmed_block_txs_removes_tx_and_replay_key_regression_1847() {
        let config = Config::default();
        let mut mempool = Mempool::new(&config);

        // Build a signed ContributionCommit tx (generates a protocol_replay_key on admission).
        let payload = build_valid_payload(TransactionType::ContributionCommit, 0, "drain-1847");
        let tx = build_launch_tx(TransactionType::ContributionCommit, payload, "drain-1847");
        let tx_id = tx.id.expect("set_id must assign an id");

        mempool
            .add_transaction(tx)
            .expect("valid commit must be admitted to mempool");

        assert_eq!(mempool.size(), 1, "mempool must contain the tx before drain");
        assert!(
            mempool.get_transaction(&tx_id).is_some(),
            "tx must be retrievable before drain"
        );

        // Simulate the confirmed block containing this tx.
        mempool.drain_confirmed_block_txs(&[tx_id]);

        assert_eq!(
            mempool.size(),
            0,
            "mempool must be empty after draining the confirmed tx (#1847)"
        );
        assert!(
            mempool.get_transaction(&tx_id).is_none(),
            "confirmed tx must not be retrievable after drain (#1847)"
        );

        // After drain the replay key is cleared — a re-submission with the same
        // commit_id is accepted back into the mempool (the blockchain's own ledger
        // state rejects it at inclusion; mempool-level dedup is cleared by drain).
        let payload2 =
            build_valid_payload(TransactionType::ContributionCommit, 0, "drain-1847");
        let tx2 = build_launch_tx(TransactionType::ContributionCommit, payload2, "drain-1847");
        assert!(
            mempool.add_transaction(tx2).is_ok(),
            "replay key must be cleared after drain so the tx can re-enter the mempool (#1847)"
        );
    }

    // ── #1735: purge_older_than_secs tests ───────────────────────────────────
    //
    // `purge_older_than_secs` is the operator-facing eviction path (call it with
    // a runtime age limit). These tests verify it removes the transaction AND
    // clears the protocol replay key so that a re-submission is accepted.

    /// `purge_older_than_secs(0)` must evict all transactions immediately, return
    /// the correct count, and clear the replay keys so re-submission is accepted.
    ///
    /// This is distinct from `clean_old_transactions` (which uses a stored
    /// `max_age`). `purge_older_than_secs` is the operator-invoked path.
    #[test]
    fn purge_older_than_secs_zero_removes_all_and_clears_replay_key() {
        let config = Config::default();
        let mut mempool = Mempool::new(&config);

        // Add a signed ContributionCommit — it acquires a protocol replay key on admission.
        let payload = build_valid_payload(TransactionType::ContributionCommit, 0, "purge-zero");
        let tx = build_launch_tx(TransactionType::ContributionCommit, payload.clone(), "purge-zero");
        mempool
            .add_transaction(tx)
            .expect("valid commit must be admitted");

        // Duplicate should be rejected before purge (replay key is held).
        let dup = build_launch_tx(TransactionType::ContributionCommit, payload.clone(), "purge-zero-dup");
        assert!(
            mempool.add_transaction(dup).is_err(),
            "duplicate commit must be rejected while replay key is held"
        );

        assert_eq!(mempool.size(), 1, "mempool must hold 1 tx before purge");

        // purge_older_than_secs(0) — every instant is ≥ 0 seconds old.
        let evicted = mempool.purge_older_than_secs(0);
        assert_eq!(evicted, 1, "exactly 1 tx should be evicted");
        assert_eq!(mempool.size(), 0, "mempool must be empty after purge");

        // Replay key is cleared — the same payload can now re-enter the mempool.
        let resubmit = build_launch_tx(TransactionType::ContributionCommit, payload, "purge-zero-resubmit");
        assert!(
            mempool.add_transaction(resubmit).is_ok(),
            "replay key must be cleared after purge so re-submission is accepted"
        );
    }

    /// `purge_older_than_secs` with a large max_age (u64::MAX seconds) must not
    /// remove any recently-added transactions (they are not yet expired).
    #[test]
    fn purge_older_than_secs_large_max_age_removes_nothing() {
        let config = Config::default();
        let mut mempool = Mempool::new(&config);

        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("purge-max-input"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 100,
                address: "wrf-purge-max".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.set_id();
        mempool.add_transaction(tx).expect("should accept tx");
        assert_eq!(mempool.size(), 1);

        // u64::MAX seconds in the future — nothing should be evicted.
        let evicted = mempool.purge_older_than_secs(u64::MAX);
        assert_eq!(evicted, 0, "purge with u64::MAX max_age must not evict fresh txs");
        assert_eq!(mempool.size(), 1, "mempool must still hold the tx");
    }

    /// After a `purge_older_than_secs(0)`, the mempool double-spend map
    /// is also cleared, so the same input can be re-spent by a new transaction.
    #[test]
    fn purge_older_than_secs_clears_double_spend_tracking() {
        let config = Config::default();
        let mut mempool = Mempool::new(&config);

        let shared_txid = hash_string("shared-utxo-purge");

        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: shared_txid,
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 50,
                address: "wrf-purge-ds".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.set_id();
        mempool.add_transaction(tx).expect("first spend must succeed");

        // Immediately rejected because the input is already tracked.
        let mut tx2 = Transaction::new(
            vec![TransactionInput {
                txid: shared_txid,
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 50,
                address: "wrf-purge-ds-2".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx2.set_id();
        assert!(
            mempool.add_transaction(tx2.clone()).is_err(),
            "double spend must be rejected before purge"
        );

        // Purge all transactions.
        mempool.purge_older_than_secs(0);
        assert_eq!(mempool.size(), 0);

        // Now the same input can be spent again (double-spend map cleared).
        assert!(
            mempool.add_transaction(tx2).is_ok(),
            "double-spend map must be cleared after purge"
        );
    }

    // ── #1735 slice 2: Stake/Unstake replay-key and NFT gap tests ────────────
    //
    // Stake and Unstake transactions generate a protocol replay key at admission
    // (`"stake:{account}:{amount}:{nonce}"`).  A second Stake tx with the same
    // tuple — but a different UTXO input (so the double-spend check does not
    // fire) — must be caught by the replay-key check.
    //
    // NftCreation falls to `_ => Ok(None)` in `validate_launch_payload_and_replay_key`
    // (no protocol replay key), so two NFT txs with different inputs are BOTH
    // admitted.  That is intentional (multiple NFTs per block are valid), and
    // replay protection is provided at confirmation time by the blockchain ledger.
    // These tests PIN that behavior so we notice if it ever changes.

    /// Build a signed Stake or Unstake transaction for the given account key.
    ///
    /// `nonce` is the tx-level nonce that is both embedded in the signed message
    /// and set on the returned transaction.  Using the same `nonce` + `amount`
    /// for two transactions with different inputs produces the same replay key.
    fn make_signed_stake_tx(
        account_key: &PrivateKey,
        amount: u64,
        nonce: u64,
        is_unstake: bool,
        tag: &str,
    ) -> Transaction {
        let account_hex =
            Address::from_public_key(&PublicKey::from_private_key(account_key)).to_hex();
        let mut payload = StakeAuthPayload {
            account: account_hex,
            amount,
            auth_pubkey: None,
            auth_signature: None,
        };
        payload.sign_authorization(account_key, is_unstake, nonce);
        let payload_json = serde_json::to_string(&payload).expect("serialize stake payload");

        let tx_type = if is_unstake {
            TransactionType::Unstake
        } else {
            TransactionType::Stake
        };
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("stake-input-{tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-stake-out-{tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = tx_type;
        tx.data = Some(payload_json);
        tx.nonce = nonce;
        tx.set_id();
        tx
    }

    /// Build a valid, `from`-authorized transparent transfer (#1727) with a
    /// unique input outpoint per `tag`, mirroring `make_signed_stake_tx`.
    fn make_signed_transparent_transfer(
        from_key: &PrivateKey,
        to_hex: &str,
        amount: u64,
        nonce: u64,
        tag: &str,
    ) -> Transaction {
        use crate::blockchain::transaction::TransparentTransferPayload;
        let from_hex = Address::from_public_key(&PublicKey::from_private_key(from_key)).to_hex();
        let mut payload = TransparentTransferPayload {
            from: from_hex.clone(),
            to: to_hex.to_string(),
            amount,
            transparent_opt_out: true,
            opt_out_audit_tag: None,
            auth_pubkey: None,
            auth_signature: None,
            authority: None,
            authority_signature: None,
            passkey_auth: None,
            sponsorship: None,
            session_key_auth: None,
        };
        payload.sign_authorization(from_key, nonce);
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("transfer-input-{tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: from_hex,
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::Transfer;
        tx.data = Some(serde_json::to_string(&payload).expect("serialize transfer payload"));
        tx.nonce = nonce;
        tx.set_id();
        tx
    }

    /// #2023: two transparent transfers with the same (from, nonce) produce the
    /// same `transfer:{from}:{nonce}` replay key; the second must be rejected
    /// even though it spends a different UTXO input (fresh per-submission txid) —
    /// exactly how production resubmits bypass the outpoint double-spend index
    /// and flood the relay layer.
    #[test]
    fn transfer_duplicate_replay_key_rejected_at_mempool() {
        let mut rng = StdRng::seed_from_u64(0x2023_5001);
        let from_key = PrivateKey::generate(&mut rng);
        let to_hex =
            Address::from_public_key(&PublicKey::from_private_key(&PrivateKey::generate(&mut rng)))
                .to_hex();

        let first = make_signed_transparent_transfer(&from_key, &to_hex, 10, 0, "dup-1");
        // Different tag → different input txid → different tx_id, so this passes
        // both the "already in mempool" and double-spend checks.
        let second = make_signed_transparent_transfer(&from_key, &to_hex, 10, 0, "dup-2");
        assert_ne!(first.id, second.id, "test setup: tx ids must differ");

        let mut mp = Mempool::new(&Config::default());
        mp.add_transaction(first).expect("first transfer admitted");
        let err = mp
            .add_transaction(second)
            .expect_err("duplicate (from, nonce) transfer must be rejected");
        assert!(
            err.to_string().contains("Launch protocol replay detected"),
            "unexpected error: {err}"
        );
    }

    /// #2024: a non-transparent `Transfer` carrying an input without a valid
    /// ed25519 signature must be rejected at admission (previously it passed on
    /// structure alone and was gossiped, only to be rejected at block time).
    #[test]
    fn non_transparent_transfer_with_unsigned_input_rejected() {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("unsigned-transfer-input"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: "wrf-unsigned-out".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::Transfer;
        tx.nonce = 0;
        tx.set_id();

        let mut mp = Mempool::new(&Config::default());
        let err = mp
            .add_transaction(tx)
            .expect_err("non-transparent transfer with unsigned input must be rejected");
        assert!(
            err.to_string().contains("invalid input signature"),
            "unexpected error: {err}"
        );
    }

    /// #2024 positive control: a non-transparent `Transfer` whose input carries a
    /// valid ed25519 signature over the signing hash must still be admitted — the
    /// new gate must not reject legitimate transfers.
    #[test]
    fn non_transparent_transfer_with_valid_input_signature_admitted() {
        let mut rng = StdRng::seed_from_u64(0x2024_5001);
        let key = PrivateKey::generate(&mut rng);
        let pk_hex = PublicKey::from_private_key(&key).to_hex();

        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("signed-transfer-input"),
                vout: 0,
                signature: None,
                public_key: Some(pk_hex),
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: "wrf-signed-out".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::Transfer;
        tx.nonce = 0;
        // Sign the signing hash (which excludes the input `signature` field),
        // mirroring how a wallet signs a UTXO input.
        let msg = tx.signing_hash();
        let sig = key.sign(msg.as_bytes());
        tx.inputs[0].signature = Some(sig.to_hex());
        tx.set_id();

        let mut mp = Mempool::new(&Config::default());
        mp.add_transaction(tx)
            .expect("transfer with valid input signature must be admitted");
    }

    #[test]
    fn stake_admission_signed_rewrapped_copy_is_refused() {
        // Two Stake txs with the same (account, amount, nonce) produce the same
        // protocol replay key even across different inputs. Interim policy
        // refuses BOTH rather than relying on a confirmation-scoped pool key.
        let mut rng = StdRng::seed_from_u64(0x1735_5001);
        let account_key = PrivateKey::generate(&mut rng);

        let first = make_signed_stake_tx(&account_key, 1_000, 0, false, "stake-replay-1");
        // Different tag → different input txid → different tx_id, bypassing
        // the double-spend check. Policy must stop even the first authorization.
        let second = make_signed_stake_tx(&account_key, 1_000, 0, false, "stake-replay-2");

        let mut mempool = Mempool::new(&Config::default());
        assert_eq!(
            mempool.launch_replay_key(&first).unwrap(),
            mempool.launch_replay_key(&second).unwrap()
        );
        for tx in [first, second] {
            assert!(tx.verify_stake_authorization());
            let err = mempool
                .add_transaction(tx)
                .expect_err("signed Stake admission is disabled");
            assert!(err.to_string().contains("disabled for local admission"));
            assert_eq!(mempool.size(), 0);
            assert!(mempool.protocol_replay_keys.is_empty());
            assert!(mempool.per_sender_count.is_empty());
        }
    }

    #[test]
    fn unstake_admission_signed_rewrapped_copy_is_refused() {
        // Same as above but for Unstake.  Replay key is `"unstake:account:amount:nonce"`.
        let mut rng = StdRng::seed_from_u64(0x1735_5002);
        let account_key = PrivateKey::generate(&mut rng);

        let first = make_signed_stake_tx(&account_key, 500, 0, true, "unstake-replay-1");
        let second = make_signed_stake_tx(&account_key, 500, 0, true, "unstake-replay-2");

        let mut mempool = Mempool::new(&Config::default());
        assert_eq!(
            mempool.launch_replay_key(&first).unwrap(),
            mempool.launch_replay_key(&second).unwrap()
        );
        for tx in [first, second] {
            assert!(tx.verify_stake_authorization());
            let err = mempool
                .add_transaction(tx)
                .expect_err("signed Unstake admission is disabled");
            assert!(err.to_string().contains("disabled for local admission"));
            assert_eq!(mempool.size(), 0);
            assert!(mempool.protocol_replay_keys.is_empty());
            assert!(mempool.per_sender_count.is_empty());
        }
    }

    #[test]
    fn stake_admission_signed_different_nonces_are_refused() {
        // Different nonces retain distinct historical pool-key representation,
        // but neither can bypass the interim local admission refusal.
        let mut rng = StdRng::seed_from_u64(0x1735_5003);
        let account_key = PrivateKey::generate(&mut rng);

        let nonce0 = make_signed_stake_tx(&account_key, 1_000, 0, false, "distinct-nonce-0");
        let nonce1 = make_signed_stake_tx(&account_key, 1_000, 1, false, "distinct-nonce-1");

        let mut mempool = Mempool::new(&Config::default());
        assert_ne!(
            mempool.launch_replay_key(&nonce0).unwrap(),
            mempool.launch_replay_key(&nonce1).unwrap()
        );
        for tx in [nonce0, nonce1] {
            assert!(tx.verify_stake_authorization());
            let err = mempool
                .add_transaction(tx)
                .expect_err("new nonce cannot bypass interim admission policy");
            assert!(err.to_string().contains("disabled for local admission"));
        }
        assert_eq!(mempool.size(), 0);
        assert!(mempool.protocol_replay_keys.is_empty());
        assert!(mempool.per_sender_count.is_empty());
    }

    #[test]
    fn nft_creation_has_no_protocol_replay_key() {
        // NftCreation falls to `_ => Ok(None)` in `validate_launch_payload_and_replay_key`,
        // so it has NO protocol-level replay key.  Two NftCreation txs spending
        // DIFFERENT inputs must both be admitted — replay protection comes from
        // the blockchain ledger at confirmation time, not the mempool.
        //
        // This test PINS the current behavior.  If NftCreation ever gains a
        // protocol replay key (making it consistent with stake/oracle), this test
        // will fail and alert the developer to update the test intentionally.
        let mut mempool = Mempool::new(&Config::default());

        let mut nft1 = Transaction::new(
            vec![TransactionInput {
                txid: hash_string("nft-input-alpha"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: "wrf-nft-out-alpha".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        nft1.tx_type = TransactionType::NftCreation;
        nft1.set_id();

        let mut nft2 = Transaction::new(
            vec![TransactionInput {
                // Different input → different tx_id → passes double-spend check.
                txid: hash_string("nft-input-beta"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: "wrf-nft-out-beta".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        nft2.tx_type = TransactionType::NftCreation;
        nft2.set_id();

        mempool
            .add_transaction(nft1)
            .expect("first NftCreation must be admitted (no protocol replay key)");
        mempool
            .add_transaction(nft2)
            .expect("second NftCreation with different input must also be admitted — \
                     NftCreation has no protocol replay key, only UTXO-level dedup");
        assert_eq!(
            mempool.size(),
            2,
            "both NftCreation txs must be in mempool (no protocol replay key for NftCreation)"
        );
    }

    // ── #1735: eviction-path replay-key clearing ──────────────────────────────
    //
    // Three eviction paths each have their own inline `protocol_replay_keys.remove`
    // call: `remove_transaction` (size-pressure), `clean_old_transactions` (max-age
    // maintenance), and `purge_older_than_secs` (operator-invoked).  The last path
    // is covered by `purge_older_than_secs_zero_removes_all_and_clears_replay_key`.
    // The first two paths are pinned below.

    /// `evict_transactions` (triggered by size pressure inside `add_transaction`)
    /// must clear the protocol replay key when it evicts a ContributionCommit tx.
    ///
    /// Approach: set max_size = cc_size + filler_size so that adding a third tx
    /// MUST trigger eviction.  The CC tx (fee=0) is the lowest-fee entry and is
    /// evicted first.  Re-submitting the same commit after eviction must succeed
    /// because `remove_transaction` cleared the replay key.
    #[test]
    fn size_pressure_eviction_clears_commit_replay_key() {
        // Build all three transactions up-front so we can size the pool precisely.
        let cc_payload =
            build_valid_payload(TransactionType::ContributionCommit, 0, "evict-rk");
        let cc_tx = build_launch_tx(
            TransactionType::ContributionCommit,
            cc_payload.clone(),
            "evict-rk",
        );
        let cc_size = cc_tx.size();
        let cc_id = cc_tx.id.expect("cc tx must have an id");

        let filler = build_regular_tx_with_fee(1, "evict-rk-filler");
        let filler_size = filler.size();

        // Pool holds exactly cc_size + filler_size bytes.
        // Adding any further tx will overflow and trigger evict_transactions.
        let mut config = Config::default();
        config.mempool_max_size_bytes = cc_size + filler_size;
        let mut mempool = Mempool::new(&config);

        // Admit CC tx (fee=0) — protocol replay key is recorded.
        mempool
            .add_transaction(cc_tx)
            .expect("CC tx must be admitted into an empty mempool");

        // Fill the pool to capacity with a low-fee tx.
        mempool
            .add_transaction(filler)
            .expect("filler tx must fit into the remaining pool capacity");
        assert_eq!(mempool.size(), 2, "pool must hold exactly two transactions before overflow");

        // Add a high-fee tx.  Pool is full → evict_transactions fires.
        // CC tx (fee=0) is cheapest → it is evicted so the high-fee tx fits.
        let high = build_regular_tx_with_fee(100_000, "evict-rk-high");
        let high_id = high.id.unwrap();
        mempool
            .add_transaction(high)
            .expect("high-fee tx must be admitted after CC tx is evicted under size pressure");

        // CC tx must be gone; high-fee tx must be present.
        assert!(
            mempool.get_transaction(&cc_id).is_none(),
            "CC tx must have been evicted from the pool under size pressure (#1735)"
        );
        assert!(
            mempool.get_transaction(&high_id).is_some(),
            "high-fee tx must be in the pool after eviction (#1735)"
        );

        // The replay key was cleared by remove_transaction inside evict_transactions.
        // Re-submitting the same commit must now succeed.
        let cc_tx2 = build_launch_tx(
            TransactionType::ContributionCommit,
            cc_payload,
            "evict-rk",
        );
        mempool
            .add_transaction(cc_tx2)
            .expect("CC tx replay key must be cleared by size-pressure eviction — \
                     re-submission must succeed (#1735)");
    }

    // ── #1976: reverse-dependency index correctness ──────────────────────────
    //
    // `remove_transaction` (and thus `evict_transactions`) previously rescanned
    // every dependency set per removal — O(m×n). The fix maintains a `dependents`
    // reverse index so removal is O(d). These tests pin the behavior-preserving
    // contract: dependency edges are dropped on removal exactly as before, and
    // block-building still respects parent-before-child ordering after evictions.

    /// Builds a child tx whose sole input spends `parent_id` (vout 0), so the
    /// mempool records `child` as depending on `parent`.
    fn build_child_of(parent_id: Hash, fee: u64, out_tag: &str) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: parent_id,
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-child-{out_tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.fee = fee;
        tx.set_id();
        tx
    }

    /// Removing a parent must drop the dependency edge from every child that
    /// depended on it — exactly the semantics of the old O(m) scan, now served
    /// by the O(d) reverse index. After the parent is gone, the child has no
    /// remaining dependencies and so is independently includable.
    #[test]
    fn remove_parent_clears_child_dependency_edge() {
        let mut mempool = Mempool::new(&Config::default());

        let parent = build_regular_tx_with_fee(1_000, "dep-parent");
        let parent_id = parent.id.unwrap();
        mempool.add_transaction(parent).expect("parent must be admitted");

        let child = build_child_of(parent_id, 2_000, "a");
        let child_id = child.id.unwrap();
        mempool.add_transaction(child).expect("child must be admitted");

        // Before removal: child depends on parent, so it cannot be included
        // unless the parent is already in the selected set.
        assert!(
            !mempool.can_include_transaction(&child_id, &FxHashSet::default()),
            "child must depend on parent before parent removal"
        );
        assert_eq!(
            mempool.dependents.get(&parent_id).map(|s| s.len()),
            Some(1),
            "reverse index must record the child as a dependent of the parent"
        );

        // Remove the parent. The dependency edge to it must be stripped from the
        // child's dependency set, leaving the child with no dependencies.
        assert!(mempool.remove_transaction(&parent_id), "parent removal must succeed");

        assert!(
            mempool.can_include_transaction(&child_id, &FxHashSet::default()),
            "after parent removal the child must have no remaining dependencies"
        );
        assert!(
            !mempool.dependents.contains_key(&parent_id),
            "reverse index entry for the removed parent must be gone"
        );
        // The forward map must not retain a now-empty dependency set for the child.
        assert!(
            !mempool.dependencies.contains_key(&child_id),
            "child's emptied dependency set must be cleaned up"
        );
    }

    /// Eviction under size pressure must preserve dependency-aware block
    /// ordering. We fill the pool with a low-fee filler plus a parent→child
    /// chain, then add a high-fee tx that forces the filler out. The surviving
    /// parent/child must still be returned parent-before-child by the block
    /// builder, proving the reverse-index eviction kept dependency state intact.
    #[test]
    fn eviction_preserves_dependency_ordering() {
        // Build everything first so the pool can be sized precisely.
        // Parent has the higher fee so the block builder (which selects in
        // descending fee-rate order and includes a child only after its parent)
        // visits the parent first; both still clear the low-fee filler.
        let parent = build_regular_tx_with_fee(6_000, "evict-parent");
        let parent_id = parent.id.unwrap();
        let parent_size = parent.size();

        let child = build_child_of(parent_id, 5_000, "evict");
        let child_id = child.id.unwrap();
        let child_size = child.size();

        // Lowest-fee entry, and deliberately the largest tx (padded address) so
        // evicting this single filler frees more than enough room for `high` —
        // guaranteeing the higher-fee parent/child chain is never touched.
        let filler =
            build_regular_tx_with_fee(1, "evict-filler-padded-to-be-the-largest-tx-in-pool");
        let filler_size = filler.size();

        // High-fee tx forces eviction of the lowest-fee entry (the filler).
        let high = build_regular_tx_with_fee(100_000, "evict-high");
        let high_id = high.id.unwrap();
        let high_size = high.size();
        assert!(
            filler_size >= high_size,
            "test invariant: filler must be at least as large as the incoming high-fee tx \
             so a single eviction frees enough room"
        );

        let mut config = Config::default();
        config.mempool_max_size_bytes = parent_size + child_size + filler_size;
        let mut mempool = Mempool::new(&config);

        mempool.add_transaction(parent).expect("parent admitted");
        mempool.add_transaction(child).expect("child admitted");
        mempool.add_transaction(filler).expect("filler fills the pool");
        assert_eq!(mempool.size(), 3, "pool holds parent, child, filler before overflow");

        mempool.add_transaction(high).expect("high-fee tx admitted after eviction");

        assert!(mempool.get_transaction(&parent_id).is_some(), "parent must survive eviction");
        assert!(mempool.get_transaction(&child_id).is_some(), "child must survive eviction");
        assert!(mempool.get_transaction(&high_id).is_some(), "high-fee tx must be present");

        // Block builder must still yield parent strictly before child.
        let block = mempool.get_transactions_with_limit(usize::MAX);
        let parent_pos =
            block.iter().position(|t| *t.id.as_ref().unwrap() == parent_id).expect("parent in block");
        let child_pos =
            block.iter().position(|t| *t.id.as_ref().unwrap() == child_id).expect("child in block");
        assert!(
            parent_pos < child_pos,
            "parent must be selected before its child after eviction (dependency ordering preserved)"
        );
    }

    /// `clean_old_transactions` must clear the protocol replay key when it
    /// expires an old ContributionCommit tx.
    ///
    /// The existing `clean_old_transactions_evicts_expired` test uses a plain
    /// transaction (no protocol replay key).  This test pins the CC-specific
    /// code path (`protocol_replay_keys.remove` inside `clean_old_transactions`).
    #[test]
    fn clean_old_transactions_clears_commit_replay_key() {
        let mut config = Config::default();
        config.mempool_max_age_secs = 60; // 60-second max age
        let mut mempool = Mempool::new(&config);

        // Admit a ContributionCommit — replay key is recorded.
        let payload =
            build_valid_payload(TransactionType::ContributionCommit, 0, "age-evict-rk");
        let cc_tx = build_launch_tx(
            TransactionType::ContributionCommit,
            payload.clone(),
            "age-evict-rk",
        );
        let cc_id = cc_tx.id.expect("cc tx must have an id");
        mempool
            .add_transaction(cc_tx)
            .expect("CC tx must be admitted");

        // Duplicate is rejected while the replay key is still held.
        let dup = build_launch_tx(
            TransactionType::ContributionCommit,
            payload.clone(),
            "age-evict-rk-dup",
        );
        mempool
            .add_transaction(dup)
            .expect_err("duplicate commit must be rejected while CC tx is in the pool");

        // Backdate the CC tx timestamp to 1 hour ago — it is now considered expired.
        let old_ts = Instant::now() - Duration::from_secs(3_600);
        assert!(
            mempool.set_transaction_timestamp_for_test(&cc_id, old_ts),
            "timestamp backdate must succeed"
        );

        // clean_old_transactions must remove the expired CC tx and clear its replay key.
        let evicted = mempool.clean_old_transactions();
        assert_eq!(evicted, 1, "one expired transaction must be evicted");
        assert_eq!(mempool.size(), 0, "mempool must be empty after age eviction");
        assert!(
            mempool.get_transaction(&cc_id).is_none(),
            "expired CC tx must not be retrievable after clean_old_transactions"
        );

        // Replay key cleared → the same commit can be re-admitted.
        let cc_tx2 = build_launch_tx(
            TransactionType::ContributionCommit,
            payload,
            "age-evict-rk",
        );
        mempool
            .add_transaction(cc_tx2)
            .expect("CC replay key must be cleared by clean_old_transactions — \
                     re-submission must succeed (#1735)");
        assert_eq!(
            mempool.size(),
            1,
            "re-submitted CC tx must be present in the mempool (#1735)"
        );
    }

    // ── #1735 hardening: mempool fee-ordering edges ──────────────────────────

    /// Transactions that tie on both `fee_rate` and `abs_fee` must be ordered
    /// by txid ascending, regardless of insertion order.
    ///
    /// The fee-order BTreeMap key is `(Reverse(fee_rate), Reverse(abs_fee), txid)`.
    /// Within the same fee tier the third component (txid) is ascending, making
    /// the ordering deterministic across any insertion sequence.
    #[test]
    fn fee_tie_ordering_is_deterministic_regardless_of_insertion_order() {
        // Use fee=0 so all three transactions share exactly the same fee_rate
        // (0) and abs_fee (0), leaving txid as the only distinguishing sort key.
        // #2026 note: fee_rate is now milli-units/byte (fee*1000/size). Because a
        // Hash serializes as a variable-length JSON byte array, distinct txids
        // give distinct `tx.size()`, so equal *nonzero* fees no longer collapse
        // to an identical fee_rate the way the old coarse `fee/size` rate did.
        // fee=0 is the one size-independent tie (0*1000/size == 0 for any size),
        // which is exactly what this test needs to isolate the txid tie-break.
        let tx_a = build_regular_tx_with_fee(0, "a");
        let tx_b = build_regular_tx_with_fee(0, "b");
        let tx_c = build_regular_tx_with_fee(0, "c");

        let id_a = tx_a.id.unwrap();
        let id_b = tx_b.id.unwrap();
        let id_c = tx_c.id.unwrap();

        // Mempool 1: add in order a → b → c.
        let mut mp1 = Mempool::new(&Config::default());
        mp1.add_transaction(tx_a.clone()).unwrap();
        mp1.add_transaction(tx_b.clone()).unwrap();
        mp1.add_transaction(tx_c.clone()).unwrap();

        let order1: Vec<Hash> = mp1
            .iter_by_fee_order()
            .map(|tx| tx.id.unwrap())
            .collect();

        // Mempool 2: add in reverse order c → b → a.
        let mut mp2 = Mempool::new(&Config::default());
        mp2.add_transaction(tx_c).unwrap();
        mp2.add_transaction(tx_b).unwrap();
        mp2.add_transaction(tx_a).unwrap();

        let order2: Vec<Hash> = mp2
            .iter_by_fee_order()
            .map(|tx| tx.id.unwrap())
            .collect();

        // Both orderings must be identical (sorted by txid ascending within the tier).
        assert_eq!(
            order1, order2,
            "fee-tied transactions must be ordered by txid regardless of insertion order"
        );

        // Sanity: the ordering must be ascending by txid within the tie tier.
        let mut sorted = vec![id_a, id_b, id_c];
        sorted.sort();
        assert_eq!(
            order1, sorted,
            "fee-tied transactions must appear in ascending txid order"
        );
    }

    /// Eviction must stop as soon as the incoming transaction *fits*, not once
    /// it has freed `needed_size` bytes.
    ///
    /// Regression for the over-eviction fixed alongside this test. The old stop
    /// condition (`freed_size >= needed_size`) ignored headroom the pool already
    /// had, so an incoming transaction larger than the cheapest entry caused a
    /// second, unnecessary eviction of a fee-paying transaction.
    ///
    /// This case is deterministic where the size-pressure test below is not:
    /// `incoming` is given a long address so it is unconditionally larger than
    /// `cheap`, rather than relying on how two hashes happen to serialize. That
    /// matters because the over-eviction only manifests when
    /// `incoming_sz > cheap_sz` — left to chance it reproduces in roughly 1 run
    /// in 13, which is not a regression test.
    #[test]
    fn eviction_stops_once_there_is_room_and_does_not_over_evict() {
        let cheap = build_regular_tx_with_fee(1, "a");
        let keep = build_regular_tx_with_fee(5, "b");
        // 64-char tag widens the serialized `address`, dwarfing the few bytes of
        // run-to-run jitter in the hash encoding.
        let incoming = build_regular_tx_with_fee(9, &"z".repeat(64));

        let cheap_id = cheap.id.unwrap();
        let keep_id = keep.id.unwrap();
        let incoming_id = incoming.id.unwrap();

        let cheap_sz = cheap.size();
        let keep_sz = keep.size();
        let incoming_sz = incoming.size();

        assert!(
            incoming_sz > cheap_sz,
            "precondition: incoming ({incoming_sz}) must exceed cheap ({cheap_sz}); \
             without that the over-eviction path is not exercised"
        );

        // One byte short of holding all three, so admitting `incoming` must free
        // something — but freeing `cheap` alone is already sufficient.
        let capacity = cheap_sz + keep_sz + incoming_sz - 1;
        let mut config = Config::default();
        config.mempool_max_size_bytes = capacity;
        let mut mempool = Mempool::new(&config);

        mempool.add_transaction(cheap).expect("cheap must fit");
        mempool.add_transaction(keep).expect("keep must fit");
        mempool
            .add_transaction(incoming)
            .expect("incoming must be admitted after evicting cheap");

        assert!(
            mempool.get_transaction(&cheap_id).is_none(),
            "cheapest entry must be evicted"
        );
        assert!(
            mempool.get_transaction(&keep_id).is_some(),
            "keep must survive — room existed after evicting cheap, so evicting it too \
             is over-eviction (this is the assertion the old stop condition failed)"
        );
        assert!(
            mempool.get_transaction(&incoming_id).is_some(),
            "incoming must be admitted"
        );
        assert_eq!(
            mempool.size(),
            2,
            "exactly one transaction should be evicted"
        );
    }

    /// #2026: the fee-per-byte signal must survive integer division. Two txs
    /// with the SAME absolute fee but different sizes must order by fee-per-byte
    /// (smaller tx first). Under the old `fee / tx_size` formula both truncated
    /// to rate 0 (fee < tx_size), collapsing the per-byte signal so ordering
    /// fell entirely to abs_fee (equal here) then txid — i.e. the documented
    /// primary key was dead. The milli-units/byte scaling restores it.
    #[test]
    fn fee_rate_per_byte_signal_survives_scaling() {
        // Same absolute fee (200 < tx_size, so the old formula gave rate 0 for
        // both), different sizes via a long padded tag on the "big" tx.
        let small = build_regular_tx_with_fee(200, "s");
        let big = build_regular_tx_with_fee(200, &"p".repeat(256));

        assert!(
            big.size() > small.size(),
            "padded tx must serialize larger (small={}, big={})",
            small.size(),
            big.size()
        );

        let mut mp = Mempool::new(&Config::default());
        mp.add_transaction(small.clone()).unwrap();
        mp.add_transaction(big.clone()).unwrap();

        let order: Vec<Hash> = mp.iter_by_fee_order().map(|tx| tx.id.unwrap()).collect();
        // Higher fee-per-byte (the smaller tx) must come first. With both at the
        // old rate 0 this ordering was not achievable from the per-byte key.
        assert_eq!(
            order[0],
            small.id.unwrap(),
            "smaller tx (higher fee-per-byte) must order before the larger tx — \
             the per-byte signal must not collapse to 0 (#2026)"
        );
    }

    /// Under precise size pressure, `evict_transactions` must remove the
    /// lowest-fee-rate transaction and admit the incoming higher-fee transaction.
    ///
    /// Unlike `adversarial_mempool_full_evicts_lowest_fee` (which accepts both
    /// success and error), this test sets pool capacity so the outcome is
    /// deterministic: low (fee=1) is always the cheapest entry and must be
    /// evicted, never medium (fee=2) or high (fee=9).
    #[test]
    fn eviction_under_precise_size_pressure_removes_exactly_lowest_fee_tx() {
        let low = build_regular_tx_with_fee(1, "p");
        let med = build_regular_tx_with_fee(2, "q");
        let high = build_regular_tx_with_fee(9, "r");

        let low_id = low.id.unwrap();
        let med_id = med.id.unwrap();
        let high_id = high.id.unwrap();

        let low_sz = low.size();
        let med_sz = med.size();
        let high_sz = high.size();

        // Capacity: enough for `med` plus whichever of {low, high} is larger.
        //
        // This previously read `low_sz + med_sz`, which only produced the
        // intended pressure when `high_sz <= low_sz` — an assumption the test
        // asserted directly and which is simply not true. `Transaction::size()`
        // is `serde_json::to_vec(self).len()`, and `id` serializes as
        // `{"bytes":[117,157,247,...]}` — 32 numbers of 1-3 digits each, whose
        // width depends on the hash's byte values. `Transaction::new` stamps
        // `Utc::now()` and `calculate_hash` folds the timestamp, so the same
        // logical transaction serializes to a different length on every
        // construction (measured: 722-728 bytes over 200 builds). The old
        // assertion therefore compared two effectively random numbers and
        // failed whenever `high` happened to hash wider than `low` — ~12% of
        // local runs, and the failure seen on run 29885774963.
        //
        // The property under test never needed byte-identical sizes. It needs
        // the pool to hold `med` plus exactly one of the other two, which this
        // capacity guarantees for ANY sizes:
        //   * low + med fit          (low_sz <= max, always)
        //   * all three do not fit   (asserted below)
        //   * after evicting low, high fits (high_sz <= max, always)
        let capacity = med_sz + low_sz.max(high_sz);

        // Non-vacuity guard: if all three fitted, nothing would be evicted and
        // every assertion below would pass while testing nothing. Since
        // capacity = med_sz + max(low_sz, high_sz), this holds whenever the
        // smaller of low/high is non-empty — but assert it rather than assume,
        // because a future change to `size()` should break this loudly here
        // rather than silently turn the test into a no-op.
        assert!(
            low_sz + med_sz + high_sz > capacity,
            "precondition: all three ({low_sz}+{med_sz}+{high_sz}) must exceed capacity \
             ({capacity}), or admitting `high` would not force an eviction and this test \
             would assert nothing"
        );

        let mut config = Config::default();
        config.mempool_max_size_bytes = capacity;
        let mut mempool = Mempool::new(&config);

        // Fill pool: low then medium.
        mempool.add_transaction(low).expect("low-fee tx must fit");
        mempool
            .add_transaction(med)
            .expect("medium-fee tx must fit — pool sized for med + one of low/high");
        assert_eq!(
            mempool.size(),
            2,
            "precondition: pool holds exactly two transactions"
        );

        // Adding `high` must evict the cheapest entry (low) and then be admitted.
        //
        // Worth being precise about *which* key decides "cheapest" here, because
        // the ordering key is `(Reverse(fee_rate), Reverse(abs_fee), tx_id)` and
        // `fee_rate` is integer `fee / size`. With fees of 1/2/9 over ~720-byte
        // transactions all three fee rates truncate to 0, so the discriminator
        // is the `abs_fee` secondary key — which is exactly what it exists for
        // (see the "avoids priority inversion when integer division collapses
        // nearby fees to same rate" comment on the insert). This test therefore
        // covers that secondary key, not the primary fee-rate ordering.
        mempool
            .add_transaction(high)
            .expect("high-fee tx must be admitted after low is evicted under size pressure");

        assert!(
            mempool.get_transaction(&low_id).is_none(),
            "low-fee tx (fee=1) must be evicted — it is the cheapest entry"
        );
        assert!(
            mempool.get_transaction(&med_id).is_some(),
            "medium-fee tx (fee=2) must survive eviction — only the cheapest is removed"
        );
        assert!(
            mempool.get_transaction(&high_id).is_some(),
            "high-fee tx (fee=9) must be admitted after eviction"
        );
    }

    // ── #2555 atomic admission ──────────────────────────────────────────────

    fn spend_of(txid: Hash, vout: u32, address: &str, fee: u64) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid,
                vout,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 100,
                address: address.to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.fee = fee;
        tx.set_id();
        tx
    }

    /// Full snapshot of every pool index, for byte-identical Err assertions.
    fn index_snapshot(
        pool: &Mempool,
    ) -> (
        Vec<Hash>,
        usize,
        Vec<(Reverse<u64>, Reverse<u64>, Hash)>,
        FxHashMap<Hash, (u64, u64)>,
        FxHashMap<Hash, FxHashSet<Hash>>,
        FxHashMap<Hash, FxHashSet<Hash>>,
        FxHashMap<(Hash, u32), Vec<Hash>>,
        Vec<Hash>,
        FxHashMap<String, usize>,
        FxHashSet<String>,
    ) {
        let mut tx_ids: Vec<Hash> = pool.transactions.keys().copied().collect();
        tx_ids.sort();
        let mut ts_ids: Vec<Hash> = pool.timestamps.keys().copied().collect();
        ts_ids.sort();
        (
            tx_ids,
            pool.current_size,
            pool.fee_order.keys().cloned().collect(),
            pool.tx_fee_rates.clone(),
            pool.dependencies.clone(),
            pool.dependents.clone(),
            pool.double_spends.clone(),
            ts_ids,
            pool.per_sender_count.clone(),
            pool.protocol_replay_keys.clone(),
        )
    }

    #[test]
    fn rejected_double_spend_after_eviction_pressure_leaves_pool_unchanged_2555() {
        // The #2576-review shape: the conflicting spend does not fit without
        // eviction, but the double-spend rejection must fire before any
        // capacity work touches the pool.
        let shared_txid = hash_string("atomic-admission-shared-utxo");
        let resident = spend_of(shared_txid, 0, "atomic-admission-resident", 1);
        let resident_id = resident.id.expect("resident id");
        let conflicting = spend_of(shared_txid, 0, "atomic-admission-conflicting", 10_000);

        let mut config = Config::default();
        config.mempool_max_size_bytes = resident.size() + conflicting.size() - 1;
        let mut mempool = Mempool::new(&config);
        mempool.add_transaction(resident).expect("resident admitted");
        let before = index_snapshot(&mempool);

        let err = mempool
            .add_transaction(conflicting)
            .expect_err("conflicting spend must be rejected");
        assert!(err.to_string().contains("Double spend"));
        assert_eq!(
            index_snapshot(&mempool),
            before,
            "a rejected double-spend must leave every pool index untouched"
        );
        assert!(mempool.get_transaction(&resident_id).is_some());
    }

    #[test]
    fn infeasible_eviction_err_leaves_every_index_byte_identical_2555() {
        // The #2576-review RED: capacity work that cannot make room must
        // return "Unable to make room for transaction" with every index
        // byte-identical — the mutating eviction path removed entries before
        // discovering infeasibility. High-priority residents are ineligible
        // victims, so a pool full of them is exactly the infeasible case.
        let mut config = Config::default();
        let resident_a = spend_of(hash_string("atomic-utxo-a"), 0, "atomic-a", 5);
        let resident_b = spend_of(hash_string("atomic-utxo-b"), 0, "atomic-b", 5);
        config.mempool_max_size_bytes = resident_a.size() + resident_b.size();
        let mut mempool = Mempool::new(&config);

        let id_a = resident_a.id.expect("id a");
        let id_b = resident_b.id.expect("id b");
        mempool.add_transaction(resident_a).expect("resident a admitted");
        mempool.add_transaction(resident_b).expect("resident b admitted");
        mempool.set_priority(&id_a, TxPriority::High);
        mempool.set_priority(&id_b, TxPriority::High);
        let before = index_snapshot(&mempool);

        let incoming = spend_of(hash_string("atomic-utxo-c"), 0, "atomic-c", 50_000);
        let err = mempool
            .add_transaction(incoming)
            .expect_err("no victim is evictable; admission must fail");
        assert!(
            err.to_string().contains("Unable to make room"),
            "unexpected error: {err}"
        );
        assert_eq!(
            index_snapshot(&mempool),
            before,
            "a failed capacity plan must leave every pool index byte-identical"
        );
    }

    #[test]
    fn admitted_child_never_references_an_evicted_parent_2555() {
        // The #2576-review dependency RED: under eviction pressure, an
        // admitted child's dependency edges must all point at live entries.
        // The parent is the cheapest resident — the old eviction order would
        // pick it first — but it is the incoming child's ancestor, so the
        // planner must take the filler instead.
        let parent = spend_of(hash_string("dep-utxo-parent"), 0, "dep-parent", 1);
        let parent_id = parent.id.expect("parent id");
        let filler = spend_of(hash_string("dep-utxo-filler"), 0, "dep-filler", 2);
        let filler_id = filler.id.expect("filler id");
        let child = spend_of(parent_id, 0, "dep-child", 10_000);
        let child_id = child.id.expect("child id");

        let mut config = Config::default();
        // Both residents fit; parent + filler + child does not; child fits
        // once the filler is gone.
        config.mempool_max_size_bytes = parent.size() + filler.size() + child.size() - 1;
        let mut mempool = Mempool::new(&config);
        mempool.add_transaction(parent).expect("parent admitted");
        mempool.add_transaction(filler).expect("filler admitted");

        mempool.add_transaction(child).expect("child admitted under pressure");

        assert!(
            mempool.get_transaction(&parent_id).is_some(),
            "the child's in-pool ancestor must not be the eviction victim"
        );
        assert!(
            mempool.get_transaction(&filler_id).is_none(),
            "the unprotected filler is the correct victim"
        );
        let child_deps = mempool.dependencies.get(&child_id).expect("child has deps");
        assert_eq!(child_deps.len(), 1);
        assert!(child_deps.contains(&parent_id));
        for dep in child_deps {
            assert!(
                mempool.transactions.contains_key(dep),
                "dependency edge {dep} points at a transaction not in the pool"
            );
        }
        assert!(
            mempool.dependents.get(&parent_id).is_some_and(|d| d.contains(&child_id)),
            "reverse index must record the child under its live parent"
        );
    }

    #[test]
    fn ancestor_protection_refuses_rather_than_orphans_2555() {
        // If the ONLY way to make room is evicting the child's own ancestor,
        // admission must refuse with the pool untouched — not admit a child
        // whose parent it just removed.
        let parent = spend_of(hash_string("dep-only-utxo-parent"), 0, "dep-only-parent", 1);
        let parent_id = parent.id.expect("parent id");
        let child = spend_of(parent_id, 0, "dep-only-child", 10_000);

        let mut config = Config::default();
        config.mempool_max_size_bytes = parent.size() + child.size() - 1;
        let mut mempool = Mempool::new(&config);
        mempool.add_transaction(parent).expect("parent admitted");
        let before = index_snapshot(&mempool);

        let err = mempool
            .add_transaction(child)
            .expect_err("room only exists by evicting the ancestor; must refuse");
        assert!(
            err.to_string().contains("Unable to make room"),
            "unexpected error: {err}"
        );
        assert_eq!(index_snapshot(&mempool), before);
    }
}
