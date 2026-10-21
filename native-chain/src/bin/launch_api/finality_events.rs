//! BFT finality signal consumer (#1216).
//!
//! This module owns the async task that closes the wiring gap in the
//! reorg rejection path. [`Blockchain::plan_reorg`] already refuses any
//! fork whose fork point sits at or below the blockchain's
//! [`finalized_height`](wrfcoin_shared_protocol::blockchain::chain::Blockchain::finalized_height);
//! the rejection check lives at `shared-protocol/src/blockchain/chain.rs:382`.
//! The check is inert because nothing outside the test suite ever called
//! [`set_finalized_height`](wrfcoin_shared_protocol::blockchain::chain::Blockchain::set_finalized_height).
//!
//! [`run_finality_event_loop`] is that missing caller. It listens on a
//! [`FinalitySignalReceiver`](wrfcoin_shared_protocol::consensus::finality::FinalitySignalReceiver),
//! receives signals emitted by BFT sources (the HotStuff commit path or
//! `BftFinalityGadget::submit_vote`), and forwards the finalized height
//! into the chain's monotonic guard.
//!
//! # Not p2p-gated
//!
//! The consumer is deliberately **not** under `#[cfg(feature = "p2p")]`.
//! BFT finality is a local-chain property — it does not require the p2p
//! feature — so a single-node dev deployment can still exercise the
//! guard without recompiling.
//!
//! # Monotonicity / stale-finality safety
//!
//! [`Blockchain::set_finalized_height`] is monotonic: a lower height is
//! silently ignored. This consumer therefore does not need its own
//! reordering buffer — out-of-order or duplicated signals are harmless,
//! and a dropped signal simply means the next signal must catch up at a
//! strictly higher height (BFT sources only emit on new finalization).
//!
//! Refs #1216, #1160.

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::consensus::finality::FinalitySignalReceiver;
use wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode;
use wrfcoin_shared_protocol::invariants::finality::{FinalityViolation, SharedFinalityInvariant};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

/// Why [`run_finality_event_loop`] stopped.
///
/// The consumer used to return `()`, which meant a caller could — and the
/// production spawn site did — discard the outcome and let the node keep
/// producing blocks after a consensus-safety breach. A safety violation must
/// be impossible to drop on the floor, so it is now in the return type.
#[derive(Debug)]
#[must_use = "a finality consumer exit may be a safety violation that must halt the node"]
pub enum ConsumerExit {
    /// Every sender was dropped — normal shutdown.
    ChannelClosed,
    /// INV-FINALITY-1 was breached. The node MUST NOT continue participating.
    SafetyViolation {
        violation: FinalityViolation,
        source: &'static str,
    },
}

impl ConsumerExit {
    /// Whether this exit means the node must stop.
    pub fn is_fatal(&self) -> bool {
        matches!(self, ConsumerExit::SafetyViolation { .. })
    }
}

/// Take the node out of service after a consensus-safety breach.
///
/// Forces `ReadOnly` partition mode on both the shared flag and the mempool —
/// the same pair `POST /operator/partition-mode` sets — so the node stops
/// producing blocks and stops accepting transactions while remaining
/// queryable for the incident investigation.
///
/// Split out from [`supervise_finality_consumer`]'s process termination so the
/// halt is testable: a test can assert the node was actually taken out of
/// service without the test binary exiting.
///
/// `violation` is a rendered `&str` rather than [`FinalityViolation`] because
/// this is now the halt used by *every* stateful invariant (#2097):
/// INV-SUPPLY-1 reaches it through
/// [`stateful_invariants`](crate::stateful_invariants). Giving the supply
/// invariant its own halt would be a second, weaker enforcement mechanism —
/// exactly what registering into one registry exists to avoid.
pub async fn quiesce_after_safety_violation(
    partition_mode: &Arc<RwLock<PartitionMode>>,
    mempool: &Arc<RwLock<Mempool>>,
    violation: &str,
    source: &str,
) {
    {
        let mut mode = partition_mode.write().await;
        *mode = PartitionMode::ReadOnly;
    }
    {
        let mut pool = mempool.write().await;
        pool.set_partition_mode(PartitionMode::ReadOnly);
    }

    log::error!(
        "FATAL: {violation} (source={source}) — node forced to {} and will terminate. \
         This node observed a consensus- or supply-safety violation; it must not \
         continue participating. See docs/security/EMERGENCY_RUNBOOK.md §1 Emergency halt.",
        PartitionMode::ReadOnly.label(),
    );
}

/// Await the finality consumer and halt the node if it stopped unsafely.
///
/// Returns the exit reason so the caller performs the actual process
/// termination — keeping `std::process::exit` at the single call site in
/// `main.rs` where it can be reviewed, rather than buried in a library
/// function that tests cannot call.
///
/// A panicking consumer is treated as fatal too: the consumer is the only
/// thing that evaluates INV-FINALITY-1 on the live signal path, so a node
/// whose consumer died is a node with no safety check running.
pub async fn supervise_finality_consumer(
    handle: JoinHandle<ConsumerExit>,
    partition_mode: Arc<RwLock<PartitionMode>>,
    mempool: Arc<RwLock<Mempool>>,
) -> ConsumerExit {
    match handle.await {
        Ok(ConsumerExit::ChannelClosed) => ConsumerExit::ChannelClosed,
        Ok(ConsumerExit::SafetyViolation { violation, source }) => {
            quiesce_after_safety_violation(
                &partition_mode,
                &mempool,
                &violation.to_string(),
                source,
            )
            .await;
            ConsumerExit::SafetyViolation { violation, source }
        },
        Err(join_error) => {
            let violation = FinalityViolation::ConflictingFinalization {
                height: 0,
                seen: [0u8; 32],
                incoming: [0u8; 32],
            };
            log::error!(
                "FATAL: BFT finality consumer task died ({join_error}) — no invariant is \
                 evaluating INV-FINALITY-1 on the live signal path any more."
            );
            quiesce_after_safety_violation(
                &partition_mode,
                &mempool,
                &violation.to_string(),
                "consumer-task-died",
            )
            .await;
            ConsumerExit::SafetyViolation {
                violation,
                source: "consumer-task-died",
            }
        },
    }
}

/// Consume [`FinalitySignal`]s from the channel and forward each into
/// [`Blockchain::set_finalized_height`].
///
/// This task runs for the lifetime of the process. When every sender
/// half of the channel is dropped (e.g. during graceful shutdown),
/// `recv` returns `None`, the loop logs and exits, and the spawned
/// task joins cleanly.
///
/// When `storage` is supplied, every monotonic advance of
/// `finalized_height` is also persisted to the database (issue #1275).
/// Without that persistence step, a node restart would silently reset
/// `finalized_height` to 0 and permit reorgs of already-finalized
/// blocks. The argument is optional so unit tests can exercise the
/// in-memory guard without a real redb database.
pub async fn run_finality_event_loop(
    mut signal_rx: FinalitySignalReceiver,
    blockchain: Arc<RwLock<Blockchain>>,
    storage: Option<Arc<RwLock<Storage>>>,
    finality_invariant: SharedFinalityInvariant,
) -> ConsumerExit {
    log::info!(
        "BFT finality signal consumer started — any finalized-height updates will \
         now propagate into Blockchain::plan_reorg's rejection guard (#1216)"
    );

    // INV-FINALITY-1 (#2096). The per-signal hash check below compares against
    // whatever block is stored locally *right now*, which a reorg can change.
    // This remembers every finalization the process has ever accepted, so a
    // second, different finalization at an already-finalized height is caught
    // even when local state has since moved.
    //
    // Passed in rather than constructed here so the caller can register the
    // same instance in a `StatefulInvariantRegistry` and poll it — the
    // extension surface future stateful invariants implement.

    while let Some(signal) = signal_rx.recv().await {
        let mut chain = blockchain.write().await;

        // Defence-in-depth: cross-check signal.block_hash against the
        // locally-stored canonical block before advancing finalized_height.
        //
        // Production signal emitters (bft_finality.rs::submit_vote and the
        // cert-gossip gate in p2p_events.rs) already validate the hash before
        // creating a FinalitySignal, so this guard fires only on:
        //   • future emission paths that forget the chain-level check, or
        //   • phantom-finality: a signal whose height exceeds the local tip
        //     would permanently arm plan_reorg's rejection guard for a block
        //     that does not exist in the canonical chain.
        //
        // We hold the write lock throughout so the chain cannot be concurrently
        // extended between the check and the set_finalized_height call.
        let hash_ok = match chain.block_at_height(signal.height) {
            None => {
                log::warn!(
                    "BFT finality signal rejected: no local block at height {} \
                     (source={} chain_tip={}) — finalized_height not advanced",
                    signal.height,
                    signal.source,
                    chain.height,
                );
                false
            },
            Some(block) => {
                let local_hash: [u8; 32] =
                    block.hash.unwrap_or_else(|| block.calculate_hash()).into();
                if local_hash != signal.block_hash {
                    log::warn!(
                        "BFT finality signal rejected: block_hash mismatch at height {} \
                         (signal={} local={} source={}) — finalized_height not advanced",
                        signal.height,
                        hex::encode(signal.block_hash),
                        hex::encode(local_hash),
                        signal.source,
                    );
                    false
                } else {
                    true
                }
            },
        };
        if !hash_ok {
            continue;
        }

        // INV-FINALITY-1 (#2096).
        //
        // Only a *conflicting* finalization is fatal. A below-frontier height
        // is an ordering anomaly that this channel is documented to produce
        // (see "Stale-finality ordering" in `consensus::finality`) and that
        // `set_finalized_height` already clamps — halting on it would take
        // down healthy nodes during normal catch-up.
        //
        // A conflict, by contrast, means the chain has been finalized two
        // different ways at one height. That is unrecoverable, so we halt:
        // `finalized_height` freezes at the last value known to be safe and
        // `plan_reorg` keeps rejecting reorgs below it, rather than ratcheting
        // the guard forward on a contradiction.
        //
        // The halt is externally visible without adding a metric — the
        // existing `wrfcoin_chain_bft_last_advance_timestamp_seconds`
        // absence-of-advance alert fires once finality stops advancing.
        // Operator response is the emergency-halt procedure in
        // `docs/security/EMERGENCY_RUNBOOK.md` (#2101).
        if let Err(violation) =
            finality_invariant.observe_finalized(signal.height, signal.block_hash)
        {
            if violation.is_safety_violation() {
                log::error!(
                    "FATAL: {violation} (source={}) — halting the BFT finality consumer. \
                     finalized_height stays at {}; this node will not advance finality again \
                     without operator intervention. See docs/security/EMERGENCY_RUNBOOK.md.",
                    signal.source,
                    chain.finalized_height(),
                );
                return ConsumerExit::SafetyViolation {
                    violation,
                    source: signal.source,
                };
            }
            log::debug!(
                "{violation} (source={}) — tolerated: out-of-order finality delivery is \
                 expected and set_finalized_height clamps it.",
                signal.source,
            );
        }

        let previous = chain.finalized_height();
        chain.set_finalized_height(signal.height);
        let new = chain.finalized_height();
        drop(chain);

        if new > previous {
            // #1388: latch the BFT-active gauge on the first successful
            // advance. Operators use `wrfcoin_chain_bft_finality_active == 0`
            // to detect nodes where BFT has never fired — without this flip,
            // `wrfcoin_chain_finalized_height` is indistinguishable from the
            // probabilistic `tip - MAX_REORG_DEPTH` fallback.
            CORE_METRICS.mark_bft_finality_active();
            // #1399: update the BFT-specific gauges so operators can distinguish
            // "BFT gadget healthy" from "BFT gadget silent but probabilistic
            // floor advancing." `bft_finalized_height` is the raw BFT value
            // (never clamped); `bft_last_advance_timestamp_seconds` enables the
            // absence-of-advance alert even when the height gauge has a value.
            CORE_METRICS.record_bft_advance(new);

            log::info!(
                "BFT finality advanced: height={} previous={} source={} hash={}",
                new,
                previous,
                signal.source,
                hex::encode(signal.block_hash)
            );

            // Persist the new finalized height so it survives restarts
            // (issue #1275). Persistence failures are logged but do not
            // abort the consumer — the in-memory guard is already armed,
            // and the next successful persist (via persist_block or the
            // next finality signal) will catch up.
            if let Some(ref storage) = storage {
                let st = storage.read().await;
                if let Err(e) = st.persist_finalized_height(new).await {
                    log::error!(
                        "Failed to persist finalized_height={new}: {e} \
                         — in-memory guard is armed, but restart may regress"
                    );
                }
            }
        } else {
            // Monotonic clamp fired — this is expected when signals arrive
            // out-of-order or a duplicate is retried. Log at debug so
            // operators see the pattern without noise in steady state.
            log::debug!(
                "BFT finality signal ignored (non-advancing): incoming_height={} \
                 current_finalized={} source={}",
                signal.height,
                new,
                signal.source
            );
        }
    }

    log::warn!("BFT finality signal channel closed, consumer shutting down");
    ConsumerExit::ChannelClosed
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_native_chain::blockchain::block::Block;
    use wrfcoin_shared_protocol::config::settings::Config;
    use wrfcoin_shared_protocol::consensus::finality::{FinalitySignal, finality_signal_channel};

    /// A partition-mode + mempool pair in the same shape `AppState` holds them.
    fn service_handles() -> (Arc<RwLock<PartitionMode>>, Arc<RwLock<Mempool>>) {
        (
            Arc::new(RwLock::new(PartitionMode::Normal)),
            Arc::new(RwLock::new(Mempool::new(&Config::default()))),
        )
    }

    async fn assert_out_of_service(
        partition_mode: &Arc<RwLock<PartitionMode>>,
        mempool: &Arc<RwLock<Mempool>>,
    ) {
        assert_eq!(
            *partition_mode.read().await,
            PartitionMode::ReadOnly,
            "the node must be forced ReadOnly"
        );
        let pool = mempool.read().await;
        assert!(
            !pool.partition_mode().accepts_mempool_writes(),
            "the mempool must stop accepting transactions"
        );
        assert!(
            !pool.partition_mode().allows_block_production(),
            "block production must be halted"
        );
    }

    /// Helper: extend a fresh chain by `count` trivial blocks.
    fn extend_chain(chain: &mut Blockchain, count: u64) {
        for _ in 0..count {
            let height = chain.height + 1;
            let mut block = Block::new(1, chain.tip, Vec::new(), 1, height);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            chain
                .add_block(block)
                .expect("extend_chain: add_block must succeed");
        }
    }

    /// Helper: return the stored hash of the block at `height` as `[u8; 32]`.
    ///
    /// Panics if the block doesn't exist or has no hash — both are test
    /// precondition violations.
    fn block_hash_at(chain: &Blockchain, height: u64) -> [u8; 32] {
        chain
            .block_at_height(height)
            .unwrap_or_else(|| panic!("no block at height {height}"))
            .hash
            .unwrap_or_else(|| panic!("block at height {height} has no hash"))
            .into()
    }

    /// The consumer must advance `finalized_height` when it receives a
    /// signal — this is the single line of wiring the whole issue #1216
    /// turns on.
    #[tokio::test]
    async fn consumer_advances_finalized_height_on_signal() {
        let mut chain = Blockchain::default();
        extend_chain(&mut chain, 20);
        // Capture the canonical hash before wrapping so the signal is valid.
        let hash_at_10 = block_hash_at(&chain, 10);
        let chain = Arc::new(RwLock::new(chain));

        assert_eq!(
            chain.read().await.finalized_height(),
            0,
            "precondition: guard must start inert"
        );

        let (tx, rx) = finality_signal_channel();
        let chain_clone = chain.clone();
        let handle = tokio::spawn(run_finality_event_loop(
            rx,
            chain_clone,
            None,
            SharedFinalityInvariant::new(),
        ));

        tx.send(FinalitySignal::new(10, hash_at_10, "test"))
            .await
            .expect("send must succeed");

        // Close the channel so the loop exits.
        drop(tx);
        let exit = handle.await.expect("consumer must shut down cleanly");
        assert!(
            !exit.is_fatal(),
            "clean shutdown must not be fatal: {exit:?}"
        );

        assert_eq!(
            chain.read().await.finalized_height(),
            10,
            "consumer must have advanced the guard"
        );
    }

    /// Stale / out-of-order signals must be clamped and must not roll the
    /// guard backwards. This is the safety property the briefing asked us
    /// to be explicit about.
    #[tokio::test]
    async fn consumer_ignores_stale_signals() {
        let mut chain = Blockchain::default();
        extend_chain(&mut chain, 20);
        // Capture canonical hashes so signals pass the block_hash guard.
        let hash_at_15 = block_hash_at(&chain, 15);
        let hash_at_7 = block_hash_at(&chain, 7);
        let chain = Arc::new(RwLock::new(chain));

        let (tx, rx) = finality_signal_channel();
        let chain_clone = chain.clone();
        let handle = tokio::spawn(run_finality_event_loop(
            rx,
            chain_clone,
            None,
            SharedFinalityInvariant::new(),
        ));

        tx.send(FinalitySignal::new(15, hash_at_15, "test"))
            .await
            .unwrap();
        tx.send(FinalitySignal::new(7, hash_at_7, "test-stale"))
            .await
            .unwrap();
        tx.send(FinalitySignal::new(15, hash_at_15, "test-duplicate"))
            .await
            .unwrap();

        drop(tx);
        assert!(!handle.await.unwrap().is_fatal());

        assert_eq!(
            chain.read().await.finalized_height(),
            15,
            "stale and duplicate signals must not decrease the finalized height"
        );
    }

    /// Once `finalized_height` is set, `plan_reorg` refuses reorgs that
    /// would revert finalized blocks. This end-to-end test is the whole
    /// point of #1216: the consumer arms the guard, the guard fires.
    #[tokio::test]
    async fn consumer_armed_guard_rejects_deep_reorg() {
        let mut chain = Blockchain::default();
        extend_chain(&mut chain, 30);
        // Capture canonical hash before wrapping so the signal is valid.
        let hash_at_20 = block_hash_at(&chain, 20);
        let chain = Arc::new(RwLock::new(chain));

        let (tx, rx) = finality_signal_channel();
        let chain_clone = chain.clone();
        let handle = tokio::spawn(run_finality_event_loop(
            rx,
            chain_clone,
            None,
            SharedFinalityInvariant::new(),
        ));

        // Advance finality to height 20.
        tx.send(FinalitySignal::new(20, hash_at_20, "test"))
            .await
            .unwrap();
        drop(tx);
        assert!(!handle.await.unwrap().is_fatal());

        // Build a candidate fork whose fork point is at height 10
        // (below the finalized height of 20) but whose tip exceeds the
        // local chain. Normally `plan_reorg` would accept this (depth
        // 20 is within MAX_REORG_DEPTH=100). With the guard armed, it
        // must refuse.
        let fork_parent = {
            let c = chain.read().await;
            c.block_at_height(10).expect("height 10 must exist").clone()
        };
        let fork_parent_hash = fork_parent
            .hash
            .unwrap_or_else(|| fork_parent.calculate_hash());

        let mut candidate = Vec::new();
        let mut prev_hash = fork_parent_hash;
        for h in 11..=35 {
            // Nudge the version so the candidate hashes differ from the
            // local chain (otherwise the fork would be identical and
            // `plan_reorg` would treat it as a no-op before the guard
            // check).
            let mut block = Block::new(2, prev_hash, Vec::new(), 1, h);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            prev_hash = hash;
            candidate.push(block);
        }

        let chain_reader = chain.read().await;
        let plan = chain_reader
            .plan_reorg(&candidate)
            .expect("plan_reorg must not error");

        assert!(
            plan.is_none(),
            "with finalized_height=20, a fork at height 10 must be refused \
             — this is the BFT finality guard activating end-to-end (#1216)"
        );
    }

    /// #1388: once the consumer processes a monotonic advance, the
    /// `wrfcoin_chain_bft_finality_active` gauge must latch to 1. This is the
    /// signal the alert rule uses to detect "BFT has never fired on this
    /// node" — without it, `wrfcoin_chain_finalized_height` is
    /// indistinguishable from the probabilistic fallback.
    ///
    /// `CORE_METRICS` is a process-wide `LazyLock`, so this test only asserts
    /// the gauge has advanced to 1 after consumption (it may already be 1
    /// from a prior test run within the same process).
    #[tokio::test]
    async fn consumer_advance_marks_bft_finality_active_gauge() {
        let mut chain = Blockchain::default();
        extend_chain(&mut chain, 20);
        // Capture canonical hash before wrapping so the signal passes the guard.
        let hash_at_12 = block_hash_at(&chain, 12);
        let chain = Arc::new(RwLock::new(chain));

        let (tx, rx) = finality_signal_channel();
        let chain_clone = chain.clone();
        let handle = tokio::spawn(run_finality_event_loop(
            rx,
            chain_clone,
            None,
            SharedFinalityInvariant::new(),
        ));

        tx.send(FinalitySignal::new(12, hash_at_12, "test-1388"))
            .await
            .unwrap();
        drop(tx);
        assert!(!handle.await.unwrap().is_fatal());

        assert_eq!(
            chain.read().await.finalized_height(),
            12,
            "sanity: advance must have landed"
        );
        assert_eq!(
            CORE_METRICS.bft_finality_active.get(),
            1,
            "advance must latch wrfcoin_chain_bft_finality_active to 1 (#1388)"
        );
    }

    /// #1388 / #1424: a `bft-cert-gossip` signal — the source label the
    /// p2p ingestion path uses when it adopts a peer's complete
    /// FinalityCertificate — must arm the reorg guard exactly the same
    /// way a locally-produced `bft-gadget` signal does. This proves that
    /// a freshly-booted node which only ever sees gossiped certificates
    /// (never its own votes) still gets a usefully-armed reorg guard.
    #[tokio::test]
    async fn cert_gossip_signal_advances_finality_just_like_local_vote() {
        let mut chain = Blockchain::default();
        extend_chain(&mut chain, 25);
        // Capture canonical hash before wrapping so the signal passes the guard.
        let hash_at_18 = block_hash_at(&chain, 18);
        let chain = Arc::new(RwLock::new(chain));

        let (tx, rx) = finality_signal_channel();
        let chain_clone = chain.clone();
        let handle = tokio::spawn(run_finality_event_loop(
            rx,
            chain_clone,
            None,
            SharedFinalityInvariant::new(),
        ));

        // The signal source matches what `p2p_events.rs` emits when an
        // inbound FinalityCertificate is decoded.
        tx.send(FinalitySignal::new(18, hash_at_18, "bft-cert-gossip"))
            .await
            .expect("send must succeed");
        drop(tx);
        assert!(!handle.await.unwrap().is_fatal());

        assert_eq!(
            chain.read().await.finalized_height(),
            18,
            "cert-gossip signals from peers must advance finality identically to local votes"
        );
    }

    /// Defence-in-depth guard: a signal whose `block_hash` does not match
    /// the locally-stored canonical block, or whose height exceeds the chain
    /// tip, must be silently dropped — `finalized_height` must not advance.
    ///
    /// This is the new consumer-side check added alongside the existing
    /// emitter-side validations (validate_p2p_finality_certificate_against_chain
    /// in p2p_events.rs, submit_vote in bft_finality.rs).
    #[tokio::test]
    async fn consumer_rejects_signal_with_wrong_hash_or_phantom_height() {
        let mut chain = Blockchain::default();
        extend_chain(&mut chain, 10);
        let chain = Arc::new(RwLock::new(chain));

        let (tx, rx) = finality_signal_channel();
        let chain_clone = chain.clone();
        let handle = tokio::spawn(run_finality_event_loop(
            rx,
            chain_clone,
            None,
            SharedFinalityInvariant::new(),
        ));

        // Wrong hash for an existing height — must be rejected.
        tx.send(FinalitySignal::new(5, [0xde; 32], "bad-hash"))
            .await
            .expect("send must succeed");
        // Height beyond the chain tip — must be rejected (phantom finality).
        tx.send(FinalitySignal::new(99, [0u8; 32], "phantom-height"))
            .await
            .expect("send must succeed");

        drop(tx);
        let exit = handle.await.expect("consumer must shut down cleanly");
        assert!(
            !exit.is_fatal(),
            "clean shutdown must not be fatal: {exit:?}"
        );

        assert_eq!(
            chain.read().await.finalized_height(),
            0,
            "wrong-hash and phantom-height signals must not advance finalized_height"
        );
    }

    /// Regression guard: dropping the sender must terminate the consumer
    /// cleanly so main.rs shutdown does not deadlock on task join.
    #[tokio::test]
    async fn consumer_exits_cleanly_when_sender_dropped() {
        let chain = Arc::new(RwLock::new(Blockchain::default()));
        let (tx, rx) = finality_signal_channel();
        let handle = tokio::spawn(run_finality_event_loop(
            rx,
            chain,
            None,
            SharedFinalityInvariant::new(),
        ));

        drop(tx);

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
        assert!(
            result.is_ok(),
            "consumer must terminate within 5s of sender drop"
        );
        let exit = result.unwrap().expect("consumer must not panic");
        assert!(!exit.is_fatal(), "sender drop is a clean exit: {exit:?}");
    }

    /// INV-FINALITY-1 (#2096): two different hashes finalized at one height
    /// must halt the consumer rather than ratchet the guard forward.
    ///
    /// The reorg between the two signals is what makes this reachable: the
    /// per-signal hash check compares against *current* local state, so once
    /// the chain has been rewritten it happily accepts the second, conflicting
    /// finalization. Only the invariant's memory catches it.
    #[tokio::test]
    async fn consumer_halts_on_conflicting_finalization_at_one_height() {
        let mut chain_a = Blockchain::default();
        extend_chain(&mut chain_a, 20);
        let hash_at_10 = block_hash_at(&chain_a, 10);
        let hash_at_12 = block_hash_at(&chain_a, 12);
        let chain = Arc::new(RwLock::new(chain_a));

        let (tx, rx) = finality_signal_channel();
        let handle = tokio::spawn(run_finality_event_loop(
            rx,
            chain.clone(),
            None,
            SharedFinalityInvariant::new(),
        ));

        // Finalize height 10 on the original chain.
        tx.send(FinalitySignal::new(10, hash_at_10, "test"))
            .await
            .expect("first signal must send");
        // Also advance to 12 so we can prove the guard stops moving after the halt.
        tx.send(FinalitySignal::new(12, hash_at_12, "test"))
            .await
            .expect("second signal must send");

        // Wait for both to be consumed before rewriting history.
        for _ in 0..100 {
            if chain.read().await.finalized_height() == 12 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(chain.read().await.finalized_height(), 12);

        // Rebuild a *different* chain so height 10 now holds a different block,
        // and that new block passes the consumer's local-hash check.
        let mut chain_b = Blockchain::default();
        extend_chain(&mut chain_b, 3);
        extend_chain(&mut chain_b, 17);
        let conflicting_hash_at_10 = block_hash_at(&chain_b, 10);
        assert_ne!(
            conflicting_hash_at_10, hash_at_10,
            "fixture precondition: the two chains must differ at height 10"
        );
        {
            let mut guard = chain.write().await;
            let finalized = guard.finalized_height();
            *guard = chain_b;
            guard.set_finalized_height(finalized);
        }

        tx.send(FinalitySignal::new(
            10,
            conflicting_hash_at_10,
            "test-conflict",
        ))
        .await
        .expect("conflicting signal must send");

        // The consumer must return on its own, without the sender being dropped.
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
        assert!(
            result.is_ok(),
            "consumer must halt within 5s of a conflicting finalization, \
             while the channel is still open"
        );
        let exit = result
            .unwrap()
            .expect("halt must be a clean return, not a panic");
        assert!(
            exit.is_fatal(),
            "a conflicting finalization must return a fatal exit, got {exit:?}"
        );
        assert!(
            matches!(exit, ConsumerExit::SafetyViolation { .. }),
            "got {exit:?}"
        );

        assert_eq!(
            chain.read().await.finalized_height(),
            12,
            "finalized_height must freeze at the last safe value"
        );
    }

    /// The complement: a stale lower-height signal with a *different* hash is
    /// normal catch-up, not a conflict, and must not halt the consumer.
    #[tokio::test]
    async fn consumer_tolerates_new_below_frontier_height() {
        let mut chain = Blockchain::default();
        extend_chain(&mut chain, 20);
        let hash_at_15 = block_hash_at(&chain, 15);
        let hash_at_7 = block_hash_at(&chain, 7);
        let chain = Arc::new(RwLock::new(chain));

        let (tx, rx) = finality_signal_channel();
        let handle = tokio::spawn(run_finality_event_loop(
            rx,
            chain.clone(),
            None,
            SharedFinalityInvariant::new(),
        ));

        tx.send(FinalitySignal::new(15, hash_at_15, "test"))
            .await
            .unwrap();
        // Height 7 was never finalized before and is below the frontier — an
        // ordering anomaly, and a genuinely different hash from height 15.
        tx.send(FinalitySignal::new(7, hash_at_7, "test-stale"))
            .await
            .unwrap();
        tx.send(FinalitySignal::new(15, hash_at_15, "test-dup"))
            .await
            .unwrap();

        drop(tx);
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
        assert!(
            result.is_ok(),
            "consumer must not halt on an ordering anomaly"
        );
        let exit = result.unwrap().expect("consumer must not panic");
        assert!(
            !exit.is_fatal(),
            "ordering anomalies are not fatal: {exit:?}"
        );

        assert_eq!(
            chain.read().await.finalized_height(),
            15,
            "the guard must still hold the highest finalized height"
        );
    }

    /// The blocker from the #2246 review, end to end.
    ///
    /// Previously the production spawn site did `tokio::spawn(run_..)` and
    /// dropped the `JoinHandle`, so a detected conflicting finalization killed
    /// one background task and the node carried on producing blocks. This
    /// drives the REAL consumer with a REAL constructed conflict, hands its
    /// handle to the supervisor exactly as `main.rs` does, and asserts the
    /// node is taken out of service.
    #[tokio::test]
    async fn supervisor_takes_the_node_out_of_service_on_conflicting_finalization() {
        let mut chain_a = Blockchain::default();
        extend_chain(&mut chain_a, 20);
        let hash_at_10 = block_hash_at(&chain_a, 10);
        let chain = Arc::new(RwLock::new(chain_a));
        let (partition_mode, mempool) = service_handles();

        let (tx, rx) = finality_signal_channel();
        let consumer = tokio::spawn(run_finality_event_loop(
            rx,
            chain.clone(),
            None,
            SharedFinalityInvariant::new(),
        ));

        tx.send(FinalitySignal::new(10, hash_at_10, "test"))
            .await
            .expect("first signal");
        for _ in 0..100 {
            if chain.read().await.finalized_height() == 10 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        // Rewrite history so height 10 now holds a different block that still
        // passes the consumer's local-hash check.
        let mut chain_b = Blockchain::default();
        extend_chain(&mut chain_b, 3);
        extend_chain(&mut chain_b, 17);
        let conflicting = block_hash_at(&chain_b, 10);
        assert_ne!(conflicting, hash_at_10, "fixture must actually conflict");
        {
            let mut guard = chain.write().await;
            let finalized = guard.finalized_height();
            *guard = chain_b;
            guard.set_finalized_height(finalized);
        }

        tx.send(FinalitySignal::new(10, conflicting, "test-conflict"))
            .await
            .expect("conflicting signal");

        // Precondition: still in service before the supervisor reacts.
        assert_eq!(*partition_mode.read().await, PartitionMode::Normal);

        let exit = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            supervise_finality_consumer(consumer, partition_mode.clone(), mempool.clone()),
        )
        .await
        .expect("the supervisor must act within 5s, with the channel still open");

        assert!(exit.is_fatal(), "got {exit:?}");
        assert!(
            matches!(exit, ConsumerExit::SafetyViolation { .. }),
            "got {exit:?}"
        );
        assert_out_of_service(&partition_mode, &mempool).await;
    }

    /// The complement: a clean shutdown must NOT take the node out of service.
    /// Without this, "always halt" would pass the test above and be useless.
    #[tokio::test]
    async fn supervisor_leaves_the_node_in_service_on_clean_shutdown() {
        let mut chain = Blockchain::default();
        extend_chain(&mut chain, 5);
        let chain = Arc::new(RwLock::new(chain));
        let (partition_mode, mempool) = service_handles();

        let (tx, rx) = finality_signal_channel();
        let consumer = tokio::spawn(run_finality_event_loop(
            rx,
            chain,
            None,
            SharedFinalityInvariant::new(),
        ));
        drop(tx);

        let exit = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            supervise_finality_consumer(consumer, partition_mode.clone(), mempool.clone()),
        )
        .await
        .expect("supervisor must return after the channel closes");

        assert!(!exit.is_fatal(), "got {exit:?}");
        assert_eq!(
            *partition_mode.read().await,
            PartitionMode::Normal,
            "a clean shutdown must not quiesce the node"
        );
    }

    /// A consumer that dies leaves nothing evaluating INV-FINALITY-1 on the
    /// live path, so it is fatal too — otherwise killing the task would be a
    /// way to disable the safety check silently.
    #[tokio::test]
    async fn supervisor_treats_a_dead_consumer_as_fatal() {
        let (partition_mode, mempool) = service_handles();
        let consumer: JoinHandle<ConsumerExit> = tokio::spawn(async {
            panic!("consumer died");
        });

        let exit =
            supervise_finality_consumer(consumer, partition_mode.clone(), mempool.clone()).await;

        assert!(exit.is_fatal(), "a dead consumer must be fatal: {exit:?}");
        assert_out_of_service(&partition_mode, &mempool).await;
    }
}
