# Rust Code Commenting Standards

This guide provides comprehensive commenting standards for Rust code in the WRFCoin blockchain
platform.

## Documentation Comments

### Function Documentation

Use documentation comments (`///`) for all public functions:

````rust
/// Validates and processes a new block for the WRFCoin blockchain
///
/// This function performs comprehensive validation of a proposed block including
/// cryptographic verification, transaction validation, and consensus rule checking.
/// The validation follows the WRFCoin consensus protocol v2.1 specification.
///
/// # Arguments
///
/// * `block` - The proposed block to validate and process
/// * `previous_hash` - Hash of the previous block in the chain
/// * `validator_context` - Current validator state and configuration
///
/// # Returns
///
/// Returns `Ok(BlockProcessingResult)` if validation succeeds, containing the
/// processed block and any state changes. Returns `Err(BlockValidationError)`
/// if validation fails with specific error details.
///
/// # Errors
///
/// * `InvalidBlockHash` - Block hash doesn't match calculated hash
/// * `InvalidTimestamp` - Block timestamp is outside acceptable range
/// * `InsufficientStake` - Validator doesn't have required stake amount
/// * `TransactionValidationError` - One or more transactions are invalid
///
/// # Examples
///
/// ```rust
/// use wrfcoin_core::{Block, ValidatorContext, process_block};
///
/// let validator_context = ValidatorContext::new(validator_key, stake_amount);
/// let result = process_block(proposed_block, previous_hash, validator_context).await?;
///
/// match result {
///     Ok(processing_result) => {
///         println!("Block accepted: {}", processing_result.block_hash);
///         apply_state_changes(processing_result.state_changes);
///     }
///     Err(error) => {
///         eprintln!("Block rejected: {:?}", error);
///     }
/// }
/// ```
///
/// # Security Considerations
///
/// This function performs cryptographic operations and must be called with
/// validated input data. Block validation includes:
/// - Digital signature verification using Ed25519
/// - Merkle tree validation for transaction integrity
/// - Double-spend prevention checks
/// - Consensus rule enforcement
///
/// # Performance Notes
///
/// Block processing is computationally intensive (avg 50-200ms for typical blocks).
/// Uses parallel validation for transactions when block contains >10 transactions.
/// Memory usage scales with transaction count (approx 1KB per transaction).
///
/// # See Also
///
/// * [`validate_transaction`] - Individual transaction validation
/// * [`consensus_rules`] - Consensus protocol specification
/// * [`crypto::verify_signature`] - Cryptographic verification functions
pub async fn process_block(
    block: Block,
    previous_hash: Hash256,
    validator_context: ValidatorContext,
) -> Result<BlockProcessingResult, BlockValidationError> {
    // Implementation details...
}
````

### Struct Documentation

Document structs with their purpose, guarantees, and usage patterns:

````rust
/// High-performance transaction pool for the WRFCoin blockchain
///
/// The `TransactionPool` manages pending transactions before they are included
/// in blocks. It provides efficient insertion, removal, and iteration capabilities
/// with automatic expiration and conflict detection.
///
/// # Thread Safety
///
/// This struct is thread-safe and designed for concurrent access by multiple
/// validator nodes. All public methods use internal locking mechanisms.
///
/// # Memory Management
///
/// The pool automatically manages memory usage with configurable limits:
/// - Maximum transaction count (default: 10,000)
/// - Maximum memory usage (default: 50MB)
/// - Transaction TTL (default: 30 minutes)
///
/// # Examples
///
/// ```rust
/// use wrfcoin_core::TransactionPool;
///
/// // Create pool with default configuration
/// let pool = TransactionPool::new();
///
/// // Add transaction to pool
/// pool.add_transaction(transaction).await?;
///
/// // Get transactions for next block
/// let transactions = pool.get_pending_transactions(1000).await;
///
/// // Remove processed transactions
/// pool.remove_transactions(&processed_tx_hashes).await;
/// ```
///
/// # Configuration
///
/// ```rust
/// let config = TransactionPoolConfig {
///     max_transactions: 15000,
///     max_memory_mb: 100,
///     transaction_ttl_minutes: 45,
///     enable_mempool_replacement: true,
/// };
///
/// let pool = TransactionPool::with_config(config);
/// ```
#[derive(Debug)]
pub struct TransactionPool {
    /// Pending transactions indexed by hash for O(1) lookups
    ///
    /// Uses a HashMap for fast transaction retrieval during block construction.
    /// Key: Transaction hash, Value: Transaction with metadata
    transactions: Arc<RwLock<HashMap<Hash256, PooledTransaction>>>,

    /// Transaction ordering by fee rate (highest first)
    ///
    /// BTreeMap ensures transactions are always ordered by fee rate for
    /// optimal block construction. Key: Fee rate, Value: Transaction hash
    fee_ordered: Arc<RwLock<BTreeMap<FeeRate, Hash256>>>,

    /// Configuration parameters for pool behavior
    ///
    /// Immutable configuration set during pool creation. Controls memory
    /// limits, expiration policies, and replacement rules.
    config: TransactionPoolConfig,

    /// Current memory usage tracking in bytes
    ///
    /// Atomic counter for thread-safe memory usage tracking. Updated
    /// on every transaction addition/removal for accurate limits.
    memory_usage: AtomicU64,

    /// Background task handles for cleanup and maintenance
    ///
    /// Handles for background tasks that perform periodic cleanup of
    /// expired transactions and memory optimization.
    _cleanup_handles: Vec<JoinHandle<()>>,
}

impl TransactionPool {
    /// Creates a new transaction pool with default configuration
    ///
    /// Initializes all internal data structures and starts background
    /// maintenance tasks for automatic cleanup and optimization.
    ///
    /// # Returns
    ///
    /// A new `TransactionPool` instance ready for transaction management.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let pool = TransactionPool::new();
    /// assert_eq!(pool.len(), 0);
    /// ```
    pub fn new() -> Self {
        Self::with_config(TransactionPoolConfig::default())
    }

    /// Adds a transaction to the pool after validation
    ///
    /// Validates the transaction against current pool state and network rules
    /// before adding. Performs duplicate detection and replacement logic.
    ///
    /// # Arguments
    ///
    /// * `transaction` - The transaction to add to the pool
    ///
    /// # Returns
    ///
    /// `Ok(())` if transaction was successfully added, `Err(PoolError)` if
    /// the transaction is invalid or cannot be added.
    ///
    /// # Errors
    ///
    /// * `DuplicateTransaction` - Transaction already exists in pool
    /// * `InsufficientFee` - Transaction fee is below minimum threshold
    /// * `PoolCapacityExceeded` - Pool has reached maximum capacity
    /// * `InvalidTransaction` - Transaction fails validation checks
    ///
    /// # Performance
    ///
    /// Average operation time: O(log n) where n is the number of transactions.
    /// Memory allocation: ~200 bytes per transaction for metadata.
    pub async fn add_transaction(&self, transaction: Transaction) -> Result<(), PoolError> {
        // VALIDATION: Comprehensive transaction validation before pool insertion
        // Prevents invalid transactions from corrupting pool state
        self.validate_transaction(&transaction).await?;

        // CONCURRENCY: Acquire write locks in consistent order to prevent deadlocks
        // Order: transactions lock -> fee_ordered lock
        let mut transactions = self.transactions.write().await;
        let mut fee_ordered = self.fee_ordered.write().await;

        let tx_hash = transaction.hash();

        // BUSINESS RULE: Check for duplicate transactions
        // Duplicate detection prevents double-spending in the mempool
        if transactions.contains_key(&tx_hash) {
            return Err(PoolError::DuplicateTransaction(tx_hash));
        }

        // CAPACITY: Check memory and transaction count limits
        // Prevents DoS attacks through resource exhaustion
        let tx_size = transaction.serialized_size();
        let current_memory = self.memory_usage.load(Ordering::Acquire);
        let max_memory = self.config.max_memory_mb * 1024 * 1024;

        if current_memory + tx_size as u64 > max_memory {
            // POLICY: Attempt to evict low-fee transactions to make space
            if !self.try_evict_low_fee_transactions(tx_size).await {
                return Err(PoolError::PoolCapacityExceeded);
            }
        }

        // ALGORITHM: Calculate fee rate for priority ordering
        let fee_rate = FeeRate::calculate(&transaction);

        // OPTIMIZATION: Insert into both data structures atomically
        let pooled_tx = PooledTransaction {
            transaction: transaction.clone(),
            added_at: Instant::now(),
            fee_rate,
            size_bytes: tx_size,
        };

        transactions.insert(tx_hash, pooled_tx);
        fee_ordered.insert(fee_rate, tx_hash);

        // METRICS: Update memory usage counter
        self.memory_usage.fetch_add(tx_size as u64, Ordering::AcqRel);

        Ok(())
    }
}
````

### Trait Documentation

Document traits with their contracts and implementation requirements:

````rust
/// Consensus mechanism trait for blockchain validation
///
/// Defines the interface for consensus algorithms in the WRFCoin blockchain.
/// Implementations must provide deterministic validation and ensure network
/// agreement on block acceptance criteria.
///
/// # Safety Requirements
///
/// Implementations must guarantee:
/// - Deterministic validation (same input always produces same result)
/// - Byzantine fault tolerance up to 1/3 of validator nodes
/// - Finality within acceptable time bounds (< 30 seconds)
/// - Resistance to common attacks (nothing-at-stake, long-range, etc.)
///
/// # Performance Requirements
///
/// - Block validation: < 100ms for typical blocks
/// - Memory usage: < 100MB during validation
/// - Network messages: < 1KB per consensus round
///
/// # Examples
///
/// ```rust
/// use wrfcoin_core::consensus::Consensus;
///
/// struct ProofOfStakeConsensus {
///     validator_set: ValidatorSet,
///     stake_tracker: StakeTracker,
/// }
///
/// impl Consensus for ProofOfStakeConsensus {
///     async fn validate_block(&self, block: &Block) -> Result<ValidationResult, ConsensusError> {
///         // Implement PoS validation logic
///         self.validate_stake_requirements(block).await?;
///         self.verify_signatures(block).await?;
///         Ok(ValidationResult::Valid)
///     }
/// }
/// ```
pub trait Consensus: Send + Sync + 'static {
    /// Validates a proposed block against consensus rules
    ///
    /// This method must perform all necessary checks to determine if a block
    /// should be accepted into the blockchain. Validation must be deterministic
    /// and consistent across all network participants.
    ///
    /// # Arguments
    ///
    /// * `block` - The block to validate
    /// * `context` - Current blockchain state and validator information
    ///
    /// # Returns
    ///
    /// Returns `ValidationResult::Valid` if the block passes all consensus
    /// checks, or `ValidationResult::Invalid(reason)` with specific error details.
    ///
    /// # Errors
    ///
    /// * `InvalidValidator` - Block proposer is not authorized
    /// * `InsufficientStake` - Validator lacks required stake
    /// * `InvalidSignature` - Block signature verification failed
    /// * `TimestampError` - Block timestamp is invalid
    async fn validate_block(
        &self,
        block: &Block,
        context: &ValidationContext,
    ) -> Result<ValidationResult, ConsensusError>;

    /// Determines the next validator for block proposal
    ///
    /// Implements the validator selection algorithm for the consensus mechanism.
    /// Must be deterministic and fair according to the consensus rules.
    ///
    /// # Arguments
    ///
    /// * `slot` - The time slot for which to select a validator
    /// * `randomness` - Source of randomness for selection (e.g., block hash)
    ///
    /// # Returns
    ///
    /// The validator ID selected for the given slot, or `None` if no validator
    /// is eligible (e.g., insufficient stake).
    async fn select_validator(
        &self,
        slot: BlockSlot,
        randomness: &[u8],
    ) -> Result<Option<ValidatorId>, ConsensusError>;

    /// Handles consensus messages from other validators
    ///
    /// Processes incoming consensus-related messages such as votes, proposals,
    /// or attestations. Implementation depends on the specific consensus algorithm.
    ///
    /// # Arguments
    ///
    /// * `message` - The consensus message to process
    /// * `sender` - ID of the validator who sent the message
    ///
    /// # Returns
    ///
    /// `Ok(())` if message was processed successfully, error otherwise.
    async fn handle_consensus_message(
        &self,
        message: ConsensusMessage,
        sender: ValidatorId,
    ) -> Result<(), ConsensusError>;
}
````

### Enum Documentation

Document enums with variant explanations and usage:

````rust
/// Represents possible errors during blockchain operations
///
/// This enum covers all error conditions that can occur during blockchain
/// processing, from transaction validation to consensus failures. Each variant
/// provides specific error information for proper error handling.
///
/// # Error Categories
///
/// - **Validation Errors**: Issues with transaction or block validation
/// - **Consensus Errors**: Problems with consensus mechanism operation
/// - **Network Errors**: Communication failures with other nodes
/// - **Storage Errors**: Database or file system issues
/// - **Cryptographic Errors**: Digital signature or hash verification failures
///
/// # Examples
///
/// ```rust
/// match blockchain.add_transaction(tx).await {
///     Ok(result) => println!("Transaction added: {}", result.tx_hash),
///     Err(BlockchainError::InvalidTransaction(reason)) => {
///         eprintln!("Transaction rejected: {}", reason);
///     }
///     Err(BlockchainError::InsufficientFunds { required, available }) => {
///         eprintln!("Insufficient funds: need {}, have {}", required, available);
///     }
///     Err(error) => eprintln!("Unexpected error: {:?}", error),
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockchainError {
    /// Transaction failed validation checks
    ///
    /// Indicates that a transaction does not meet the basic validity requirements
    /// such as proper format, valid signatures, or business rule compliance.
    ///
    /// # Common Causes
    /// - Invalid digital signatures
    /// - Malformed transaction structure
    /// - Expired transaction timestamps
    /// - Invalid transaction version
    InvalidTransaction(String),

    /// Insufficient funds for transaction execution
    ///
    /// The sender's account does not have enough balance to cover the
    /// transaction amount plus fees. This is a recoverable error condition.
    ///
    /// # Fields
    /// - `required`: Total amount needed (amount + fees)
    /// - `available`: Current account balance
    InsufficientFunds {
        required: Amount,
        available: Amount,
    },

    /// Block failed consensus validation
    ///
    /// The proposed block does not meet consensus requirements such as
    /// proper validator selection, stake requirements, or signature verification.
    ///
    /// # Variants
    /// - Invalid block proposer
    /// - Insufficient validator stake
    /// - Invalid block signatures
    /// - Consensus rule violations
    ConsensusError(ConsensusErrorKind),

    /// Network communication failure
    ///
    /// Failed to communicate with other blockchain nodes. This includes
    /// connection timeouts, protocol errors, and peer discovery issues.
    ///
    /// # Recovery
    /// Network errors are often temporary and may resolve with retry logic.
    /// Implements exponential backoff for automatic recovery.
    NetworkError {
        /// Type of network operation that failed
        operation: String,
        /// Underlying network error details
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Storage subsystem error
    ///
    /// Database or file system operations failed. This includes disk full
    /// conditions, permission errors, and database corruption.
    ///
    /// # Severity
    /// Storage errors are often fatal and may require manual intervention
    /// such as disk space cleanup or database repair.
    StorageError(StorageErrorKind),

    /// Cryptographic operation failed
    ///
    /// Digital signature verification, hash calculation, or other cryptographic
    /// operations encountered an error. Usually indicates corrupted data.
    CryptographicError {
        /// Type of cryptographic operation
        operation: &'static str,
        /// Error details from crypto library
        details: String,
    },

    /// Invalid blockchain state detected
    ///
    /// The blockchain has entered an inconsistent state, possibly due to
    /// concurrent modifications or corrupted data. Requires state recovery.
    InvalidState {
        /// Description of the inconsistency
        description: String,
        /// Current block height when error occurred
        block_height: u64,
    },
}

impl std::fmt::Display for BlockchainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockchainError::InvalidTransaction(reason) => {
                write!(f, "Invalid transaction: {}", reason)
            }
            BlockchainError::InsufficientFunds { required, available } => {
                write!(
                    f,
                    "Insufficient funds: required {}, available {}",
                    required, available
                )
            }
            BlockchainError::ConsensusError(kind) => {
                write!(f, "Consensus error: {:?}", kind)
            }
            BlockchainError::NetworkError { operation, source } => {
                write!(f, "Network error during {}: {}", operation, source)
            }
            BlockchainError::StorageError(kind) => {
                write!(f, "Storage error: {:?}", kind)
            }
            BlockchainError::CryptographicError { operation, details } => {
                write!(f, "Cryptographic error in {}: {}", operation, details)
            }
            BlockchainError::InvalidState { description, block_height } => {
                write!(
                    f,
                    "Invalid blockchain state at height {}: {}",
                    block_height, description
                )
            }
        }
    }
}

impl std::error::Error for BlockchainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BlockchainError::NetworkError { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}
````

## Inline Comments

### Algorithm Implementation Comments

Explain complex algorithms and data structures:

```rust
impl BlockchainCore {
    /// Performs fast block synchronization using checkpoint-based approach
    async fn fast_sync_to_checkpoint(&mut self, target_checkpoint: Checkpoint) -> Result<(), SyncError> {
        // ALGORITHM: Fast sync implementation based on Ethereum's "fast sync" protocol
        // Phase 1: Download and verify checkpoint headers (light validation)
        // Phase 2: Download state trie at checkpoint height (parallel)
        // Phase 3: Download and validate recent blocks (full validation)

        let checkpoint_height = target_checkpoint.block_height;
        let current_height = self.get_current_height().await?;

        if checkpoint_height <= current_height {
            return Ok(()); // Already synced past checkpoint
        }

        // OPTIMIZATION: Skip full validation for historical blocks during initial sync
        // Trade-off: Trust checkpoint security vs. sync speed
        // Justification: Checkpoints are verified by multiple trusted sources
        let sync_start_height = current_height + 1;
        let fast_sync_threshold = checkpoint_height.saturating_sub(1000); // Last 1000 blocks get full validation

        // PERFORMANCE: Parallel download of block headers
        // Download headers in batches to maximize network utilization
        // Batch size: 128 headers (optimal for 1MB network packets)
        let header_batches = self.calculate_header_batches(sync_start_height, fast_sync_threshold);

        for batch in header_batches {
            // CONCURRENCY: Download multiple batches concurrently but process sequentially
            // Maintains block ordering while maximizing download throughput
            let headers = self.download_header_batch(batch).await?;

            for header in headers {
                // VALIDATION: Light validation during fast sync (PoW/PoS + basic structure)
                // Skip expensive transaction validation until recent blocks
                self.validate_header_light(&header).await?;

                // STORAGE: Store header immediately for checkpoint progress tracking
                self.store_header(&header).await?;

                // PROGRESS: Update sync progress for UI feedback
                self.update_sync_progress(header.height, checkpoint_height).await;
            }
        }

        // ALGORITHM: State trie download at checkpoint height
        // Download the complete state trie to establish valid starting point
        let state_root = target_checkpoint.state_root;

        // PERFORMANCE: Parallel state download using work-stealing queue
        // Each worker downloads different trie branches to maximize throughput
        let mut state_downloader = ParallelStateDownloader::new(
            self.network.clone(),
            8, // Worker count = CPU cores for I/O bound work
        );

        // RELIABILITY: Verify state integrity during download
        // Prevents accepting corrupted state data that could compromise node
        let downloaded_state = state_downloader.download_state_trie(state_root).await?;

        if downloaded_state.root_hash() != state_root {
            return Err(SyncError::CorruptedState {
                expected: state_root,
                actual: downloaded_state.root_hash(),
            });
        }

        // ATOMICITY: Apply state in single database transaction
        // Ensures consistent state even if sync process is interrupted
        let mut tx = self.database.begin_transaction().await?;

        // STORAGE: Replace current state with checkpoint state
        tx.clear_state_trie().await?;
        tx.insert_state_trie(downloaded_state).await?;
        tx.set_current_height(checkpoint_height).await?;

        tx.commit().await?;

        // ALGORITHM: Full validation for recent blocks after checkpoint
        // Last 1000 blocks require full validation for security
        let recent_start = checkpoint_height + 1;
        let chain_tip = self.get_chain_tip_height().await?;

        for height in recent_start..=chain_tip {
            // SECURITY: Full validation including transaction execution
            // Recent blocks affect current state and must be fully verified
            let block = self.download_block(height).await?;
            self.validate_block_full(&block).await?;
            self.apply_block(&block).await?;
        }

        Ok(())
    }
}
```

### Performance-Critical Code Comments

Document performance optimizations and trade-offs:

```rust
impl TransactionValidator {
    /// High-performance transaction validation with signature caching
    async fn validate_transaction_batch(&self, transactions: &[Transaction]) -> Result<Vec<ValidationResult>, ValidationError> {
        // PERFORMANCE: Batch validation reduces overhead by ~60% vs individual validation
        // Benchmarks: 1000 tx/batch = 12ms avg, individual = 30ms total
        let batch_size = transactions.len();

        // OPTIMIZATION: Pre-allocate result vector to avoid reallocations
        let mut results = Vec::with_capacity(batch_size);

        // CONCURRENCY: Parallel signature verification using thread pool
        // CPU-bound crypto operations benefit from parallelization
        // Thread count = min(CPU cores, batch_size / 4) for optimal CPU utilization
        let thread_count = std::cmp::min(
            num_cpus::get(),
            std::cmp::max(1, batch_size / 4),
        );

        // MEMORY: Signature cache reduces crypto operations by ~85%
        // Cache hit rate: >90% in typical network conditions
        // Memory overhead: ~32 bytes per cached signature
        let mut signature_cache_hits = 0;
        let mut signature_verifications = 0;

        // ALGORITHM: Split transactions across worker threads
        let chunk_size = (batch_size + thread_count - 1) / thread_count;
        let chunks: Vec<_> = transactions.chunks(chunk_size).collect();

        // PERFORMANCE: Use scoped threads to avoid heap allocation for thread handles
        let validation_results = crossbeam::scope(|scope| {
            let mut handles = Vec::with_capacity(chunks.len());

            for chunk in chunks {
                let handle = scope.spawn(move |_| {
                    let mut chunk_results = Vec::with_capacity(chunk.len());

                    for tx in chunk {
                        // CACHE: Check signature cache first (O(1) hash lookup)
                        let tx_hash = tx.hash();

                        if let Some(cached_result) = self.signature_cache.get(&tx_hash) {
                            // METRICS: Track cache hit rate for performance monitoring
                            signature_cache_hits += 1;
                            chunk_results.push(cached_result.clone());
                            continue;
                        }

                        // CRYPTO: Expensive signature verification (Ed25519: ~0.5ms per signature)
                        let signature_valid = self.verify_transaction_signature(tx);
                        signature_verifications += 1;

                        // VALIDATION: Additional business logic validation
                        let validation_result = if signature_valid {
                            self.validate_transaction_logic(tx)
                        } else {
                            ValidationResult::Invalid("Invalid signature".to_string())
                        };

                        // CACHE: Store result for future validations (TTL: 5 minutes)
                        self.signature_cache.insert(
                            tx_hash,
                            validation_result.clone(),
                            Duration::from_secs(300),
                        );

                        chunk_results.push(validation_result);
                    }

                    chunk_results
                });

                handles.push(handle);
            }

            // CONCURRENCY: Collect results from all worker threads
            let mut all_results = Vec::with_capacity(batch_size);
            for handle in handles {
                let chunk_results = handle.join().map_err(|_| {
                    ValidationError::ThreadPanic("Validation thread panicked".to_string())
                })?;
                all_results.extend(chunk_results);
            }

            Ok(all_results)
        })?;

        // METRICS: Log performance statistics for monitoring
        let cache_hit_rate = (signature_cache_hits as f64 / batch_size as f64) * 100.0;

        debug!(
            "Batch validation completed: {} transactions, {:.1}% cache hit rate, {} new verifications",
            batch_size,
            cache_hit_rate,
            signature_verifications
        );

        // MONITORING: Alert if cache hit rate drops below threshold
        if cache_hit_rate < 70.0 && batch_size > 10 {
            warn!(
                "Low signature cache hit rate: {:.1}% (threshold: 70%)",
                cache_hit_rate
            );
        }

        Ok(validation_results)
    }
}
```

### Error Handling and Safety Comments

Document error conditions and safety guarantees:

```rust
impl BlockStorage {
    /// Safely writes block to storage with atomic consistency guarantees
    async fn store_block_atomic(&mut self, block: Block) -> Result<(), StorageError> {
        // SAFETY: Atomic block storage prevents partial writes that could corrupt blockchain
        // ACID properties: Atomicity (all-or-nothing), Consistency (valid state),
        // Isolation (concurrent safety), Durability (persistent storage)

        let block_hash = block.hash();
        let block_height = block.height();

        // VALIDATION: Pre-storage validation to prevent corrupted data
        if block_height == 0 && !block.is_genesis_block() {
            return Err(StorageError::InvalidGenesisBlock);
        }

        // CONSISTENCY: Verify block connects to existing chain
        if block_height > 0 {
            let expected_parent = self.get_block_hash(block_height - 1).await?;
            if block.parent_hash() != expected_parent {
                return Err(StorageError::InvalidParentHash {
                    expected: expected_parent,
                    actual: block.parent_hash(),
                    height: block_height,
                });
            }
        }

        // ATOMICITY: Database transaction ensures all-or-nothing semantics
        // If any part of block storage fails, entire operation is rolled back
        let mut transaction = self.database.begin_transaction().await
            .map_err(|e| StorageError::DatabaseError {
                operation: "begin_transaction",
                source: e,
            })?;

        // STEP 1: Store block data (largest operation, most likely to fail)
        transaction.insert_block_data(&block_hash, &block).await
            .map_err(|e| StorageError::DatabaseError {
                operation: "insert_block_data",
                source: e,
            })?;

        // STEP 2: Store block transactions with references
        for (index, tx) in block.transactions().iter().enumerate() {
            transaction.insert_transaction(
                &tx.hash(),
                tx,
                Some(TransactionLocation {
                    block_hash,
                    block_height,
                    index: index as u32,
                }),
            ).await
            .map_err(|e| StorageError::DatabaseError {
                operation: "insert_transaction",
                source: e,
            })?;
        }

        // STEP 3: Update block height index
        transaction.set_block_hash_at_height(block_height, block_hash).await
            .map_err(|e| StorageError::DatabaseError {
                operation: "set_block_hash_at_height",
                source: e,
            })?;

        // STEP 4: Update chain tip if this extends the main chain
        let current_tip_height = self.get_chain_tip_height().await?;
        if block_height > current_tip_height {
            transaction.set_chain_tip(block_height, block_hash).await
                .map_err(|e| StorageError::DatabaseError {
                    operation: "set_chain_tip",
                    source: e,
                })?;
        }

        // DURABILITY: Commit transaction to persistent storage
        // WAL (Write-Ahead Logging) ensures durability even with system crashes
        transaction.commit().await
            .map_err(|e| StorageError::DatabaseError {
                operation: "commit_transaction",
                source: e,
            })?;

        // RELIABILITY: Verify block was stored correctly
        // Post-storage verification catches rare storage corruption issues
        let stored_block = self.get_block(&block_hash).await?;
        if stored_block.hash() != block_hash {
            // PANIC: Data corruption detected - this should never happen
            panic!(
                "CRITICAL: Block storage corruption detected! \
                 Expected hash: {}, Stored hash: {}. \
                 Database integrity compromised.",
                block_hash,
                stored_block.hash()
            );
        }

        // METRICS: Update storage statistics
        self.update_storage_metrics(block.serialized_size()).await;

        info!(
            "Block {} (height: {}) stored successfully with {} transactions",
            block_hash,
            block_height,
            block.transactions().len()
        );

        Ok(())
    }
}
```

## Module-Level Documentation

### Module Purpose and Organization

````rust
//! # WRFCoin Blockchain Core
//!
//! This module contains the core blockchain implementation for WRFCoin,
//! including block validation, transaction processing, and consensus mechanisms.
//!
//! ## Architecture Overview
//!
//! The blockchain core is organized into several key components:
//!
//! - **Block Management**: Block creation, validation, and storage
//! - **Transaction Processing**: Transaction validation and execution
//! - **Consensus Engine**: Proof-of-Stake consensus implementation
//! - **State Management**: Blockchain state tracking and updates
//! - **Network Protocol**: Peer-to-peer communication and synchronization
//!
//! ## Key Features
//!
//! - **High Performance**: >1000 TPS with sub-second finality
//! - **Quantum Resistant**: Post-quantum cryptography (CRYSTALS-Dilithium)
//! - **Energy Efficient**: Proof-of-Stake consensus with minimal energy usage
//! - **Weather Integration**: Native support for weather data and carbon credits
//!
//! ## Threading Model
//!
//! The blockchain core uses an actor-based threading model:
//! - Main consensus thread for block validation
//! - Worker pool for transaction validation (CPU-bound)
//! - Network I/O threads for peer communication
//! - Storage threads for database operations
//!
//! ## Memory Usage
//!
//! Typical memory usage patterns:
//! - Block cache: 50MB (configurable)
//! - Transaction pool: 100MB (configurable)
//! - State trie: 200MB (grows with blockchain size)
//! - Network buffers: 20MB
//!
//! ## Examples
//!
//! ### Basic Blockchain Operations
//!
//! ```rust
//! use wrfcoin_core::{Blockchain, BlockchainConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize blockchain with default configuration
//!     let config = BlockchainConfig::mainnet();
//!     let mut blockchain = Blockchain::new(config).await?;
//!
//!     // Start the blockchain network
//!     blockchain.start().await?;
//!
//!     // Process transactions
//!     let transaction = create_sample_transaction();
//!     blockchain.submit_transaction(transaction).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Custom Consensus Implementation
//!
//! ```rust
//! use wrfcoin_core::consensus::{Consensus, ConsensusEngine};
//!
//! struct CustomConsensus {
//!     // Custom consensus fields
//! }
//!
//! impl Consensus for CustomConsensus {
//!     // Implement consensus trait methods
//! }
//!
//! let consensus = CustomConsensus::new();
//! let engine = ConsensusEngine::with_consensus(consensus);
//! ```
//!
//! ## Security Considerations
//!
//! - All cryptographic operations use constant-time implementations
//! - Transaction validation includes comprehensive security checks
//! - State transitions are atomic and verifiable
//! - Network protocol includes anti-spam and DoS protection
//!
//! ## Performance Tuning
//!
//! Key configuration parameters for performance optimization:
//!
//! - `max_block_size`: Maximum block size in bytes (default: 2MB)
//! - `transaction_pool_size`: Maximum transactions in mempool (default: 10,000)
//! - `validation_threads`: Parallel validation workers (default: CPU cores)
//! - `block_time`: Target time between blocks (default: 2 seconds)
//!
//! ## Monitoring and Metrics
//!
//! The blockchain core exposes metrics for monitoring:
//! - Block production rate
//! - Transaction throughput
//! - Memory usage
//! - Network connectivity
//! - Consensus participation
//!
//! Access metrics via the `metrics()` method on blockchain instances.

use std::sync::Arc;
use tokio::sync::RwLock;

// Re-export key types for convenience
pub use block::{Block, BlockHeader, GenesisBlock};
pub use transaction::{Transaction, TransactionOutput, Amount};
pub use consensus::{Consensus, ProofOfStake, ValidatorSet};
pub use error::{BlockchainError, ValidationError, ConsensusError};

// Module declarations
pub mod block;
pub mod transaction;
pub mod consensus;
pub mod storage;
pub mod network;
pub mod crypto;
pub mod state;
pub mod error;

/// Current blockchain protocol version
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum number of transactions per block
pub const MAX_TRANSACTIONS_PER_BLOCK: usize = 2000;

/// Target block time in seconds
pub const TARGET_BLOCK_TIME: u64 = 2;
````

## Comment Quality Guidelines

### Required Comments

**Must Have Comments:**

- All public functions, structs, enums, and traits
- Complex algorithms and data structures
- Performance-critical code sections
- Security-sensitive operations
- Error handling strategies
- Unsafe code blocks (with safety justification)

### Comment Style Rules

```rust
// ✅ GOOD: Explains reasoning and context
/// Implements parallel Merkle tree construction for improved performance
///
/// Uses work-stealing queue to distribute leaf processing across CPU cores.
/// Reduces tree construction time from O(n) to O(n/cores) for large datasets.
fn build_merkle_tree_parallel(leaves: &[Hash]) -> MerkleTree {
    // ALGORITHM: Bottom-up tree construction with parallel leaf processing
    // Each worker processes a range of leaves, then threads synchronize
    // at each tree level until reaching the root

    // PERFORMANCE: Pre-allocate tree nodes to avoid dynamic allocation
    let node_count = calculate_tree_size(leaves.len());
    let mut nodes = Vec::with_capacity(node_count);

    // Implementation...
}

// ❌ BAD: States the obvious
/// Builds a Merkle tree
fn build_merkle_tree_parallel(leaves: &[Hash]) -> MerkleTree {
    // Create a vector for nodes
    let mut nodes = Vec::new();

    // Implementation...
}
```

### Comment Categories

Use consistent prefixes for different comment types:

- `ALGORITHM:` - Algorithmic approach and complexity
- `PERFORMANCE:` - Performance optimizations and benchmarks
- `SECURITY:` - Security considerations and threat mitigation
- `SAFETY:` - Memory safety and invariant guarantees
- `CONCURRENCY:` - Thread safety and synchronization
- `VALIDATION:` - Input validation and error checking
- `COMPATIBILITY:` - Version compatibility and migration
- `BUSINESS RULE:` - Domain-specific logic and requirements

---

**Next**: [Solidity Commenting Standards](./solidity-standards.md)

**Last Updated**: January 2025  
**Version**: 1.0.0
