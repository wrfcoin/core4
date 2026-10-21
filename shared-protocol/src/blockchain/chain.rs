use crate::blockchain::block::Block;
use crate::blockchain::state::{State, StateTransition};
use crate::crypto::hash::Hash;
use crate::monitoring::CORE_METRICS;
use anyhow::Result;
use std::collections::BTreeMap;

/// Default maximum number of blocks retained in memory. Older blocks are evicted
/// on `add_block` but remain available on disk via the Storage layer.
pub const DEFAULT_MAX_IN_MEMORY_BLOCKS: usize = 200;

/// Reads `WRFCOIN_MAX_IN_MEMORY_BLOCKS` from the environment.
/// Invalid or zero values fall back to `DEFAULT_MAX_IN_MEMORY_BLOCKS`.
///
/// Safety net for #1973: when an operator configures a window smaller than
/// [`MAX_REORG_DEPTH`], the fork point of a deep reorg is likely to have been
/// evicted from memory, so every such reorg must resolve its fork point from
/// disk (via [`Blockchain::plan_reorg_with_loader`]). `execute_reorg` now
/// honors disk-resolved fork points, so this is no longer a correctness
/// hazard — but a callerless `plan_reorg` (no loader) cannot follow those
/// forks. We emit a one-line warning so the trade-off is visible in logs
/// rather than clamping the value (which would override an explicit operator
/// choice and break intentionally-small windows).
pub fn configured_max_in_memory_blocks() -> usize {
    let configured = std::env::var("WRFCOIN_MAX_IN_MEMORY_BLOCKS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_MAX_IN_MEMORY_BLOCKS);

    if (configured as u64) < MAX_REORG_DEPTH {
        log::warn!(
            "WRFCOIN_MAX_IN_MEMORY_BLOCKS={} is below MAX_REORG_DEPTH={}: \
             deep reorgs will rely on the disk-backed block loader to resolve \
             evicted fork points; plan_reorg() without a loader cannot follow \
             forks beyond the in-memory window",
            configured,
            MAX_REORG_DEPTH,
        );
    }

    configured
}

/// Maximum reorg depth (in blocks). Forks deeper than this are rejected and
/// require manual intervention or a full re-sync.
pub const MAX_REORG_DEPTH: u64 = 100;

/// Describes a planned chain reorganization.
#[derive(Debug, Clone)]
pub struct ReorgPlan {
    /// Height of the common ancestor block (the fork point).
    pub fork_height: u64,
    /// Hash of the fork-point block, as resolved by the planner (#1973).
    ///
    /// The planner may resolve the fork point from disk (via
    /// [`Blockchain::plan_reorg_with_loader`]) when it lies below the
    /// in-memory window. In that case `execute_reorg` cannot recover the
    /// fork-point hash from `self.blocks` (the block has been evicted), so
    /// it is carried here so the executor can set the new tip without
    /// consulting in-memory state. Equal to [`Hash::default()`] when
    /// `fork_height == 0` (genesis).
    pub fork_hash: Hash,
    /// Number of local blocks that will be reverted.
    pub blocks_to_revert: u64,
    /// New blocks to apply, ordered by ascending height, starting at
    /// `fork_height + 1`.
    pub blocks_to_apply: Vec<Block>,
}

impl ReorgPlan {
    /// Whether executing this plan would REPLACE a canonical block 1: a genesis
    /// fork (`fork_height == 0`) on a chain that already holds block 1.
    ///
    /// Block 1 is not an ordinary block to a running node: it arms process-wide,
    /// first-install-wins inputs (protocol version, activation table, base
    /// feature schedule, schedule-extension authority — #3277). A node path
    /// that swaps it under a running process leaves those inputs armed from the
    /// OLD block 1 (#3370), so the node path refuses such a plan. The planner
    /// itself stays policy-neutral: it still reports genesis forks.
    ///
    /// A genesis-anchored plan on a chain still at height 0 reverts nothing and
    /// is not a replacement.
    pub fn replaces_block_one(&self) -> bool {
        self.fork_height == 0 && self.blocks_to_revert > 0
    }
}

#[derive(Debug, Clone)]
pub struct Blockchain {
    pub height: u64,
    pub tip: Hash,
    pub state: State,
    blocks: BTreeMap<u64, Block>,
    /// Total number of blocks ever added (including evicted ones).
    total_block_count: u64,
    max_in_memory_blocks: usize,
    /// Highest block height with BFT finality (#1216).
    ///
    /// Set by the consensus adapter whenever the BFT finality gadget
    /// emits `FinalityEvent::BlockFinalized`. `plan_reorg` refuses any
    /// fork whose fork point is at or below this height, guaranteeing
    /// that finalized blocks are never reverted.
    ///
    /// Defaults to 0 (no finality), so behavior is unchanged for callers
    /// that do not wire in the gadget.
    finalized_height: u64,
}

impl Default for Blockchain {
    fn default() -> Self {
        Self {
            height: 0,
            tip: Hash::default(),
            state: State::default(),
            blocks: BTreeMap::new(),
            total_block_count: 0,
            max_in_memory_blocks: configured_max_in_memory_blocks(),
            finalized_height: 0,
        }
    }
}

impl Blockchain {
    pub fn with_capacity(max_in_memory_blocks: usize) -> Self {
        Self {
            height: 0,
            tip: Hash::default(),
            state: State::default(),
            blocks: BTreeMap::new(),
            total_block_count: 0,
            max_in_memory_blocks,
            finalized_height: 0,
        }
    }

    /// Returns the highest block height with BFT finality (#1216).
    ///
    /// Blocks at or below this height are guaranteed to be stable —
    /// `plan_reorg` will refuse to revert them.
    pub fn finalized_height(&self) -> u64 {
        self.finalized_height
    }

    /// Update the BFT-finalized height (#1216).
    ///
    /// Intended to be called by the consensus adapter when the BFT
    /// finality gadget emits `FinalityEvent::BlockFinalized`. The value
    /// is clamped monotonically — lower heights are ignored to prevent
    /// accidental rollback of finality.
    pub fn set_finalized_height(&mut self, new_height: u64) {
        if new_height > self.finalized_height {
            self.finalized_height = new_height;
        }
    }

    pub fn calculate_block_reward(&self, block_height: u64) -> u64 {
        crate::consensus::proof_of_publishing::genesis_emission_partition(block_height, self.state.genesis_emission_config.as_ref()).emission() as u64
    }

    pub fn add_block(&mut self, block: Block) -> Result<()> {
        // ── Hash validation (#1028) ─────────────────────────────────────
        // Blocks MUST carry a pre-computed hash. Accepting None here would
        // allow silent hash divergence between the miner and its peers.
        let stored_hash = block.hash.ok_or_else(|| {
            anyhow::anyhow!(
                "Block at height {} has no hash — every block must be finalized with set_hash() before add_block()",
                block.height,
            )
        })?;

        // NOTE: We intentionally do NOT recompute and compare the hash here
        // because different consensus algorithms use different hashing schemes.
        // PoW blocks use double-SHA256 over the header (see pow.rs mine_block),
        // while PoP blocks use blake3 via calculate_hash(). A blanket
        // recomputation check would reject valid PoW-mined blocks. Consensus-
        // specific hash validation is handled by each ConsensusEngine's
        // validate_block() method.

        // ── Chain-continuity validation (#1018) ──────────────────────────
        // Skip for the very first block (genesis / initial load) since there
        // is no prior tip to validate against.
        if self.total_block_count > 0 {
            let expected_height = self.height + 1;
            if block.height != expected_height {
                anyhow::bail!(
                    "Block height {} does not follow chain tip height {} (expected {})",
                    block.height,
                    self.height,
                    expected_height,
                );
            }
            if block.prev_block_hash != self.tip {
                anyhow::bail!(
                    "Block at height {} has prev_hash {} but chain tip is {}",
                    block.height,
                    block.prev_block_hash,
                    self.tip,
                );
            }
        }

        self.height = block.height;
        self.tip = stored_hash;
        if self.blocks.len() >= self.max_in_memory_blocks {
            if let Some(&oldest) = self.blocks.keys().next() {
                self.blocks.remove(&oldest);
            }
        }
        self.blocks.insert(block.height, block);
        self.total_block_count += 1;
        Ok(())
    }

    pub fn max_in_memory_blocks(&self) -> usize {
        self.max_in_memory_blocks
    }

    pub fn blocks(&self) -> &BTreeMap<u64, Block> {
        &self.blocks
    }

    /// Returns the most recently added block, if any.
    pub fn last_block(&self) -> Option<&Block> {
        self.blocks.values().next_back()
    }

    /// Returns an iterator over blocks in the given height range (inclusive).
    pub fn blocks_range(&self, start: u64, end: u64) -> impl Iterator<Item = &Block> {
        self.blocks.range(start..=end).map(|(_, b)| b)
    }

    /// Returns the block at the given height, if present in memory.
    pub fn block_at_height(&self, height: u64) -> Option<&Block> {
        self.blocks.get(&height)
    }

    /// Number of blocks currently held in the in-memory window.
    pub fn in_memory_block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Total number of blocks ever added to this chain (including evicted).
    pub fn total_block_count(&self) -> u64 {
        self.total_block_count
    }

    /// Reconstruct a Blockchain from persisted storage data.
    ///
    /// `finalized_height` must be the value previously persisted by the
    /// storage layer. Passing `0` on a chain that had a non-zero finalized
    /// height would violate chain safety by allowing reorgs of
    /// already-finalized blocks. Fresh/genesis chains (no prior persisted
    /// state) should pass `0`.
    pub fn from_persisted(
        height: u64,
        tip: Hash,
        state: State,
        blocks: BTreeMap<u64, Block>,
        total_block_count: u64,
        finalized_height: u64,
    ) -> Self {
        Self {
            height,
            tip,
            state,
            blocks,
            total_block_count,
            max_in_memory_blocks: configured_max_in_memory_blocks(),
            finalized_height,
        }
    }

    pub fn apply_state_transition(&mut self, transition: StateTransition) -> Result<()> {
        self.state
            .apply_transition(transition)
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Apply a single state transition without recomputing the state root.
    /// Caller must call `self.state.recalculate_state_root()` when done.
    pub fn apply_state_transition_no_root(
        &mut self,
        transition: StateTransition,
    ) -> Result<()> {
        self.state
            .apply_transition_inner(transition)
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Apply multiple state transitions, recomputing the state root only once at the end.
    pub fn apply_state_transitions_batch(
        &mut self,
        transitions: Vec<StateTransition>,
    ) -> Result<()> {
        self.state
            .apply_transitions_batch(transitions)
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Height-scoped [`Self::apply_state_transitions_batch`] (#2846).
    ///
    /// Opens the ADR-003 emission partition ledger for `height` before applying,
    /// so a block replayed through `Blockchain` charges the same per-block
    /// ceilings the producer charged. Block-applying callers should prefer this.
    pub fn apply_state_transitions_batch_at_height(
        &mut self,
        height: u64,
        transitions: Vec<StateTransition>,
    ) -> Result<()> {
        self.state
            .apply_transitions_batch_at_height(height, transitions)
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    // ------------------------------------------------------------------
    // Chain reorganization
    // ------------------------------------------------------------------

    /// Find the fork point (common ancestor height) between the local chain
    /// and a set of candidate blocks from an alternative fork.
    ///
    /// `candidate_blocks` must be sorted by ascending height and form a
    /// valid chain (each block's `prev_block_hash` matches the previous
    /// block's hash). The method walks backwards from the lowest candidate
    /// block to find a local block whose hash matches the candidate's
    /// `prev_block_hash`.
    ///
    /// Returns `None` if no common ancestor is found within the in-memory
    /// block window.
    pub fn find_fork_point(&self, candidate_blocks: &[Block]) -> Option<u64> {
        let first = candidate_blocks.first()?;

        // The first candidate block's prev_block_hash should reference a
        // block on our chain — that block is the fork point.
        for (&h, blk) in self.blocks.iter().rev() {
            let blk_hash = blk.hash.unwrap_or_else(|| blk.calculate_hash());
            if blk_hash == first.prev_block_hash {
                return Some(h);
            }
        }

        // Also check if the fork point is at height 0 (genesis).
        if first.height == 1 && first.prev_block_hash == Hash::default() {
            return Some(0);
        }

        None
    }

    /// Find the fork point, falling back to a caller-supplied block loader
    /// when the in-memory search fails.
    ///
    /// The `load_block` callback should return the block at the given height
    /// from persistent storage (e.g. `Storage::load_blocks_range`). The
    /// fallback search is bounded by [`MAX_REORG_DEPTH`] to prevent
    /// unbounded disk reads.
    ///
    /// This addresses issue #980: the original `find_fork_point` only
    /// searched the in-memory window (last ~200 blocks). With 1s block
    /// times, deep forks within `MAX_REORG_DEPTH` could silently fail.
    pub fn find_fork_point_with_block_loader(
        &self,
        candidate_blocks: &[Block],
        load_block: impl Fn(u64) -> Option<Block>,
    ) -> Option<u64> {
        // Try the fast in-memory path first.
        if let Some(h) = self.find_fork_point(candidate_blocks) {
            return Some(h);
        }

        let first = candidate_blocks.first()?;

        // Determine the search range: walk backwards from the first
        // candidate's predecessor, but never beyond MAX_REORG_DEPTH below
        // our chain tip and never below height 1.
        let search_start = first.height.saturating_sub(1);
        let search_floor = self.height.saturating_sub(MAX_REORG_DEPTH);
        // Don't search heights we already have in memory.
        let in_memory_min = self.blocks.keys().next().copied().unwrap_or(0);
        let effective_start = if search_start >= in_memory_min {
            in_memory_min.saturating_sub(1)
        } else {
            search_start
        };

        // Walk backwards through storage.
        let mut h = effective_start;
        loop {
            if h < search_floor || h == 0 {
                break;
            }
            if let Some(blk) = load_block(h) {
                let blk_hash = blk.hash.unwrap_or_else(|| blk.calculate_hash());
                if blk_hash == first.prev_block_hash {
                    return Some(h);
                }
            }
            h -= 1;
        }

        // Genesis check (same as find_fork_point).
        if first.height == 1 && first.prev_block_hash == Hash::default() {
            return Some(0);
        }

        None
    }

    /// Determine whether a candidate fork should replace the local chain.
    ///
    /// Returns `Some(ReorgPlan)` when the candidate chain is strictly
    /// better (longer, or same length with a lower tip hash) and forks
    /// within `MAX_REORG_DEPTH`. Returns `None` when the local chain is
    /// already best or the candidate is invalid.
    pub fn plan_reorg(&self, candidate_blocks: &[Block]) -> Result<Option<ReorgPlan>> {
        // Delegate to the loader-aware variant with a no-op loader so that
        // behavior is unchanged for callers that cannot consult storage
        // (tests, legacy code paths). Callers that have access to persisted
        // blocks should prefer `plan_reorg_with_loader` — see issue #1277.
        self.plan_reorg_with_loader(candidate_blocks, |_| None)
    }

    /// Plan a reorg, falling back to the caller-supplied block loader when
    /// the fork point is outside the in-memory window.
    ///
    /// This fixes issue #1277: `plan_reorg` alone only searches the in-memory
    /// window (~200 blocks). Deep forks whose fork point has been evicted
    /// from memory but is still within [`MAX_REORG_DEPTH`] silently failed
    /// to reorg. Honest nodes could not follow valid deep reorgs, creating
    /// a liveness / correctness hazard.
    ///
    /// The `load_block` closure should return the block at the given height
    /// from persistent storage (e.g. `Storage::load_blocks_range(h, h)`).
    /// The fallback search is bounded by [`MAX_REORG_DEPTH`].
    pub fn plan_reorg_with_loader<F: Fn(u64) -> Option<Block>>(
        &self,
        candidate_blocks: &[Block],
        load_block: F,
    ) -> Result<Option<ReorgPlan>> {
        if candidate_blocks.is_empty() {
            return Ok(None);
        }

        // Candidate blocks must be sorted ascending by height.
        let candidate_tip_height = candidate_blocks.last().map(|b| b.height).unwrap_or(0);

        // Local chain is already at least as good — check length first.
        if candidate_tip_height < self.height {
            return Ok(None);
        }

        // Tie-break: equal height → prefer lower tip hash.
        if candidate_tip_height == self.height {
            let candidate_tip_hash = candidate_blocks
                .last()
                .and_then(|b| b.hash)
                .unwrap_or_else(|| candidate_blocks.last().unwrap().calculate_hash());
            if candidate_tip_hash >= self.tip {
                return Ok(None);
            }
        }

        let fork_height = match self
            .find_fork_point_with_block_loader(candidate_blocks, &load_block)
        {
            Some(h) => h,
            None => return Ok(None), // no common ancestor in memory or within MAX_REORG_DEPTH on disk
        };

        // BFT finality guard (#1216): refuse to revert any block at or
        // below the BFT-finalized height. Once the finality gadget has
        // signaled that a block is final, no probabilistic fork choice
        // can override it — doing so would break the gadget's 2/3+
        // safety guarantee.
        if fork_height < self.finalized_height {
            log::warn!(
                "Rejecting reorg: fork point {} is below BFT-finalized height {}",
                fork_height,
                self.finalized_height
            );
            CORE_METRICS.inc_fork_observed();
            CORE_METRICS.set_fork_active(true);
            return Ok(None);
        }

        let blocks_to_revert = self.height.saturating_sub(fork_height);
        if blocks_to_revert > MAX_REORG_DEPTH {
            log::warn!(
                "Rejecting reorg: depth {} exceeds MAX_REORG_DEPTH {}",
                blocks_to_revert,
                MAX_REORG_DEPTH
            );
            // Record the unresolvable fork for operator alerting (#1148).
            // `chain_fork_active` stays high until a successful reorg clears
            // it or the operator intervenes.
            CORE_METRICS.inc_fork_observed();
            CORE_METRICS.set_fork_active(true);
            return Ok(None);
        }

        // Validate that candidate blocks form a contiguous chain from
        // fork_height + 1 onward.
        let expected_start = fork_height + 1;
        if candidate_blocks.first().map(|b| b.height) != Some(expected_start) {
            return Ok(None);
        }
        for window in candidate_blocks.windows(2) {
            if window[1].height != window[0].height + 1 {
                return Ok(None);
            }
            let prev_hash = window[0].hash.unwrap_or_else(|| window[0].calculate_hash());
            if window[1].prev_block_hash != prev_hash {
                return Ok(None);
            }
        }

        // Candidate fork is within MAX_REORG_DEPTH and will be actionable —
        // record the observation for the fork-rate alert (#1148).
        CORE_METRICS.inc_fork_observed();

        // Resolve and carry the fork-point block hash so `execute_reorg`
        // can install the correct tip even when the fork point has been
        // evicted from the in-memory window (#1973). The first candidate
        // block's `prev_block_hash` is, by the contiguity validation above,
        // exactly the hash of the fork-point block at `fork_height`. For a
        // genesis fork (`fork_height == 0`) this is `Hash::default()`, which
        // matches the executor's genesis branch.
        let fork_hash = candidate_blocks
            .first()
            .map(|b| b.prev_block_hash)
            .unwrap_or_default();

        Ok(Some(ReorgPlan {
            fork_height,
            fork_hash,
            blocks_to_revert,
            blocks_to_apply: candidate_blocks.to_vec(),
        }))
    }

    /// Execute a chain reorganization.
    ///
    /// 1. Truncate in-memory blocks to the fork point.
    /// 2. Replace state with `fork_state` (state as of `plan.fork_height`).
    /// 3. Apply each new block via `add_block()`.
    ///
    /// **Caller is responsible** for providing the correct `fork_state`
    /// (typically loaded from a storage checkpoint and replayed to the fork
    /// height) and for persisting the result to storage.
    pub fn execute_reorg(&mut self, plan: &ReorgPlan, fork_state: State) -> Result<()> {
        // Remove blocks above the fork point from the in-memory window.
        let heights_to_remove: Vec<u64> = self
            .blocks
            .range((plan.fork_height + 1)..)
            .map(|(&h, _)| h)
            .collect();
        for h in &heights_to_remove {
            self.blocks.remove(h);
        }

        // Reset chain tip to the fork point.
        if let Some(fork_block) = self.blocks.get(&plan.fork_height) {
            self.tip = fork_block
                .hash
                .unwrap_or_else(|| fork_block.calculate_hash());
            self.height = plan.fork_height;
        } else if plan.fork_height == 0 {
            self.tip = Hash::default();
            self.height = 0;
        } else {
            // The fork point is below the in-memory window and has been
            // evicted (#1973). `plan_reorg_with_loader` legitimately
            // resolves such fork points from disk, so the executor must
            // honor them too — bailing here would abort a correctly-planned
            // deep reorg and strand the node on the abandoned chain (a
            // liveness hazard). Use the fork-point hash the planner carried
            // on the plan instead of consulting in-memory state.
            self.tip = plan.fork_hash;
            self.height = plan.fork_height;
        }

        // Replace state, carrying the node-local supervisor handles across.
        //
        // `State::supply_invariant` is `#[serde(skip)]`, and this method's own
        // contract says `fork_state` is "typically loaded from a storage
        // checkpoint" — i.e. deserialized, so its handle is `None`. Assigning
        // it wholesale would silently disconnect INV-SUPPLY-1 (#2097): the
        // supervisor would keep polling a latch nothing writes to and pass
        // forever, which is worse than not having the check, because the node
        // would report itself as watched. The handle belongs to the *node*, not
        // to any one state snapshot, so it survives the swap.
        let supply_invariant = self.state.supply_invariant.take();
        self.state = fork_state;
        if self.state.supply_invariant.is_none() {
            self.state.supply_invariant = supply_invariant;
        }

        // Apply the new fork's blocks.
        for block in &plan.blocks_to_apply {
            self.add_block(block.clone())?;
        }

        log::info!(
            "REORG complete: reverted {} blocks, applied {} new blocks, new tip height={}",
            plan.blocks_to_revert,
            plan.blocks_to_apply.len(),
            self.height,
        );

        // Report the completed reorg to the metrics subsystem (#1148).
        // This bumps `chain_reorg_total`, pins `chain_reorg_depth` to the
        // just-reverted depth, and clears `chain_fork_active` (we have
        // converged onto the new canonical tip).
        CORE_METRICS.inc_reorg(plan.blocks_to_revert);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a block with its hash properly set.
    fn make_block(version: u32, prev: Hash, txs: Vec<crate::blockchain::transaction::Transaction>, difficulty: u32, height: u64) -> Block {
        let mut block = Block::new(version, prev, txs, difficulty, height);
        let h = block.calculate_hash();
        block.set_hash(h);
        block
    }

    #[test]
    fn blocks_evicted_beyond_max_in_memory() {
        let mut chain = Blockchain::with_capacity(10);
        let total = 15u64;
        for h in 1..=total {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }
        assert_eq!(chain.in_memory_block_count(), 10);
        assert_eq!(chain.total_block_count(), total);
        // Oldest in-memory block should be height 6
        let oldest = chain.blocks().values().next().unwrap();
        assert_eq!(oldest.height, 6);
        // Newest should be the last added
        let newest = chain.blocks().values().next_back().unwrap();
        assert_eq!(newest.height, total);
    }

    #[test]
    fn blocks_evicted_at_configured_limit() {
        let mut chain = Blockchain::with_capacity(5);
        for h in 1..=10u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }
        assert_eq!(chain.in_memory_block_count(), 5);
        assert_eq!(chain.total_block_count(), 10);
        assert_eq!(chain.blocks().values().next().unwrap().height, 6);
    }

    #[test]
    fn configured_max_falls_back_to_default_for_invalid_values() {
        // Zero → default
        unsafe { std::env::set_var("WRFCOIN_MAX_IN_MEMORY_BLOCKS", "0"); }
        assert_eq!(configured_max_in_memory_blocks(), DEFAULT_MAX_IN_MEMORY_BLOCKS);
        // Non-numeric → default
        unsafe { std::env::set_var("WRFCOIN_MAX_IN_MEMORY_BLOCKS", "abc"); }
        assert_eq!(configured_max_in_memory_blocks(), DEFAULT_MAX_IN_MEMORY_BLOCKS);
        // Valid → used
        unsafe { std::env::set_var("WRFCOIN_MAX_IN_MEMORY_BLOCKS", "50"); }
        assert_eq!(configured_max_in_memory_blocks(), 50);
        unsafe { std::env::remove_var("WRFCOIN_MAX_IN_MEMORY_BLOCKS"); }
    }

    #[test]
    fn blocks_not_evicted_below_max() {
        let mut chain = Blockchain::default();
        for h in 1..=10u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }
        assert_eq!(chain.in_memory_block_count(), 10);
        assert_eq!(chain.total_block_count(), 10);
    }

    #[test]
    fn blocks_range_returns_correct_subset() {
        let mut chain = Blockchain::default();
        for h in 1..=20u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }
        let range: Vec<_> = chain.blocks_range(5, 10).collect();
        assert_eq!(range.len(), 6);
        assert_eq!(range.first().unwrap().height, 5);
        assert_eq!(range.last().unwrap().height, 10);
    }

    #[test]
    fn block_at_height_returns_correct_block() {
        let mut chain = Blockchain::default();
        for h in 1..=10u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }
        assert_eq!(chain.block_at_height(5).unwrap().height, 5);
        assert!(chain.block_at_height(0).is_none());
        assert!(chain.block_at_height(11).is_none());
    }

    // ── Hash validation tests (#1028) ────────────────────────────────────

    #[test]
    fn add_block_rejects_none_hash() {
        let mut chain = Blockchain::default();
        // Block without a hash should be rejected.
        let block = Block::new(1, Hash::default(), vec![], 1, 1);
        assert!(block.hash.is_none());
        let err = chain.add_block(block).unwrap_err();
        assert!(
            err.to_string().contains("no hash"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn batch_transitions_produce_same_root_as_sequential() {
        use crate::blockchain::state::StateTransition;
        use crate::common::types::{Address, Balance};

        let addr_a = Address([1u8; 20]);
        let addr_b = Address([2u8; 20]);

        // Create sequential chain: apply each transition individually (with state root per call)
        let mut sequential = Blockchain::default();
        sequential.state.create_account(addr_a, Balance::new(0));
        sequential.state.create_account(addr_b, Balance::new(0));

        let transitions = vec![
            StateTransition::ApplyReward {
                address: addr_a,
                amount: Balance::new(1000),
                reason: "test_reward_a".to_string(),
            },
            StateTransition::ApplyReward {
                address: addr_b,
                amount: Balance::new(2000),
                reason: "test_reward_b".to_string(),
            },
            StateTransition::ApplyReward {
                address: addr_a,
                amount: Balance::new(500),
                reason: "test_reward_a2".to_string(),
            },
        ];

        for t in transitions.clone() {
            sequential.apply_state_transition(t).unwrap();
        }

        // Create batch chain: apply all transitions at once
        let mut batch = Blockchain::default();
        batch.state.create_account(addr_a, Balance::new(0));
        batch.state.create_account(addr_b, Balance::new(0));
        batch.apply_state_transitions_batch(transitions).unwrap();

        assert_eq!(
            sequential.state.state_root, batch.state.state_root,
            "batch and sequential must produce identical state roots"
        );
    }

    /// Regression test for issue #980: find_fork_point_with_block_loader can
    /// locate a fork point that has been evicted from the in-memory window
    /// by falling back to the caller-supplied block loader.
    #[test]
    fn find_fork_point_via_storage_fallback() {
        // Use a small in-memory window (50) so the fork point is evicted
        // but still within MAX_REORG_DEPTH (100).
        // Chain height: 200, in-memory: heights 151..=200.
        let mut chain = Blockchain::with_capacity(50);
        let mut all_blocks: Vec<Block> = Vec::new();

        for h in 1..=200u64 {
            let mut block = Block::new(1, chain.tip, vec![], 1, h);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            all_blocks.push(block.clone());
            chain.add_block(block).unwrap();
        }

        // Fork at height 170 — in the in-memory window (151-200).
        // Basic find_fork_point should work.
        let fork_block_170 = &all_blocks[169]; // height 170
        let fork_hash_170 = fork_block_170.hash.unwrap();

        let mut candidate_170 = Block::new(1, fork_hash_170, vec![], 1, 171);
        let ch = candidate_170.calculate_hash();
        candidate_170.set_hash(ch);

        assert_eq!(chain.find_fork_point(&[candidate_170.clone()]), Some(170));

        // Fork at height 120 — outside the in-memory window (151-200)
        // but within MAX_REORG_DEPTH (200 - 120 = 80 < 100).
        let fork_block_120 = &all_blocks[119]; // height 120
        let fork_hash_120 = fork_block_120.hash.unwrap();

        let mut candidate_deep = Block::new(1, fork_hash_120, vec![], 1, 121);
        let cdh = candidate_deep.calculate_hash();
        candidate_deep.set_hash(cdh);

        // In-memory search must fail (height 120 was evicted).
        assert_eq!(chain.find_fork_point(&[candidate_deep.clone()]), None);

        // Storage-aware search should succeed via the block loader.
        let loader = |h: u64| -> Option<Block> {
            all_blocks.get((h as usize).checked_sub(1)?).cloned()
        };

        assert_eq!(
            chain.find_fork_point_with_block_loader(&[candidate_deep], loader),
            Some(120)
        );
    }

    /// Regression test for issue #1277: `plan_reorg_with_loader` must find
    /// the fork point via the disk-backed block loader when the fork point
    /// has been evicted from the in-memory window but is still within
    /// `MAX_REORG_DEPTH`. Without the loader, deep-but-valid reorgs would
    /// silently fail, stranding honest nodes on the wrong chain.
    #[test]
    fn plan_reorg_with_loader_resolves_evicted_fork_point() {
        use std::collections::HashMap;

        // In-memory window of 5 so that everything below height 16 is evicted.
        // Build a 20-block chain. Fork point at height 10 is outside memory
        // (min retained = 16) but well within MAX_REORG_DEPTH (20 - 10 = 10).
        let mut chain = Blockchain::with_capacity(5);
        let mut all_blocks: Vec<Block> = Vec::new();
        for h in 1..=20u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            all_blocks.push(block.clone());
            chain.add_block(block).unwrap();
        }

        // Simulated "disk" storage: every block ever added.
        let disk: HashMap<u64, Block> = all_blocks
            .iter()
            .cloned()
            .map(|b| (b.height, b))
            .collect();

        // Build a candidate fork from height 11 onward, rooted at the hash
        // of block 10 (evicted from memory).
        let fork_parent = &all_blocks[9]; // height 10
        assert!(
            !chain.blocks().contains_key(&10),
            "precondition: height 10 must be evicted from memory"
        );
        let fork_parent_hash = fork_parent.hash.unwrap();

        // Build a candidate chain of length 11 (heights 11..=21) so the
        // candidate tip strictly exceeds our tip (20) and a reorg is
        // preferred.
        let mut candidate_blocks: Vec<Block> = Vec::new();
        let mut prev_hash = fork_parent_hash;
        for h in 11..=21u64 {
            // Vary the block slightly (use a distinct difficulty) so the
            // candidate hash differs from the original chain.
            let block = make_block(1, prev_hash, vec![], 2, h);
            prev_hash = block.hash.unwrap();
            candidate_blocks.push(block);
        }

        // Baseline: `plan_reorg` (no loader) MUST fail to find the fork
        // because the fork point is evicted.
        let no_loader = chain.plan_reorg(&candidate_blocks).unwrap();
        assert!(
            no_loader.is_none(),
            "plan_reorg without loader must fail to locate an evicted fork point"
        );

        // With the loader, the reorg MUST be planned successfully.
        let with_loader = chain
            .plan_reorg_with_loader(&candidate_blocks, |h| disk.get(&h).cloned())
            .unwrap()
            .expect("plan_reorg_with_loader should find the evicted fork point on disk");

        assert_eq!(with_loader.fork_height, 10);
        assert_eq!(with_loader.blocks_to_revert, 10); // heights 11..=20
        assert_eq!(with_loader.blocks_to_apply.len(), candidate_blocks.len());
        // The plan must carry the (evicted) fork-point hash so `execute_reorg`
        // can install the correct tip without consulting in-memory state (#1973).
        assert_eq!(
            with_loader.fork_hash, fork_parent_hash,
            "plan must carry the disk-resolved fork-point hash"
        );
    }

    /// Regression test for issue #1973: `execute_reorg` must SUCCEED on a
    /// reorg whose fork point was resolved from disk by
    /// `plan_reorg_with_loader` and has been evicted from the in-memory
    /// window. Previously the executor consulted only `self.blocks` and
    /// `anyhow::bail!`ed with "Fork point block at height N not in memory",
    /// aborting a correctly-planned deep reorg — a liveness hazard whenever
    /// `WRFCOIN_MAX_IN_MEMORY_BLOCKS` < `MAX_REORG_DEPTH`. The planner
    /// honored disk; the executor did not. This asserts both now agree.
    #[test]
    fn execute_reorg_succeeds_with_disk_loaded_fork_point() {
        use std::collections::HashMap;

        // Tiny in-memory window (5) so that the fork point below height 16 is
        // evicted, yet still well within MAX_REORG_DEPTH (20 - 10 = 10).
        let mut chain = Blockchain::with_capacity(5);
        let mut all_blocks: Vec<Block> = Vec::new();
        for h in 1..=20u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            all_blocks.push(block.clone());
            chain.add_block(block).unwrap();
        }

        // Simulated disk: every block ever added.
        let disk: HashMap<u64, Block> = all_blocks
            .iter()
            .cloned()
            .map(|b| (b.height, b))
            .collect();

        // Fork point = height 10 (evicted from the 5-block window).
        let fork_parent = &all_blocks[9];
        let fork_parent_hash = fork_parent.hash.unwrap();
        assert!(
            !chain.blocks().contains_key(&10),
            "precondition: fork point height 10 must be evicted from memory"
        );

        // Build a longer candidate fork (heights 11..=21) rooted at block 10.
        let mut candidate_blocks: Vec<Block> = Vec::new();
        let mut prev_hash = fork_parent_hash;
        for h in 11..=21u64 {
            let block = make_block(1, prev_hash, vec![], 2, h);
            prev_hash = block.hash.unwrap();
            candidate_blocks.push(block);
        }
        let candidate_tip_hash = candidate_blocks.last().unwrap().hash.unwrap();

        // Plan via the disk-backed loader — this is the path that legitimately
        // resolves an evicted fork point.
        let plan = chain
            .plan_reorg_with_loader(&candidate_blocks, |h| disk.get(&h).cloned())
            .unwrap()
            .expect("plan_reorg_with_loader must plan the deep reorg");
        assert_eq!(plan.fork_height, 10);
        assert_eq!(plan.fork_hash, fork_parent_hash);

        // Execute. The fork point is NOT in memory, so the pre-fix code would
        // have bailed here. With #1973 it must succeed using plan.fork_hash.
        let fork_state = State::default();
        chain
            .execute_reorg(&plan, fork_state)
            .expect("execute_reorg must honor a disk-resolved fork point, not bail");

        // The chain must now sit on the new fork: tip = candidate tip, height
        // = candidate tip height (21).
        assert_eq!(
            chain.height, 21,
            "post-reorg height must be the candidate tip height"
        );
        assert_eq!(
            chain.tip, candidate_tip_hash,
            "post-reorg tip must be the candidate fork tip hash"
        );
        // The fork blocks must be installed in memory (subject to the window).
        assert_eq!(
            chain.block_at_height(21).unwrap().hash.unwrap(),
            candidate_tip_hash,
            "fork tip block must be installed in the in-memory window"
        );
    }

    /// Safety regression for issue #1277: even with a disk-backed loader,
    /// `plan_reorg_with_loader` must refuse forks whose fork point lies
    /// beyond `MAX_REORG_DEPTH`.
    #[test]
    fn plan_reorg_with_loader_respects_max_reorg_depth() {
        use std::collections::HashMap;

        // Build a 200-block chain with a small in-memory window.
        let mut chain = Blockchain::with_capacity(10);
        let mut all_blocks: Vec<Block> = Vec::new();
        for h in 1..=200u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            all_blocks.push(block.clone());
            chain.add_block(block).unwrap();
        }
        let disk: HashMap<u64, Block> = all_blocks
            .iter()
            .cloned()
            .map(|b| (b.height, b))
            .collect();

        // Fork at height 50 → 150 blocks deep, beyond MAX_REORG_DEPTH (100).
        let fork_parent = &all_blocks[49];
        let fork_parent_hash = fork_parent.hash.unwrap();

        let mut candidate_blocks: Vec<Block> = Vec::new();
        let mut prev_hash = fork_parent_hash;
        for h in 51..=201u64 {
            let block = make_block(1, prev_hash, vec![], 2, h);
            prev_hash = block.hash.unwrap();
            candidate_blocks.push(block);
        }

        let planned = chain
            .plan_reorg_with_loader(&candidate_blocks, |h| disk.get(&h).cloned())
            .unwrap();
        assert!(
            planned.is_none(),
            "plan_reorg_with_loader must refuse forks deeper than MAX_REORG_DEPTH even with a loader"
        );
    }

    /// find_fork_point_with_block_loader must respect MAX_REORG_DEPTH and
    /// refuse to search beyond it.
    #[test]
    fn find_fork_point_storage_respects_max_reorg_depth() {
        // Chain height: 200, capacity 50 (in-memory: 151-200).
        let mut chain = Blockchain::with_capacity(50);
        let mut all_blocks: Vec<Block> = Vec::new();

        for h in 1..=200u64 {
            let mut block = Block::new(1, chain.tip, vec![], 1, h);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            all_blocks.push(block.clone());
            chain.add_block(block).unwrap();
        }

        // Fork at height 50 is 150 blocks deep — beyond MAX_REORG_DEPTH (100).
        let fork_block_50 = &all_blocks[49]; // height 50
        let fork_hash_50 = fork_block_50.hash.unwrap();

        let mut candidate = Block::new(1, fork_hash_50, vec![], 1, 51);
        let ch = candidate.calculate_hash();
        candidate.set_hash(ch);

        let loader = |h: u64| -> Option<Block> {
            all_blocks.get((h as usize).checked_sub(1)?).cloned()
        };

        // Should return None because height 50 is beyond MAX_REORG_DEPTH.
        assert_eq!(
            chain.find_fork_point_with_block_loader(&[candidate], loader),
            None
        );
    }

    // ── add_block chain-continuity tests (#1018) ─────────────────────────

    #[test]
    fn add_block_rejects_wrong_height() {
        let mut chain = Blockchain::default();

        // Genesis block (first block) should succeed regardless of height.
        let mut genesis = Block::new(1, Hash::default(), vec![], 1, 1);
        let gh = genesis.calculate_hash();
        genesis.set_hash(gh);
        chain.add_block(genesis).unwrap();

        // Block at height 3 should fail (expected 2).
        let mut bad = Block::new(1, chain.tip, vec![], 1, 3);
        let bh = bad.calculate_hash();
        bad.set_hash(bh);
        let err = chain.add_block(bad).unwrap_err();
        assert!(
            err.to_string().contains("does not follow chain tip"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn add_block_rejects_wrong_parent_hash() {
        let mut chain = Blockchain::default();

        let mut genesis = Block::new(1, Hash::default(), vec![], 1, 1);
        let gh = genesis.calculate_hash();
        genesis.set_hash(gh);
        chain.add_block(genesis).unwrap();

        // Correct height but wrong prev_block_hash.
        let wrong_parent = Hash::default();
        let mut bad = Block::new(1, wrong_parent, vec![], 1, 2);
        let bh = bad.calculate_hash();
        bad.set_hash(bh);
        let err = chain.add_block(bad).unwrap_err();
        assert!(
            err.to_string().contains("prev_hash"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn add_block_accepts_valid_chain() {
        let mut chain = Blockchain::default();

        for h in 1..=5u64 {
            let mut block = Block::new(1, chain.tip, vec![], 1, h);
            let bh = block.calculate_hash();
            block.set_hash(bh);
            chain.add_block(block).unwrap();
        }
        assert_eq!(chain.height, 5);
        assert_eq!(chain.total_block_count, 5);
    }

    #[test]
    fn add_block_allows_first_block_at_any_height() {
        // The very first block (genesis) is not validated against a prior tip
        // because total_block_count == 0.
        let mut chain = Blockchain::default();
        let mut genesis = Block::new(1, Hash::default(), vec![], 1, 42);
        let gh = genesis.calculate_hash();
        genesis.set_hash(gh);
        chain.add_block(genesis).unwrap();
        assert_eq!(chain.height, 42);
    }

    // ---------------------------------------------------------------------------
    // #1735 hardening: reorg / fork-choice correctness
    // ---------------------------------------------------------------------------

    /// `execute_reorg` must:
    ///   1. Remove in-memory blocks above the fork point.
    ///   2. Reset chain tip and height to the fork point.
    ///   3. Install the provided `fork_state`.
    ///   4. Add the new fork blocks via `add_block()`.
    ///
    /// After execution the chain's `height`, `tip`, and block map must reflect
    /// the NEW fork, not the old chain — and the installed state_root must match
    /// a fresh computation over `fork_state` plus the fork's transitions.
    ///
    /// This is a launch-critical property: a wrong reorg leaves the node on the
    /// abandoned chain, causing silent network splits and invalid state_roots.
    #[test]
    fn execute_reorg_installs_fork_tip_and_state() {
        use crate::blockchain::state::StateTransition;
        use crate::common::types::{Address, Balance};

        let alice = Address([0xAAu8; 20]);
        let bob   = Address([0xBBu8; 20]);
        const FORK_HEIGHT: u64 = 3;

        // ── Build the MAIN chain: blocks 1–5, Alice gets 10 WRF per block ──
        let mut chain = Blockchain::default();
        chain.state.create_account(alice, Balance::new(0));
        chain.state.create_account(bob, Balance::new(0));

        let mut fork_state: Option<crate::blockchain::state::State> = None;

        for h in 1..=5u64 {
            // Apply state transition BEFORE the block (simulates block processing)
            chain.apply_state_transition(StateTransition::ApplyReward {
                address: alice,
                amount: Balance::new(10),
                reason: "main-block".to_string(),
            }).unwrap();

            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();

            if h == FORK_HEIGHT {
                // Capture state at the fork point (Alice = 30, Bob = 0).
                fork_state = Some(chain.state.clone());
            }
        }

        // Sanity-check main-chain state: Alice = 50 WRF (5 × 10).
        assert_eq!(
            chain.state.get_account(&alice).map(|a| a.balance),
            Some(Balance::new(50)),
            "main chain: Alice should have 50 WRF"
        );
        let main_tip_h5 = chain.tip;

        // ── Build the ALTERNATE fork: blocks 4'–6' from fork point (height 3) ──
        // Each block rewards Bob 20 WRF.  Fork is longer (height 6 > height 5).
        let fork_block3_hash = chain.block_at_height(FORK_HEIGHT).unwrap().hash.unwrap();
        let mut alt4 = make_block(1, fork_block3_hash, vec![], 1, 4);
        let mut alt5 = make_block(1, alt4.hash.unwrap(), vec![], 1, 5);
        let mut alt6 = make_block(1, alt5.hash.unwrap(), vec![], 1, 6);
        let alt6_hash = alt6.hash.unwrap();
        let alt_blocks = vec![alt4.clone(), alt5.clone(), alt6.clone()];

        // ── Plan the reorg ──
        let plan = chain
            .plan_reorg(&alt_blocks)
            .expect("plan_reorg must not error")
            .expect("a longer fork must yield a reorg plan");

        assert_eq!(plan.fork_height, FORK_HEIGHT);
        assert_eq!(plan.blocks_to_revert, 2); // revert blocks 4 and 5
        assert_eq!(plan.blocks_to_apply.len(), 3); // apply blocks 4', 5', 6'

        // ── Execute the reorg ──
        let fs = fork_state.unwrap();
        let fork_root_before_reorg = fs.state_root;
        chain.execute_reorg(&plan, fs).unwrap();

        // ── Post-reorg chain shape ──
        assert_eq!(chain.height, 6, "new chain height must be 6 after reorg");
        assert_eq!(chain.tip, alt6_hash, "chain tip must be alt block 6's hash");
        assert!(
            chain.block_at_height(4).unwrap().hash.unwrap() != main_tip_h5,
            "block 4 must be the fork block, not the old chain block"
        );
        assert_eq!(
            chain.block_at_height(4).unwrap().hash.unwrap(),
            alt4.hash.unwrap(),
            "in-memory block 4 must be the fork block"
        );
        assert_eq!(
            chain.block_at_height(6).unwrap().hash.unwrap(),
            alt6_hash,
            "in-memory block 6 must be the fork tip"
        );

        // ── Post-reorg state ──
        // State is fork_state (Alice = 30, Bob = 0); fork transitions NOT yet applied.
        assert_eq!(
            chain.state.state_root, fork_root_before_reorg,
            "state_root must equal fork_state root immediately after execute_reorg"
        );
        assert_eq!(
            chain.state.get_account(&alice).map(|a| a.balance),
            Some(Balance::new(30)),
            "Alice must be at fork-point balance (30 WRF) after reorg"
        );
        assert_eq!(
            chain.state.get_account(&bob).map(|a| a.balance),
            Some(Balance::new(0)),
            "Bob must be 0 immediately after reorg (fork transitions not yet applied)"
        );

        // ── Apply fork transitions and verify state_root determinism ──
        // Simulate applying the 3 fork blocks' rewards to Bob (20 WRF each).
        // Build an independent expected chain from the same fork_state_clone
        // so the comparison starts from identical base state (not a stripped State::new()).
        let fork_state_clone = chain.state.clone(); // currently == fork_state after reorg
        let mut expected = Blockchain::default();
        expected.state = fork_state_clone;

        for _ in 0..3 {
            let t = StateTransition::ApplyReward {
                address: bob,
                amount: Balance::new(20),
                reason: "fork-block".to_string(),
            };
            chain.apply_state_transition(t.clone()).unwrap();
            expected.apply_state_transition(t).unwrap();
        }

        assert_eq!(
            chain.state.get_account(&bob).map(|a| a.balance),
            Some(Balance::new(60)),
            "Bob must have 60 WRF after 3 fork transitions"
        );
        assert_eq!(
            chain.state.state_root, expected.state.state_root,
            "post-reorg state_root must match independently computed expected root"
        );
    }

    /// A reorg must not silently disconnect INV-SUPPLY-1 (#2097).
    ///
    /// `execute_reorg`'s own contract says `fork_state` is "typically loaded
    /// from a storage checkpoint" — deserialized, so its `supply_invariant` is
    /// `None` because the field is `#[serde(skip)]`. Installing it wholesale
    /// would leave the node's supervisor polling a latch nothing writes to:
    /// green forever, watching nothing. That is worse than an absent check,
    /// because the node reports itself as supervised.
    #[test]
    fn execute_reorg_preserves_the_supply_invariant_handle() {
        use crate::blockchain::state::{State, StateTransition};
        use crate::common::types::{Address, Balance};
        use crate::invariants::StatefulInvariant;
        use crate::invariants::balance::SharedSupplyInvariant;

        let alice = Address([0xAAu8; 20]);
        let handle = SharedSupplyInvariant::new();

        let mut chain = Blockchain::default();
        chain.state.create_account(alice, Balance::new(0));
        chain.state.supply_invariant = Some(handle.clone());

        let mut fork_state: Option<State> = None;
        for h in 1..=3u64 {
            chain
                .apply_state_transition(StateTransition::ApplyReward {
                    address: alice,
                    amount: Balance::new(10),
                    reason: "main-block".to_string(),
                })
                .unwrap();
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
            if h == 2 {
                // Stand in for a checkpoint load: same state, no handle —
                // which is exactly what deserializing gives you, the field
                // being `#[serde(skip)]`.
                let mut restored = chain.state.clone();
                restored.supply_invariant = None;
                fork_state = Some(restored);
            }
        }

        let fork_block2_hash = chain.block_at_height(2).unwrap().hash.unwrap();
        let alt3 = make_block(1, fork_block2_hash, vec![], 1, 3);
        let alt4 = make_block(1, alt3.hash.unwrap(), vec![], 1, 4);
        let plan = chain
            .plan_reorg(&[alt3, alt4])
            .expect("plan_reorg must not error")
            .expect("a longer fork must yield a plan");

        chain.execute_reorg(&plan, fork_state.unwrap()).unwrap();

        assert!(
            chain.state.supply_invariant.is_some(),
            "the node's supply handle must survive the state swap"
        );
        // …and it must be the SAME instance, not a fresh one: a violation
        // written through the post-reorg state has to be visible on the handle
        // the supervisor already holds.
        chain
            .state
            .supply_invariant
            .as_ref()
            .unwrap()
            .observe_violation("probe".to_string());
        assert!(
            handle.check().is_err(),
            "the surviving handle must be the instance the supervisor polls"
        );
    }

    /// `plan_reorg` must reject a fork that does not extend past the finalized
    /// height (BFT safety guard, issue #1216).
    ///
    /// A peer advertising a chain whose fork point is BELOW the locally
    /// finalized height must be silently ignored — executing such a reorg would
    /// violate the 2/3+ safety guarantee of the BFT finality gadget.
    #[test]
    fn plan_reorg_rejects_fork_below_finalized_height() {
        use crate::blockchain::state::StateTransition;
        use crate::common::types::{Address, Balance};

        let alice = Address([0xCCu8; 20]);

        // Build a 5-block main chain.
        let mut chain = Blockchain::default();
        chain.state.create_account(alice, Balance::new(0));

        for h in 1..=5u64 {
            chain.apply_state_transition(StateTransition::ApplyReward {
                address: alice,
                amount: Balance::new(10),
                reason: "block".to_string(),
            }).unwrap();
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }

        // Finalize up to height 4 — no fork below height 4 is allowed.
        chain.finalized_height = 4;

        // Build an alternate fork that diverges at height 2 (below finalized 4).
        let fork_block2_hash = chain.block_at_height(2).unwrap().hash.unwrap();
        let alt3 = make_block(1, fork_block2_hash, vec![], 1, 3);
        let alt4 = make_block(1, alt3.hash.unwrap(), vec![], 1, 4);
        let alt5 = make_block(1, alt4.hash.unwrap(), vec![], 1, 5);
        let alt6 = make_block(1, alt5.hash.unwrap(), vec![], 1, 6);

        let result = chain.plan_reorg(&[alt3, alt4, alt5, alt6]).unwrap();

        assert!(
            result.is_none(),
            "plan_reorg must return None for forks below finalized_height (BFT safety)"
        );
    }

    // ── #1735 hardening: finality edge cases ────────────────────────────────

    /// `set_finalized_height` must be monotonic: advancing is accepted,
    /// reverting is silently ignored, and equal values are no-ops.
    ///
    /// Monotonicity is the BFT safety property: once a height is finalized
    /// a lower signal (e.g. from a lagging peer or replay) must never
    /// roll back the guard.  If it did, `plan_reorg` could be tricked into
    /// accepting a deep reorg that violates the 2/3+ finality guarantee.
    #[test]
    fn set_finalized_height_is_monotonic() {
        let mut chain = Blockchain::default();
        assert_eq!(chain.finalized_height(), 0, "starts at 0");

        // Advance forward: 0 → 5
        chain.set_finalized_height(5);
        assert_eq!(chain.finalized_height(), 5);

        // Advance further: 5 → 10
        chain.set_finalized_height(10);
        assert_eq!(chain.finalized_height(), 10);

        // Same value: no-op
        chain.set_finalized_height(10);
        assert_eq!(chain.finalized_height(), 10, "same value must not change state");

        // Revert attempt: 10 → 3 must be ignored
        chain.set_finalized_height(3);
        assert_eq!(
            chain.finalized_height(),
            10,
            "lower height must be silently ignored (monotonicity)"
        );

        // Zero: always below current, must be ignored
        chain.set_finalized_height(0);
        assert_eq!(chain.finalized_height(), 10, "zero must not reset finalized_height");
    }

    /// `plan_reorg` must ALLOW a fork whose fork point equals `finalized_height`
    /// (fork AT the finalized block) — only forks BELOW it are unsafe.
    ///
    /// The guard condition is `fork_height < finalized_height`; a fork that
    /// diverges at exactly the finalized block is still valid because the
    /// finalized block itself is agreed upon.  Blocking it would prevent a
    /// node from following the canonical chain after it finalized a block
    /// that later got reorged at the boundary.
    #[test]
    fn plan_reorg_allows_fork_at_finalized_height() {
        // Build a 5-block chain, finalize up to height 3.
        let mut chain = Blockchain::default();
        for h in 1..=5u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }
        chain.set_finalized_height(3);

        // Fork that diverges at height 3 (block 3 is the common ancestor).
        // The fork is longer (heights 4'–6') so a reorg should be planned.
        let fork_block3_hash = chain.block_at_height(3).unwrap().hash.unwrap();
        let alt4 = make_block(1, fork_block3_hash, vec![], 2, 4);
        let alt5 = make_block(1, alt4.hash.unwrap(), vec![], 2, 5);
        let alt6 = make_block(1, alt5.hash.unwrap(), vec![], 2, 6);

        let plan = chain
            .plan_reorg(&[alt4, alt5, alt6])
            .expect("plan_reorg must not error for fork at finalized_height");

        assert!(
            plan.is_some(),
            "fork at finalized_height ({}) must be allowed (only BELOW is unsafe)",
            chain.finalized_height()
        );
        let plan = plan.unwrap();
        assert_eq!(plan.fork_height, 3);
        assert_eq!(plan.blocks_to_revert, 2); // revert blocks 4 and 5
        assert_eq!(plan.blocks_to_apply.len(), 3);
    }

    // ── #1735 hardening: snapshot / IBD restart correctness ─────────────────

    /// `from_persisted` must reconstruct a chain with identical height, tip,
    /// state_root, finalized_height, and total_block_count.
    ///
    /// This is the hot path for IBD restart: the node serialises these fields
    /// to disk and calls `from_persisted` on the next boot.  If any field is
    /// not faithfully restored, the node boots into an inconsistent state
    /// (wrong tip or stale state_root) and will be rejected by peers.
    #[test]
    fn from_persisted_restores_identical_chain_state() {
        use crate::blockchain::state::StateTransition;
        use crate::common::types::{Address, Balance};

        let alice = Address([0xAA_u8; 20]);

        // Build a live chain to height 5 with known state.
        let mut chain = Blockchain::default();
        chain.state.create_account(alice, Balance::new(0));

        for h in 1..=5u64 {
            chain
                .apply_state_transition(StateTransition::ApplyReward {
                    address: alice,
                    amount: Balance::new(100),
                    reason: "ibd-test".to_string(),
                })
                .unwrap();
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }
        chain.finalized_height = 3; // simulate BFT finalization

        // Capture fields that would be persisted to disk.
        let saved_height = chain.height;
        let saved_tip = chain.tip;
        let saved_state = chain.state.clone();
        let saved_blocks = chain.blocks().clone();
        let saved_total = chain.total_block_count();
        let saved_fin = chain.finalized_height;

        // Reconstruct from persisted data (simulates node restart / IBD).
        let restored = Blockchain::from_persisted(
            saved_height,
            saved_tip,
            saved_state,
            saved_blocks,
            saved_total,
            saved_fin,
        );

        assert_eq!(restored.height, chain.height, "height must be preserved");
        assert_eq!(restored.tip, chain.tip, "tip must be preserved");
        assert_eq!(
            restored.state.state_root,
            chain.state.state_root,
            "state_root must be preserved exactly (consensus-critical)"
        );
        assert_eq!(
            restored.total_block_count(),
            chain.total_block_count(),
            "total_block_count must be preserved (includes evicted blocks)"
        );
        assert_eq!(
            restored.finalized_height, chain.finalized_height,
            "finalized_height must be preserved (BFT safety)"
        );
    }

    /// After `from_persisted`, `add_block` must continue the chain normally.
    ///
    /// A restart that cannot accept new blocks would strand the node.  This
    /// test verifies that the reconstructed chain correctly validates height
    /// and parent-hash continuity for the next incoming block.
    #[test]
    fn from_persisted_chain_accepts_subsequent_blocks() {
        use crate::blockchain::state::StateTransition;
        use crate::common::types::{Address, Balance};

        let alice = Address([0xBB_u8; 20]);

        let mut chain = Blockchain::default();
        chain.state.create_account(alice, Balance::new(0));

        for h in 1..=3u64 {
            chain
                .apply_state_transition(StateTransition::ApplyReward {
                    address: alice,
                    amount: Balance::new(50),
                    reason: "pre-restart".to_string(),
                })
                .unwrap();
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }

        let saved_tip = chain.tip;
        let expected_next_height = chain.height + 1;

        // Restore chain.
        let mut restored = Blockchain::from_persisted(
            chain.height,
            chain.tip,
            chain.state.clone(),
            chain.blocks().clone(),
            chain.total_block_count(),
            chain.finalized_height,
        );

        // Block at height 4 rooted at the saved tip must be accepted.
        let next_block = make_block(1, saved_tip, vec![], 1, expected_next_height);
        restored.add_block(next_block.clone()).unwrap();

        assert_eq!(
            restored.height, expected_next_height,
            "height must advance after restart"
        );
        assert_eq!(
            restored.tip,
            next_block.hash.unwrap(),
            "tip must advance after restart"
        );
        assert_eq!(
            restored.total_block_count(),
            chain.total_block_count() + 1,
            "total_block_count must include the post-restart block"
        );
    }

    /// `from_persisted` with a non-zero `total_block_count` (representing
    /// evicted blocks that are only on disk) must preserve the count faithfully.
    ///
    /// If `total_block_count` is lost on restart, the node would report an
    /// incorrect chain length to peers and could be caught in sync loops.
    #[test]
    fn from_persisted_preserves_total_block_count_with_evicted_blocks() {
        // Build a chain with in-memory cap of 3; add 8 blocks.
        // After adding: in-memory holds heights 6-8, evicted = 5, total = 8.
        let mut chain = Blockchain::with_capacity(3);

        for h in 1..=8u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }

        assert_eq!(chain.total_block_count(), 8, "precondition: 8 blocks total");
        assert_eq!(
            chain.in_memory_block_count(),
            3,
            "precondition: only 3 blocks in memory"
        );

        // On restart, only the in-memory window is passed in (evicted blocks
        // live on disk), but total_block_count comes from persisted metadata.
        let restored = Blockchain::from_persisted(
            chain.height,
            chain.tip,
            chain.state.clone(),
            chain.blocks().clone(), // in-memory slice only
            chain.total_block_count(), // full count from persisted metadata
            chain.finalized_height,
        );

        assert_eq!(
            restored.total_block_count(),
            8,
            "total_block_count must survive restart regardless of in-memory window"
        );
        assert_eq!(
            restored.in_memory_block_count(),
            3,
            "only the passed-in blocks are in memory after restore"
        );
        assert_eq!(restored.height, 8, "height must reflect full chain length");
    }

    // ── #3370: which plans replace block 1 ──────────────────────────────────

    /// Only a genesis fork on a chain that already holds block 1 replaces it;
    /// a fork at height >= 1 (every ordinary reorg) never does.
    #[test]
    fn replaces_block_one_only_for_a_genesis_fork_over_an_existing_block_one_3370() {
        let mut chain = Blockchain::default();
        for h in 1..=3u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }
        let local_1 = chain.block_at_height(1).unwrap().hash.unwrap();

        // Genesis fork, longer than local: replaces block 1.
        let mut genesis_fork = Vec::new();
        let mut prev = Hash::default();
        for h in 1..=4u64 {
            let blk = make_block(1, prev, vec![], 2, h);
            prev = blk.hash.unwrap();
            genesis_fork.push(blk);
        }
        let plan = chain.plan_reorg(&genesis_fork).unwrap().expect("planner reports it");
        assert_eq!(plan.fork_height, 0);
        assert!(plan.replaces_block_one());

        // Fork at height 1 (shares block 1): an ordinary reorg.
        let mut fork_at_1 = Vec::new();
        let mut prev = local_1;
        for h in 2..=4u64 {
            let blk = make_block(1, prev, vec![], 2, h);
            prev = blk.hash.unwrap();
            fork_at_1.push(blk);
        }
        let plan = chain.plan_reorg(&fork_at_1).unwrap().expect("ordinary reorg is planned");
        assert_eq!(plan.fork_height, 1);
        assert!(!plan.replaces_block_one());

        // A genesis-anchored plan on an empty chain reverts nothing.
        let empty = ReorgPlan {
            fork_height: 0,
            fork_hash: Hash::default(),
            blocks_to_revert: 0,
            blocks_to_apply: genesis_fork,
        };
        assert!(!empty.replaces_block_one());
    }

    // ── #1735 hardening: MAX_REORG_DEPTH boundary tests ─────────────────────

    /// A fork exactly `MAX_REORG_DEPTH` (100) blocks deep must be ACCEPTED.
    ///
    /// The rejection condition is strictly `blocks_to_revert > MAX_REORG_DEPTH`,
    /// so depth 100 is the last value that must succeed.
    #[test]
    fn plan_reorg_accepts_at_exactly_max_reorg_depth_boundary() {
        // 100-block main chain; all blocks in memory (DEFAULT_MAX_IN_MEMORY_BLOCKS = 200).
        let mut chain = Blockchain::default();
        for h in 1..=100u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }
        assert_eq!(chain.height, 100, "precondition: chain at height 100");

        // Fork from genesis (height 0) using difficulty=2 so hashes differ
        // from the main chain.  Candidate tip height 101 > 100 → preferred.
        let mut candidate_blocks = Vec::new();
        let mut prev = Hash::default();
        for h in 1..=101u64 {
            let blk = make_block(1, prev, vec![], 2, h);
            prev = blk.hash.unwrap();
            candidate_blocks.push(blk);
        }

        let result = chain.plan_reorg(&candidate_blocks).unwrap();
        assert!(
            result.is_some(),
            "reorg exactly MAX_REORG_DEPTH (100) blocks deep must be accepted"
        );
        let plan = result.unwrap();
        assert_eq!(plan.fork_height, 0, "fork point must be at genesis (height 0)");
        assert_eq!(
            plan.blocks_to_revert, 100,
            "blocks_to_revert must equal exactly MAX_REORG_DEPTH"
        );
    }

    /// A fork `MAX_REORG_DEPTH + 1` (101) blocks deep must be REJECTED.
    ///
    /// Depth 101 satisfies `blocks_to_revert > MAX_REORG_DEPTH` → `None`.
    #[test]
    fn plan_reorg_rejects_fork_one_block_beyond_max_reorg_depth() {
        // 101-block main chain: fork depth from genesis = 101 > MAX_REORG_DEPTH.
        let mut chain = Blockchain::default();
        for h in 1..=101u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }
        assert_eq!(chain.height, 101, "precondition: chain at height 101");

        // Candidate from genesis; tip at height 102 is longer than local.
        let mut candidate_blocks = Vec::new();
        let mut prev = Hash::default();
        for h in 1..=102u64 {
            let blk = make_block(1, prev, vec![], 2, h);
            prev = blk.hash.unwrap();
            candidate_blocks.push(blk);
        }

        let result = chain.plan_reorg(&candidate_blocks).unwrap();
        assert!(
            result.is_none(),
            "reorg 101 blocks deep (MAX_REORG_DEPTH + 1) must be rejected"
        );
    }

    /// At equal tip height, a candidate whose hash equals the local tip hash
    /// must be rejected by the tie-break rule.
    ///
    /// The tie-break condition is `candidate_tip_hash >= self.tip → None`.
    /// Equal hashes are NOT strictly lower, so the local chain wins.
    #[test]
    fn plan_reorg_tiebreak_equal_height_same_hash_rejected() {
        // 5-block chain.
        let mut chain = Blockchain::default();
        for h in 1..=5u64 {
            let block = make_block(1, chain.tip, vec![], 1, h);
            chain.add_block(block).unwrap();
        }

        // Re-submit the exact tip block as the sole candidate.
        // candidate_tip_height == 5 == chain.height; hash == chain.tip.
        // Tie-break: candidate_tip_hash >= self.tip → rejected.
        let tip_block = chain.blocks().get(&5).unwrap().clone();
        let result = chain.plan_reorg(&[tip_block]).unwrap();
        assert!(
            result.is_none(),
            "candidate with hash equal to local tip must be rejected by the tie-break rule (>= rejects equal)"
        );
    }
}
