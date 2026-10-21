//! BFT finality signal plumbing (#1216).
//!
//! This module defines the data type that a BFT finality source (HotStuff
//! commit path, [`BftFinalityGadget`] supermajority, or any future gadget)
//! hands off to the chain layer so that
//! [`Blockchain::set_finalized_height`](crate::blockchain::chain::Blockchain::set_finalized_height)
//! can be called from outside the synchronous hot path.
//!
//! # Why a dedicated type?
//!
//! The BFT finality types in `consensus-engines` (`FinalityEvent`,
//! `BftFinalityGadget`, etc.) live in a crate that **depends on** this one,
//! so they cannot appear in shared-protocol signatures. The chain-layer
//! consumer (e.g. `native-chain/src/bin/launch_api`) must therefore accept
//! a type that is defined here. `FinalitySignal` is intentionally tiny —
//! only the fields that the chain-side guard actually needs — so that any
//! BFT source can emit into the channel regardless of which event enum
//! lives upstream.
//!
//! # The wiring the reorg guard relies on
//!
//! `Blockchain::plan_reorg` already refuses any fork whose fork point is
//! below `self.finalized_height`. That field starts at zero and is only
//! moved forward by `set_finalized_height`. Without this signal type and
//! an async consumer that calls `set_finalized_height` when a signal is
//! received, the guard is inert — every reorg, no matter how deep, would
//! be allowed up to `MAX_REORG_DEPTH`.
//!
//! A runtime that wants the guard to activate should:
//!
//! 1. Create a channel with [`finality_signal_channel`].
//! 2. Pass the `sender` to whichever BFT gadget or consensus engine owns
//!    the "block committed / finalized" decision.
//! 3. Spawn the chain-layer consumer task (see
//!    `native-chain/src/bin/launch_api/p2p_events.rs::run_finality_event_loop`)
//!    with the `receiver` and an `Arc<RwLock<Blockchain>>`.
//!
//! # Stale-finality ordering
//!
//! Finality events may arrive out of order — a slow p2p path can deliver
//! a finality signal for height `N-2` after height `N`. The consumer
//! handles this transparently because `set_finalized_height` already
//! enforces monotonicity: a lower height is silently dropped. The
//! consumer therefore does **not** need its own reordering buffer.
//!
//! Refs #1216, #1160.

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// The capacity of a freshly created finality signal channel.
///
/// A BFT gadget realistically finalizes at most one block per view, so a
/// modest buffer absorbs bursts (e.g. during catch-up) without ever
/// back-pressuring the consensus thread. If the consumer ever falls this
/// far behind, sender-side backpressure is the correct response.
pub const FINALITY_SIGNAL_CHANNEL_CAPACITY: usize = 256;

/// A BFT-finality declaration about a specific block, stamped by whichever
/// gadget produced it.
///
/// This is the single type that crosses the shared-protocol / chain-layer
/// boundary. It carries just enough for the consumer to:
///
/// 1. Call [`Blockchain::set_finalized_height`] with the finalized height.
/// 2. Optionally cross-check the hash against the local chain for
///    divergence diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalitySignal {
    /// Height of the block that just became BFT-final.
    pub height: u64,
    /// Hash of the block that just became BFT-final. The chain-side
    /// consumer MAY compare this against the local block at `height`
    /// to detect that the local chain is on a non-finalized fork and
    /// needs to force-reorg or halt.
    pub block_hash: [u8; 32],
    /// Source tag for observability: e.g. `"bft-gadget"`, `"hotstuff"`,
    /// `"manual"`. Not used for safety checks — purely for log lines.
    pub source: &'static str,
}

impl FinalitySignal {
    /// Construct a new finality signal.
    pub fn new(height: u64, block_hash: [u8; 32], source: &'static str) -> Self {
        Self {
            height,
            block_hash,
            source,
        }
    }
}

/// Sender half of the BFT finality channel.
///
/// Handed to consensus sources (HotStuff commit path,
/// [`BftFinalityGadget`], etc.). Cloneable so multiple sources can share
/// the same channel — the consumer merges them automatically via
/// monotonic height clamping.
pub type FinalitySignalSender = mpsc::Sender<FinalitySignal>;

/// Receiver half of the BFT finality channel.
///
/// Owned by the consumer task that threads signals into
/// [`Blockchain::set_finalized_height`].
pub type FinalitySignalReceiver = mpsc::Receiver<FinalitySignal>;

/// Create a new BFT finality signal channel with [`FINALITY_SIGNAL_CHANNEL_CAPACITY`].
///
/// Callers typically split the pair: the sender goes to one or more
/// BFT sources, and the receiver goes to a single consumer task.
pub fn finality_signal_channel() -> (FinalitySignalSender, FinalitySignalReceiver) {
    mpsc::channel(FINALITY_SIGNAL_CHANNEL_CAPACITY)
}

/// Non-blocking helper for consensus sources to emit a finality signal.
///
/// BFT gadgets run inside synchronous code paths (e.g. inside a
/// `submit_vote` call holding a lock). Using `try_send` lets the gadget
/// hand off the signal without awaiting, which keeps the consensus hot
/// path synchronous and never blocks it on a slow consumer.
///
/// If the channel is full the signal is **dropped** and a warning is
/// logged. Dropping a signal is safe with respect to the reorg guard:
/// the next finalized height will be strictly greater, and
/// `set_finalized_height` is idempotent under "catch up to the newest
/// value observed" semantics. Operators see the drop in logs and can
/// scale the consumer if needed.
pub fn emit_finality_signal(sender: &FinalitySignalSender, signal: FinalitySignal) {
    match sender.try_send(signal.clone()) {
        Ok(()) => {
            log::debug!(
                "BFT finality signal emitted: height={} source={}",
                signal.height,
                signal.source
            );
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            log::warn!(
                "BFT finality signal channel full — dropping signal for height {} \
                 (source={}). The consumer is falling behind; the next finalized \
                 height will still advance the guard monotonically.",
                signal.height,
                signal.source
            );
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            log::debug!(
                "BFT finality signal channel closed — dropping signal for height {} \
                 (source={}). This is expected during shutdown.",
                signal.height,
                signal.source
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finality_signal_new_constructs_expected_fields() {
        let hash = [7u8; 32];
        let sig = FinalitySignal::new(42, hash, "unit-test");
        assert_eq!(sig.height, 42);
        assert_eq!(sig.block_hash, hash);
        assert_eq!(sig.source, "unit-test");
    }

    #[tokio::test]
    async fn emit_finality_signal_delivers_through_channel() {
        let (tx, mut rx) = finality_signal_channel();
        let hash = [9u8; 32];
        let sig = FinalitySignal::new(100, hash, "unit-test");

        emit_finality_signal(&tx, sig.clone());

        let received = rx.recv().await.expect("channel must deliver the signal");
        assert_eq!(received, sig);
    }

    #[tokio::test]
    async fn emit_finality_signal_does_not_panic_when_channel_full() {
        // Build a tiny channel and saturate it, then verify that
        // emit_finality_signal drops gracefully.
        let (tx, _rx) = mpsc::channel::<FinalitySignal>(1);
        let sig1 = FinalitySignal::new(1, [1u8; 32], "unit-test");
        let sig2 = FinalitySignal::new(2, [2u8; 32], "unit-test");

        // Fill the channel.
        tx.try_send(sig1).expect("first try_send must succeed on empty channel");

        // Second emit must not panic even though the channel is full.
        emit_finality_signal(&tx, sig2);
        // If we reach this point, the drop path worked.
    }

    #[tokio::test]
    async fn emit_finality_signal_handles_closed_channel() {
        let (tx, rx) = finality_signal_channel();
        drop(rx); // close the channel

        // Must not panic.
        emit_finality_signal(&tx, FinalitySignal::new(5, [5u8; 32], "unit-test"));
    }

    #[test]
    fn finality_signal_channel_capacity_is_nonzero() {
        // Guard against someone accidentally setting it to zero, which
        // would break `try_send` in the common case. Read the constant
        // through a `let` so clippy does not fold the `assert!` away.
        let capacity = FINALITY_SIGNAL_CHANNEL_CAPACITY;
        assert_ne!(capacity, 0, "channel capacity must be positive");
    }
}
