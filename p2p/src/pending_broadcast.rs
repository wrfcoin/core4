//! Bounded retention for outbound gossip accepted before a topic is publishable.
//!
//! Entries contain the already-serialized bytes and topic used by the normal
//! gossipsub path. The swarm loop retries them only on a bounded timer tick
//! after observing a real mesh peer for the topic.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use libp2p::gossipsub::TopicHash;

use crate::message::MessageKind;

pub(crate) const MAX_PENDING_BROADCAST_ITEMS: usize = 64;
pub(crate) const MAX_PENDING_BROADCAST_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_PENDING_BROADCAST_RETRIES: u8 = 8;
pub(crate) const MAX_PENDING_BROADCAST_AGE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingBroadcastLimits {
    pub max_items: usize,
    pub max_bytes: usize,
    pub max_retries: u8,
    pub max_age: Duration,
}

impl Default for PendingBroadcastLimits {
    fn default() -> Self {
        Self {
            max_items: MAX_PENDING_BROADCAST_ITEMS,
            max_bytes: MAX_PENDING_BROADCAST_BYTES,
            max_retries: MAX_PENDING_BROADCAST_RETRIES,
            max_age: MAX_PENDING_BROADCAST_AGE,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PendingBroadcast {
    // Propagated messages use a content-derived gossipsub MessageId and can be
    // coalesced by the same digest. Genesis handshakes intentionally use
    // (source, sequence_number) instead (#2544), so equal handshake bytes are
    // distinct sends and must not be collapsed here.
    identity: Option<[u8; 32]>,
    topic: TopicHash,
    kind: MessageKind,
    data: Vec<u8>,
    enqueued_at: Instant,
    retries: u8,
}

impl PendingBroadcast {
    pub fn new(topic: TopicHash, kind: MessageKind, data: Vec<u8>, now: Instant) -> Self {
        Self {
            identity: (kind != MessageKind::GenesisHandshake)
                .then(|| *blake3::hash(&data).as_bytes()),
            topic,
            kind,
            data,
            enqueued_at: now,
            retries: 0,
        }
    }

    pub fn topic(&self) -> &TopicHash {
        &self.topic
    }

    pub fn kind(&self) -> MessageKind {
        self.kind
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnqueueOutcome {
    Queued,
    Deduplicated,
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryOutcome {
    Published,
    Duplicate,
    NoPeers,
    Terminal,
}

#[derive(Debug, Default)]
pub(crate) struct RetrySummary {
    pub published: Vec<(TopicHash, MessageKind, usize)>,
    pub duplicate: u64,
    pub attempts: u64,
    pub expired: u64,
    pub exhausted: u64,
    pub terminal: u64,
}

#[derive(Debug)]
pub(crate) struct PendingBroadcasts {
    limits: PendingBroadcastLimits,
    entries: VecDeque<PendingBroadcast>,
    total_bytes: usize,
}

impl Default for PendingBroadcasts {
    fn default() -> Self {
        Self::new(PendingBroadcastLimits::default())
    }
}

impl PendingBroadcasts {
    pub fn new(limits: PendingBroadcastLimits) -> Self {
        Self {
            limits,
            entries: VecDeque::new(),
            total_bytes: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn topics(&self) -> impl Iterator<Item = &TopicHash> {
        self.entries.iter().map(PendingBroadcast::topic)
    }

    /// Deterministic drop-newest overflow preserves already-accepted FIFO work
    /// and therefore per-topic consensus ordering.
    ///
    /// A `ChainStatus` is the one kind where only the latest value is
    /// meaningful (#3453): the receiver records the last status it sees per
    /// peer, so a retained older status delivered after a newer one regresses
    /// the peer's recorded height. A newly queued status therefore replaces
    /// any older queued status for the same topic instead of queuing behind it.
    pub fn enqueue(&mut self, pending: PendingBroadcast) -> EnqueueOutcome {
        if pending.identity.is_some()
            && self
                .entries
                .iter()
                .any(|entry| entry.identity == pending.identity)
        {
            return EnqueueOutcome::Deduplicated;
        }
        let (superseded_items, superseded_bytes) = self
            .entries
            .iter()
            .filter(|entry| Self::is_stale_status_for(entry, &pending.topic, pending.kind))
            .fold((0usize, 0usize), |(n, b), e| (n + 1, b + e.len()));
        if pending.len() > self.limits.max_bytes
            || self.entries.len() - superseded_items >= self.limits.max_items
            || (self.total_bytes - superseded_bytes).saturating_add(pending.len())
                > self.limits.max_bytes
        {
            return EnqueueOutcome::Overflow;
        }

        if superseded_items > 0 {
            self.supersede(&pending.topic, pending.kind);
        }
        self.total_bytes += pending.len();
        self.entries.push_back(pending);
        EnqueueOutcome::Queued
    }

    fn is_stale_status_for(entry: &PendingBroadcast, topic: &TopicHash, kind: MessageKind) -> bool {
        kind == MessageKind::ChainStatus && entry.kind == kind && &entry.topic == topic
    }

    /// Drop retained `ChainStatus` intents for `topic`, because a newer one has
    /// been (or is being) published (#3453). A no-op for every other kind:
    /// blocks and transactions are each individually meaningful. Returns the
    /// number of intents dropped.
    pub fn supersede(&mut self, topic: &TopicHash, kind: MessageKind) -> usize {
        if kind != MessageKind::ChainStatus {
            return 0;
        }
        let before = self.entries.len();
        self.entries
            .retain(|entry| !Self::is_stale_status_for(entry, topic, kind));
        self.total_bytes = self.entries.iter().map(PendingBroadcast::len).sum();
        before - self.entries.len()
    }

    /// Retry ready topics without spinning or allowing one failed topic to
    /// reorder later messages on that same topic.
    pub fn retry_ready<Ready, Publish>(
        &mut self,
        now: Instant,
        mut is_ready: Ready,
        mut publish: Publish,
    ) -> RetrySummary
    where
        Ready: FnMut(&TopicHash) -> bool,
        Publish: FnMut(&PendingBroadcast) -> RetryOutcome,
    {
        let mut summary = RetrySummary::default();
        // #2981: do not size an allocation from a queue length that peer traffic
        // influences. `entries.len()` is hard-capped at MAX_PENDING_BROADCAST_ITEMS
        // (64) by the `>= self.limits.max_items` guard in `enqueue`, so the old
        // `with_capacity` could not actually be driven large — but sizing an
        // allocator call from remote-influenced input is the shape the gate exists
        // to stop, and the cap is one edit away from being the only thing holding.
        // Amortized growth over at most 64 items costs nothing measurable.
        let mut retained = VecDeque::new();
        let mut blocked_topics: Vec<TopicHash> = Vec::new();

        while let Some(mut pending) = self.entries.pop_front() {
            if now.saturating_duration_since(pending.enqueued_at) >= self.limits.max_age {
                summary.expired += 1;
                continue;
            }
            if blocked_topics.contains(pending.topic()) || !is_ready(pending.topic()) {
                retained.push_back(pending);
                continue;
            }

            summary.attempts += 1;
            match publish(&pending) {
                RetryOutcome::Published => {
                    summary.published.push((
                        pending.topic().clone(),
                        pending.kind(),
                        pending.len(),
                    ));
                },
                RetryOutcome::Duplicate => summary.duplicate += 1,
                RetryOutcome::NoPeers => {
                    pending.retries = pending.retries.saturating_add(1);
                    if pending.retries >= self.limits.max_retries {
                        summary.exhausted += 1;
                    } else {
                        blocked_topics.push(pending.topic().clone());
                        retained.push_back(pending);
                    }
                },
                RetryOutcome::Terminal => summary.terminal += 1,
            }
        }

        self.entries = retained;
        self.total_bytes = self.entries.iter().map(PendingBroadcast::len).sum();
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(byte: u8, bytes: usize, now: Instant) -> PendingBroadcast {
        PendingBroadcast::new(
            libp2p::gossipsub::IdentTopic::new("wrfcoin/blocks/1").hash(),
            MessageKind::NewBlock,
            vec![byte; bytes],
            now,
        )
    }

    #[test]
    fn identical_pending_intent_is_coalesced_2790() {
        let now = Instant::now();
        let mut queue = PendingBroadcasts::default();
        assert_eq!(queue.enqueue(pending(7, 32, now)), EnqueueOutcome::Queued);
        assert_eq!(queue.enqueue(pending(7, 32, now)), EnqueueOutcome::Deduplicated);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.total_bytes(), 32);
    }

    #[test]
    fn repeated_handshake_bytes_remain_distinct_sequence_id_intents_2790() {
        let now = Instant::now();
        let topic = libp2p::gossipsub::IdentTopic::new("wrfcoin/genesis-handshake/1").hash();
        let mut queue = PendingBroadcasts::default();
        for _ in 0..2 {
            assert_eq!(
                queue.enqueue(PendingBroadcast::new(
                    topic.clone(),
                    MessageKind::GenesisHandshake,
                    vec![7; 32],
                    now,
                )),
                EnqueueOutcome::Queued
            );
        }
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn item_overflow_drops_newest_without_evicting_fifo_work_2790() {
        let now = Instant::now();
        let mut queue = PendingBroadcasts::new(PendingBroadcastLimits {
            max_items: 2,
            max_bytes: 128,
            max_retries: 2,
            max_age: Duration::from_secs(10),
        });
        assert_eq!(queue.enqueue(pending(1, 16, now)), EnqueueOutcome::Queued);
        assert_eq!(queue.enqueue(pending(2, 16, now)), EnqueueOutcome::Queued);
        assert_eq!(queue.enqueue(pending(3, 16, now)), EnqueueOutcome::Overflow);

        let mut published = Vec::new();
        let summary = queue.retry_ready(now, |_| true, |item| {
            published.push(item.data()[0]);
            RetryOutcome::Published
        });
        assert_eq!(published, vec![1, 2]);
        assert_eq!(summary.published.len(), 2);
    }

    #[test]
    fn byte_overflow_is_explicit_and_allocation_stays_bounded_2790() {
        let now = Instant::now();
        let mut queue = PendingBroadcasts::new(PendingBroadcastLimits {
            max_items: 8,
            max_bytes: 24,
            max_retries: 2,
            max_age: Duration::from_secs(10),
        });
        assert_eq!(queue.enqueue(pending(1, 16, now)), EnqueueOutcome::Queued);
        assert_eq!(queue.enqueue(pending(2, 16, now)), EnqueueOutcome::Overflow);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.total_bytes(), 16);
    }

    #[test]
    fn retry_exhaustion_and_age_expiry_are_terminal_and_bounded_2790() {
        let now = Instant::now();
        let limits = PendingBroadcastLimits {
            max_items: 4,
            max_bytes: 128,
            max_retries: 2,
            max_age: Duration::from_secs(5),
        };
        let mut exhausted = PendingBroadcasts::new(limits);
        assert_eq!(exhausted.enqueue(pending(1, 16, now)), EnqueueOutcome::Queued);
        assert_eq!(exhausted.retry_ready(now, |_| true, |_| RetryOutcome::NoPeers).attempts, 1);
        let second = exhausted.retry_ready(now, |_| true, |_| RetryOutcome::NoPeers);
        assert_eq!(second.exhausted, 1);
        assert_eq!(exhausted.len(), 0);

        let mut expired = PendingBroadcasts::new(limits);
        assert_eq!(expired.enqueue(pending(2, 16, now)), EnqueueOutcome::Queued);
        let summary = expired.retry_ready(
            now + Duration::from_secs(5),
            |_| false,
            |_| panic!("expired work must not be published"),
        );
        assert_eq!(summary.expired, 1);
        assert_eq!(expired.len(), 0);
    }

    fn status(byte: u8, topic: &str, now: Instant) -> PendingBroadcast {
        PendingBroadcast::new(
            libp2p::gossipsub::IdentTopic::new(topic).hash(),
            MessageKind::ChainStatus,
            vec![byte; 16],
            now,
        )
    }

    /// Replays the #3453 ordering: status(0) retained at the connection epoch,
    /// status(12801) then published directly once the mesh is publishable. The
    /// retained older status must not be delivered after the newer one.
    #[test]
    fn direct_publish_supersedes_retained_older_chain_status_3453() {
        let now = Instant::now();
        let mut queue = PendingBroadcasts::default();
        let topic = libp2p::gossipsub::IdentTopic::new("wrfcoin/status/1").hash();
        assert_eq!(
            queue.enqueue(status(0, "wrfcoin/status/1", now)),
            EnqueueOutcome::Queued
        );
        assert_eq!(queue.enqueue(pending(9, 16, now)), EnqueueOutcome::Queued);

        assert_eq!(queue.supersede(&topic, MessageKind::ChainStatus), 1);

        let mut published = Vec::new();
        queue.retry_ready(
            now,
            |_| true,
            |item| {
                published.push((item.kind(), item.data()[0]));
                RetryOutcome::Published
            },
        );
        assert_eq!(
            published,
            vec![(MessageKind::NewBlock, 9)],
            "stale status must not be sent"
        );
        assert_eq!(queue.total_bytes(), 0);
    }

    #[test]
    fn queued_chain_status_replaces_older_queued_status_same_topic_only_3453() {
        let now = Instant::now();
        let mut queue = PendingBroadcasts::default();
        assert_eq!(
            queue.enqueue(status(1, "wrfcoin/status/1", now)),
            EnqueueOutcome::Queued
        );
        assert_eq!(
            queue.enqueue(status(7, "wrfcoin/status/other", now)),
            EnqueueOutcome::Queued
        );
        assert_eq!(
            queue.enqueue(status(2, "wrfcoin/status/1", now)),
            EnqueueOutcome::Queued
        );
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.total_bytes(), 32);

        let mut published = Vec::new();
        queue.retry_ready(
            now,
            |_| true,
            |item| {
                published.push(item.data()[0]);
                RetryOutcome::Published
            },
        );
        assert_eq!(
            published,
            vec![7, 2],
            "only the latest status per topic survives"
        );
    }

    #[test]
    fn supersede_never_touches_blocks_or_overflow_keeps_older_status_3453() {
        let now = Instant::now();
        let topic = libp2p::gossipsub::IdentTopic::new("wrfcoin/blocks/1").hash();
        let mut queue = PendingBroadcasts::default();
        assert_eq!(queue.enqueue(pending(1, 16, now)), EnqueueOutcome::Queued);
        assert_eq!(queue.supersede(&topic, MessageKind::NewBlock), 0);
        assert_eq!(queue.len(), 1);

        let mut tight = PendingBroadcasts::new(PendingBroadcastLimits {
            max_items: 4,
            max_bytes: 16,
            max_retries: 2,
            max_age: Duration::from_secs(10),
        });
        assert_eq!(
            tight.enqueue(status(1, "wrfcoin/status/1", now)),
            EnqueueOutcome::Queued
        );
        // Replacement fits (frees 16 bytes), so it is accepted rather than lost.
        assert_eq!(
            tight.enqueue(status(2, "wrfcoin/status/1", now)),
            EnqueueOutcome::Queued
        );
        assert_eq!(tight.len(), 1);
    }
}
