//! Historical-block replay verification (#2750).
//!
//! # What this proves
//!
//! That the **candidate binary this harness was compiled from** can replay a
//! persisted chain as a syncing follower and reproduce every state root the
//! historical producer committed. It executes, per block, exactly the apply
//! sequence `apply_p2p_block_at` runs when a peer syncs:
//! [`crate::transition_extractor::extract_transitions`] →
//! [`State::apply_transitions_batch`] → [`State::apply_block_finalization`] →
//! root comparison against the block's committed
//! [`STATE_ROOT_METADATA_KEY`] → [`Blockchain::add_block`]. If a height-gated
//! consensus change perturbs any follower-visible path *below* its gate, the
//! replay diverges at the first affected height and this harness names it.
//!
//! # What this does NOT prove
//!
//! - **Producer-only calculus is reached only through its committed outputs.**
//!   Reward decisions ride in the block as serialized records
//!   (`reward_entries` metadata) that the follower re-applies as data — the
//!   deliberate producer==applier==WAL design. A mutation confined to the
//!   producer's reward arithmetic would change what blocks *would be built*,
//!   not how historical blocks replay; catching that class is producer-parity
//!   territory (`parity.rs`), not replay territory.
//! - **The stored genesis state is the axiom, not a conclusion.** Replay
//!   starts from `load_genesis_state()`; everything after height 0 is
//!   re-executed, the genesis content itself is trusted as stored.
//! - **A green run over a fixture proves the fixture's shapes only.** The
//!   committed fixture covers registration/commit/reveal/attestation/reward
//!   blocks; a transaction type the fixture never carries is not exercised.
//!
//! # Honest failure modes
//!
//! The recurring defect this repo guards against is absence rendering as
//! success, so: no genesis state → UNVERIFIED; zero persisted blocks →
//! UNVERIFIED; a replay in which **no block carried a well-formed root
//! commitment** (so nothing was actually asserted) → UNVERIFIED. Only a
//! replay that asserted at least one committed root can report Verified. A v1
//! replay with matching stored checkpoints instead reports VerifiedByCheckpoint
//! with a separate exit code and no per-block commitment claim.
//!
//! # Where it runs
//!
//! - CI: `native-chain/tests/replay_harness_2750.rs` builds a committed
//!   fixture chain through the real stateful producer and replays it — small
//!   (a dozen blocks), runs in the ordinary test suite on every PR.
//! - Operators: the `replay_verify` binary against a **copy** of a node data
//!   directory (never the live one — redb is single-writer) replays the real
//!   chain, including blocks 674/716/851. Not CI-runnable (needs chain data
//!   and the chain's env identity); run it before deploying any height-gated
//!   consensus change.
//!
//! # Effective feature schedule
//!
//! Like the node, the replay refreshes the process-global effective feature
//! schedule (base schedule + applied #3277 schedule extensions) from the
//! anchor state before the first block and after every applied block, via
//! [`refresh_schedule_from_canonical_state`], the #3277 S3 refresh the node's
//! block-apply sites call (#3332).
//!
//! [`STATE_ROOT_METADATA_KEY`]: wrfcoin_native_chain::blockchain::block::STATE_ROOT_METADATA_KEY

use wrfcoin_native_chain::blockchain::block::{Block, STATE_ROOT_METADATA_KEY};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::state::State;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_native_chain::upgrade::refresh_schedule_from_canonical_state;
use wrfcoin_shared_protocol::crypto::chain_binding::{SigningProtocolVersion, protocol_version_at};

use crate::transition_extractor::extract_transitions;

/// What applying one block's transitions in two orders does (see
/// [`probe_governance_order`]). Each root is the post-finalization state root,
/// or the error the batch failed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernanceOrderProbe {
    /// Entries in the block's `governance_transitions` metadata.
    pub governance_transitions: usize,
    /// Commit, reveal, oracle-attestation and node-service transactions in
    /// the block.
    pub launch_transactions: usize,
    /// Transitions the follower applies ahead of the governance batch, not
    /// counting the block-opening ones. Zero means the two orders are the
    /// same list and nothing was applied.
    pub transitions_before_governance: usize,
    /// The follower's order today: launch transitions, then governance.
    pub launch_first: Result<String, String>,
    /// The producer's order: governance, then launch transitions.
    pub governance_first: Result<String, String>,
    /// The state root the block itself commits to, when it carries one.
    pub committed_state_root: Option<String>,
}

impl GovernanceOrderProbe {
    /// Whether the follower's order reproduces the root the block commits to.
    /// `None` when the block carries no commitment, or when the two orders are
    /// the same list (nothing was applied here; the replay itself asserts the
    /// commitment).
    pub fn launch_first_matches_committed(&self) -> Option<bool> {
        if self.transitions_before_governance == 0 {
            return None;
        }
        let committed = self.committed_state_root.as_ref()?;
        Some(matches!(&self.launch_first, Ok(root) if root == committed))
    }

    /// Both orders applied and reached the same state root (or are the same
    /// list).
    pub fn orders_agree(&self) -> bool {
        self.transitions_before_governance == 0
            || matches!((&self.launch_first, &self.governance_first), (Ok(a), Ok(b)) if a == b)
    }
}

/// For a block whose `governance_transitions` metadata is non-empty: apply the
/// block's extracted transitions to two copies of the pre-block `state`, once
/// in the follower's order (launch transitions before the governance batch)
/// and once in the producer's (governance batch first), and report both
/// resulting state roots. `None` for a block with no governance transitions.
///
/// Read-only: `state` and `block` are not changed. It answers one question
/// about a chain's history: has the difference between the two orders ever
/// mattered? If no replayed block disagrees, making the orders one can be a
/// forward-only change.
pub fn probe_governance_order(state: &State, block: &Block) -> Option<GovernanceOrderProbe> {
    use wrfcoin_native_chain::blockchain::state::StateTransition;
    use wrfcoin_native_chain::blockchain::transaction::TransactionType;

    let governance: Vec<StateTransition> =
        serde_json::from_str(block.metadata.get("governance_transitions")?).ok()?;
    if governance.is_empty() {
        return None;
    }
    let launch_transactions = block
        .transactions
        .iter()
        .filter(|tx| {
            matches!(
                tx.tx_type,
                TransactionType::ContributionCommit
                    | TransactionType::ForecastCommit
                    | TransactionType::ContributionReveal
                    | TransactionType::ForecastReveal
                    | TransactionType::OracleAttestation
                    | TransactionType::NodeServiceBatch
                    | TransactionType::NodeFraudReport
                    | TransactionType::NodeBondLifecycle
            )
        })
        .count();

    let committed_state_root = block.metadata.get(STATE_ROOT_METADATA_KEY).cloned();
    let height = block.height;
    let extracted = extract_transitions(state, block, height, block.timestamp.timestamp());
    let as_json = |t: &StateTransition| serde_json::to_string(t).unwrap_or_default();
    let extracted_json: Vec<String> = extracted.iter().map(as_json).collect();
    let governance_json: Vec<String> = governance.iter().map(as_json).collect();
    let opening = extracted
        .iter()
        .take_while(|t| {
            matches!(
                t,
                StateTransition::BeginNodeServiceBlock { .. }
                    | StateTransition::RecordGenesisEmissionConfig { .. }
            )
        })
        .count();
    let Some(start) = (opening..=extracted.len().saturating_sub(governance.len()))
        .find(|&i| extracted_json[i..i + governance.len()] == governance_json[..])
    else {
        let missing = Err("the governance batch was not found among the extracted transitions"
            .to_string());
        return Some(GovernanceOrderProbe {
            governance_transitions: governance.len(),
            launch_transactions,
            transitions_before_governance: usize::MAX,
            launch_first: missing.clone(),
            governance_first: missing,
            committed_state_root,
        });
    };
    let end = start + governance.len();
    let transitions_before_governance = start - opening;

    let root_after = |order: Vec<StateTransition>| -> Result<String, String> {
        let mut copy = state.clone();
        copy.apply_transitions_batch_at_height(height, order)
            .map_err(|e| e.to_string())?;
        copy.apply_block_finalization(height);
        Ok(copy.state_root.to_string())
    };
    let (launch_first, governance_first) = if transitions_before_governance == 0 {
        let same = Ok("<same order>".to_string());
        (same.clone(), same)
    } else {
        let mut reordered = Vec::with_capacity(extracted.len());
        reordered.extend_from_slice(&extracted[..opening]);
        reordered.extend_from_slice(&extracted[start..end]);
        reordered.extend_from_slice(&extracted[opening..start]);
        reordered.extend_from_slice(&extracted[end..]);
        (root_after(extracted.clone()), root_after(reordered))
    };
    Some(GovernanceOrderProbe {
        governance_transitions: governance.len(),
        launch_transactions,
        transitions_before_governance,
        launch_first,
        governance_first,
        committed_state_root,
    })
}

/// Outcome of a replay run. `Unverified` is deliberately distinct from both
/// success and divergence: it means the run **asserted nothing**, and a run
/// that asserted nothing must never read as a pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayVerdict {
    /// Every replayed block that carried a root commitment matched it.
    Verified {
        blocks_replayed: u64,
        /// How many per-block root assertions actually executed. Always ≥ 1 —
        /// a zero here is reported as `Unverified`, never `Verified`.
        roots_asserted: u64,
        /// Checkpoint-state anchors additionally compared (stored checkpoint
        /// state root vs replayed root at the same height).
        checkpoints_asserted: u64,
        first_height: u64,
        last_height: u64,
    },
    /// No per-block root commitments existed, but stored checkpoint anchors matched.
    /// This is weaker evidence than `Verified` and has its own nonzero CLI code.
    VerifiedByCheckpoint {
        blocks_replayed: u64,
        checkpoints_asserted: u64,
        first_height: u64,
        last_height: u64,
        final_replayed_root: String,
    },
    /// The replay could not assert anything. The reason names what was
    /// missing (fixture, blocks, commitments, or chain continuity).
    Unverified { reason: String },
    /// The candidate binary does not reproduce the historical chain. `height`
    /// is the FIRST height at which divergence was observed.
    Diverged {
        height: u64,
        /// What the historical producer committed (or a description of the
        /// apply failure when the block could not even be applied).
        committed: String,
        /// What this binary's replay computed.
        applied: String,
        blocks_replayed: u64,
    },
}

impl ReplayVerdict {
    /// CLI codes: 0 per-block verified, 1 diverged, 2 unverified, 3 checkpoint evidence.
    /// Unverified is nonzero on purpose — absence must not exit like success.
    pub fn exit_code(&self) -> i32 {
        match self {
            ReplayVerdict::Verified { .. } => 0,
            ReplayVerdict::Diverged { .. } => 1,
            ReplayVerdict::Unverified { .. } => 2,
            ReplayVerdict::VerifiedByCheckpoint { .. } => 3,
        }
    }
}

/// A checkpoint comparison actually executed at this height, including mismatches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointAssertion {
    pub height: u64,
    pub checkpoint_root: String,
    pub replayed_root: String,
    pub matched: bool,
}

/// Replay with an observer for every stored checkpoint comparison (#3317).
/// Diagnostics use this even when the final verdict is not per-block verified.
pub fn replay_persisted_chain_with_checkpoint_observer<F>(
    storage: &Storage,
    up_to: Option<u64>,
    checkpoint_observe: F,
) -> ReplayVerdict
where
    F: FnMut(&CheckpointAssertion),
{
    replay_persisted_chain_observed_and_checkpoints(storage, up_to, |_, _| {}, |_, _| {}, checkpoint_observe)
}

/// [`replay_persisted_chain_with_checkpoint_observer`] with a hook that sees
/// every block together with the state BEFORE it, ahead of
/// `extract_transitions`. The hook cannot change either, so the replay itself
/// is the same one. `replay_verify --compare-metadata` uses it to count blocks
/// whose producer-written metadata differs from this binary's own derivation.
pub fn replay_persisted_chain_with_block_precheck<P, C>(
    storage: &Storage,
    up_to: Option<u64>,
    precheck: P,
    checkpoint_observe: C,
) -> ReplayVerdict
where
    P: FnMut(&State, &Block),
    C: FnMut(&CheckpointAssertion),
{
    replay_persisted_chain_observed_and_checkpoints(storage, up_to, precheck, |_, _| {}, checkpoint_observe)
}

/// Replay the persisted chain in `storage` from its stored genesis state up
/// to `up_to` (inclusive; `None` = everything persisted), asserting every
/// committed state root along the way.
pub fn replay_persisted_chain(storage: &Storage, up_to: Option<u64>) -> ReplayVerdict {
    replay_persisted_chain_observed(storage, up_to, |_, _| {})
}

/// [`replay_persisted_chain`] with a per-block observer, called after each
/// block's transitions have been applied and BEFORE its finalization (so the
/// observer's changes are folded into that block's recomputed root, which is
/// then compared).
///
/// The production entry point passes a no-op. The observer exists so the
/// harness can be **shown to fail**: the #2750 RED-control test perturbs the
/// replaying state at a chosen height and asserts the verdict is `Diverged`
/// at exactly that height. A detector that has never detected anything is not
/// evidence; this is its permanently-wired self-test seam.
pub fn replay_persisted_chain_observed<F>(
    storage: &Storage,
    up_to: Option<u64>,
    observe: F,
) -> ReplayVerdict
where
    F: FnMut(u64, &mut State),
{
    replay_persisted_chain_observed_and_checkpoints(storage, up_to, |_, _| {}, observe, |_| {})
}

fn replay_persisted_chain_observed_and_checkpoints<P, F, C>(
    storage: &Storage,
    up_to: Option<u64>,
    mut precheck: P,
    mut observe: F,
    mut checkpoint_observe: C,
) -> ReplayVerdict
where
    P: FnMut(&State, &Block),
    F: FnMut(u64, &mut State),
    C: FnMut(&CheckpointAssertion),
{
    let genesis_state = match storage.load_genesis_state() {
        Ok(Some(state)) => state,
        Ok(None) => {
            return ReplayVerdict::Unverified {
                reason: "no genesis state is persisted — nothing to replay from".to_string(),
            };
        },
        Err(e) => {
            return ReplayVerdict::Unverified {
                reason: format!("genesis state could not be loaded: {e}"),
            };
        },
    };

    let lowest = match storage.load_lowest_block() {
        Ok(Some(block)) => {
            // #3277 S5: replay under the chain's own protocol, read from
            // its block 1 like the node's boot gate: version, table, base
            // feature schedule WITH the block-1-keyed bootstrap, and the
            // schedule-extension authority. Without them every extension
            // fails closed here while the node applied it. First install
            // wins, so a harness that armed the same values first is kept.
            if block.height == 1 {
                if let Err(e) =
                    wrfcoin_shared_protocol::crypto::chain_binding::install_protocol_from_block1(
                        &block,
                    )
                {
                    return ReplayVerdict::Unverified {
                        reason: format!("block 1's protocol schedule could not be resolved: {e}"),
                    };
                }
                wrfcoin_native_chain::upgrade::install_schedule_extension_authority_for_block1(
                    &block,
                );
            }
            block.height
        },
        Ok(None) => {
            return ReplayVerdict::Unverified {
                reason: "zero blocks are persisted — a replay of nothing asserts nothing"
                    .to_string(),
            };
        },
        Err(e) => {
            return ReplayVerdict::Unverified {
                reason: format!("block archive could not be read: {e}"),
            };
        },
    };

    let end = up_to.unwrap_or(u64::MAX);
    let blocks = match storage.load_blocks_range(lowest, end) {
        Ok(blocks) => blocks,
        Err(e) => {
            return ReplayVerdict::Unverified {
                reason: format!("blocks {lowest}..={end} could not be loaded: {e}"),
            };
        },
    };
    if blocks.is_empty() {
        return ReplayVerdict::Unverified {
            reason: format!("no blocks in the requested range {lowest}..={end}"),
        };
    }

    let mut chain = Blockchain::default();
    chain.state = genesis_state;
    // #3332: `feature_active` reads a process-global effective schedule (the
    // base schedule plus the schedule extensions applied in the canonical
    // state). The node refreshes it from its anchor state before replaying
    // the first block and after every applied block (#3277 S3); the replay
    // must do the same, or every block after the first applied extension is
    // re-executed under the base schedule and false-diverges.
    refresh_schedule_from_canonical_state(&chain.state, "replay_verify anchor");

    let first_height = blocks[0].height;
    let mut blocks_replayed = 0u64;
    let mut roots_asserted = 0u64;
    let mut checkpoints_asserted = 0u64;
    let mut expected_height = first_height;
    let mut last_height = first_height;

    for block in blocks {
        let height = block.height;
        if height != expected_height {
            return ReplayVerdict::Unverified {
                reason: format!(
                    "block archive has a gap: expected height {expected_height}, found \
                     {height} — a replay across a gap proves nothing about the missing \
                     blocks"
                ),
            };
        }

        precheck(&chain.state, &block);

        // The follower apply sequence, byte-for-byte the same calls
        // `apply_p2p_block_at` makes (minus p2p-only fork/orphan handling).
        let transitions =
            extract_transitions(&chain.state, &block, height, block.timestamp.timestamp());
        if let Err(e) = chain
            .state
            .apply_transitions_batch_at_height(height, transitions.into_iter())
        {
            return ReplayVerdict::Diverged {
                height,
                committed: block
                    .metadata
                    .get(STATE_ROOT_METADATA_KEY)
                    .cloned()
                    .unwrap_or_else(|| "<no committed root>".to_string()),
                applied: format!("<transitions failed to apply: {e}>"),
                blocks_replayed,
            };
        }
        // Observer runs BEFORE finalization so a test perturbation is folded
        // into THIS block's recomputed root — the RED control must diverge at
        // the perturbed height itself, not one block later.
        observe(height, &mut chain.state);
        chain.state.apply_block_finalization(height);

        // Root comparison done explicitly rather than via
        // `apply_block_finalization_checked_at`: the checked variant skips the
        // comparison below protocol V2, and a replay whose every comparison
        // was version-skipped would read green while asserting nothing —
        // exactly the vacuity this harness exists to forbid. Blocks without a
        // commitment are replayed (they still feed later state) but assert
        // nothing, and a run with zero assertions ends Unverified below.
        if let Some(committed) = block.metadata.get(STATE_ROOT_METADATA_KEY) {
            let applied = chain.state.state_root.to_string();
            if *committed != applied {
                return ReplayVerdict::Diverged {
                    height,
                    committed: committed.clone(),
                    applied,
                    blocks_replayed,
                };
            }
            roots_asserted += 1;
        } else if protocol_version_at(height) >= SigningProtocolVersion::V2 {
            // V2+ finalization requires a commitment even if another height has
            // a matching checkpoint. Signing semantics come from the chain's
            // installed schedule, not the block header's version (#3317).
            return ReplayVerdict::Diverged {
                height,
                committed: format!(
                    "<mandatory {} state-root commitment missing>",
                    protocol_version_at(height)
                ),
                applied: chain.state.state_root.to_string(),
                blocks_replayed,
            };
        }

        // Checkpoint anchor: where the archive stored a full state checkpoint
        // at exactly this height, its root must match the replayed root too.
        if let Ok(Some((checkpoint_height, checkpoint_state))) =
            storage.load_checkpoint_at_or_before(height)
        {
            if checkpoint_height == height {
                let checkpoint_root = checkpoint_state.state_root.to_string();
                let applied = chain.state.state_root.to_string();
                checkpoint_observe(&CheckpointAssertion {
                    height,
                    checkpoint_root: checkpoint_root.clone(),
                    replayed_root: applied.clone(),
                    matched: checkpoint_root == applied,
                });
                if checkpoint_root != applied {
                    return ReplayVerdict::Diverged {
                        height,
                        committed: format!("<checkpoint state root> {checkpoint_root}"),
                        applied,
                        blocks_replayed,
                    };
                }
                checkpoints_asserted += 1;
            }
        }

        if let Err(e) = chain.add_block(block) {
            return ReplayVerdict::Unverified {
                reason: format!("block {height} failed chain-continuity validation: {e}"),
            };
        }
        // #3332: the canonical state changed — the same refresh
        // `apply_p2p_block_at` runs after `add_block`, so the next block is
        // extracted and applied under the schedule this block left in force.
        refresh_schedule_from_canonical_state(&chain.state, "replay_verify applied block");

        blocks_replayed += 1;
        last_height = height;
        expected_height = height + 1;
    }

    if roots_asserted == 0 {
        if checkpoints_asserted > 0 {
            return ReplayVerdict::VerifiedByCheckpoint {
                blocks_replayed,
                checkpoints_asserted,
                first_height,
                last_height,
                final_replayed_root: chain.state.state_root.to_string(),
            };
        }
        return ReplayVerdict::Unverified {
            reason: format!(
                "replayed {blocks_replayed} block(s) but NONE carried a state-root \
                 commitment — zero assertions executed, so this run is not evidence"
            ),
        };
    }

    ReplayVerdict::Verified {
        blocks_replayed,
        roots_asserted,
        checkpoints_asserted,
        first_height,
        last_height,
    }
}
