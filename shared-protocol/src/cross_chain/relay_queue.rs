use std::collections::VecDeque;

use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

use super::types::{BridgeMessage, RelayMessageStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayQueueConfig {
    pub max_retries: u32,
}

impl Default for RelayQueueConfig {
    fn default() -> Self {
        Self { max_retries: 3 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedRelayMessage {
    pub message: BridgeMessage,
    pub enqueued_at: i64,
    pub last_updated_at: i64,
    pub attempts: u32,
    pub status: RelayMessageStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

pub struct RelayQueue {
    config: RelayQueueConfig,
    queue: VecDeque<QueuedRelayMessage>,
    pending_ids: FxHashSet<String>,
    processed_ids: FxHashSet<String>,
}

impl RelayQueue {
    pub fn new(config: RelayQueueConfig) -> Self {
        Self {
            config,
            queue: VecDeque::new(),
            pending_ids: FxHashSet::default(),
            processed_ids: FxHashSet::default(),
        }
    }

    pub fn enqueue(&mut self, message: BridgeMessage, now_ts: i64) -> bool {
        let id = message.message_id.clone();
        if self.pending_ids.contains(&id) || self.processed_ids.contains(&id) {
            return false;
        }
        self.pending_ids.insert(id);
        self.queue.push_back(QueuedRelayMessage {
            message,
            enqueued_at: now_ts,
            last_updated_at: now_ts,
            attempts: 0,
            status: RelayMessageStatus::Pending,
            last_error: None,
        });
        true
    }

    pub fn dequeue_for_processing(&mut self, now_ts: i64) -> Option<QueuedRelayMessage> {
        let mut item = self.queue.pop_front()?;
        self.pending_ids.remove(&item.message.message_id);
        item.status = RelayMessageStatus::Processing;
        item.last_updated_at = now_ts;
        item.attempts = item.attempts.saturating_add(1);
        Some(item)
    }

    pub fn mark_processed(&mut self, item: QueuedRelayMessage) {
        self.processed_ids.insert(item.message.message_id);
    }

    pub fn mark_failed(&mut self, mut item: QueuedRelayMessage, error: String, now_ts: i64) {
        item.last_error = Some(error);
        item.last_updated_at = now_ts;
        // attempts is already incremented by dequeue_for_processing before this is called.
        // Using `>` ensures the message is retried up to `max_retries` times after the
        // first attempt (i.e. max_retries + 1 total attempts).
        if item.attempts > self.config.max_retries {
            item.status = RelayMessageStatus::Failed;
            self.processed_ids.insert(item.message.message_id);
            return;
        }
        item.status = RelayMessageStatus::Pending;
        let message_id = item.message.message_id.clone();
        self.pending_ids.insert(message_id);
        self.queue.push_back(item);
    }

    pub fn is_processed(&self, message_id: &str) -> bool {
        self.processed_ids.contains(message_id)
    }

    pub fn pending_len(&self) -> usize {
        self.queue.len()
    }
}

impl Default for RelayQueue {
    fn default() -> Self {
        Self::new(RelayQueueConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cross_chain::types::{BridgeMessageType, CrossChainDirection, CrossChainNetwork};

    fn sample_message(id_nonce: u64) -> BridgeMessage {
        BridgeMessage::new(
            CrossChainNetwork::Ethereum,
            CrossChainNetwork::Base,
            CrossChainDirection::Outbound,
            BridgeMessageType::Lock,
            "0xsender".to_string(),
            "0xrecipient".to_string(),
            10,
            id_nonce,
            1_700_000_000,
        )
    }

    #[test]
    fn queue_deduplicates_message_ids() {
        let mut queue = RelayQueue::default();
        let message = sample_message(1);
        assert!(queue.enqueue(message.clone(), 10));
        assert!(!queue.enqueue(message, 11));
        assert_eq!(queue.pending_len(), 1);
    }

    #[test]
    fn failed_message_requeues_until_max_retries() {
        // max_retries: 2 means: 1 initial attempt + 2 retries = 3 total attempts.
        // After the 3rd attempt (attempts == 3 > max_retries == 2) the message is exhausted.
        let mut queue = RelayQueue::new(RelayQueueConfig { max_retries: 2 });
        let message = sample_message(2);
        assert!(queue.enqueue(message, 10));

        // Attempt 1 — requeued (attempts=1, 1 > 2 is false)
        let first = queue.dequeue_for_processing(11).unwrap();
        queue.mark_failed(first, "temporary".to_string(), 12);
        assert_eq!(queue.pending_len(), 1);

        // Attempt 2 — requeued (attempts=2, 2 > 2 is false)
        let second = queue.dequeue_for_processing(13).unwrap();
        queue.mark_failed(second, "temporary".to_string(), 14);
        assert_eq!(queue.pending_len(), 1);

        // Attempt 3 — exhausted (attempts=3, 3 > 2 is true)
        let third = queue.dequeue_for_processing(15).unwrap();
        let id = third.message.message_id.clone();
        queue.mark_failed(third, "permanent".to_string(), 16);

        assert!(queue.is_processed(&id));
        assert_eq!(queue.pending_len(), 0);
    }
}
