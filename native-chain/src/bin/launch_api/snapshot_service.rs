//! Node-local snapshot cache and bounded receipt ledger shared by AppState harnesses.
//! This state is independent of the launch_api route namespace and never enters consensus.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::snapshot_sync::SnapshotSyncPayload;
use wrfcoin_shared_protocol::consensus::node_service_receipt::{
    NodeService, NodeServiceReceipt, SignedNodeServiceReceipt, snapshot_subject_hash,
};

/// Freshness window a serving node accepts a receipt inside (either side).
pub const RECEIPT_FRESHNESS_WINDOW_SECS: i64 = 15 * 60;
/// Node-wide capacity for the 15-minute rolling operator evidence window.
/// Unexpired accepted receipts are never evicted; this is not durable archival.
pub const RECEIPT_STORE_MAX: usize = 4_096;
/// Maximum retained receipts per consumer public key, across all bearer callers.
const RECEIPTS_PER_CONSUMER_MAX: usize = 16;
/// Admissions per 60-second monotonic window for one authenticated bearer.
/// A "caller" is the bearer token (hashed in memory), not a client or source IP.
/// Auth configures one token per role: all Reader-token holders share this
/// 32/minute budget and the 64-receipt retained share below. One holder can
/// exhaust both for every Reader client; distinct other-role bearers keep their
/// caller budgets, subject to the node-wide cap and per-consumer-key limits.
const RECEIPTS_PER_MINUTE_MAX: usize = 32;
/// Per-consumer-key admission budget; eight/minute is the only per-consumer
/// rate isolation among clients sharing a bearer, with no reserved capacity.
const RECEIPTS_PER_KEY_PER_MINUTE_MAX: usize = 8;
/// Retained share per authenticated bearer, shared by every client using it.
const RECEIPTS_PER_CALLER_MAX: usize = 64;
const RECENT_EXPORT_CUTS_MAX: usize = 32;
/// Accepted receipts expire 15 minutes after admission, measured monotonically.
/// Admission and Operator reads remove expired evidence and bookkeeping.
/// Operators must collect receipts via GET /network/snapshot-sync/receipts
/// before expiry; longer archival and settlement require a durable sink (#2848).
const RECEIPT_RETENTION: Duration = Duration::from_secs(RECEIPT_FRESHNESS_WINDOW_SECS as u64);

/// Node-local service state. One immutable cut is reused until height, tip,
/// finality or the stored state-root commitment changes. Same-height pages
/// therefore describe the captured cut, not a mixture of mutable live state.
#[derive(Default)]
pub struct SnapshotServiceState {
    pub(crate) export: tokio::sync::Mutex<Option<Arc<SnapshotExportCut>>>,
    pub(crate) ledger: Mutex<ReceiptLedger>,
    recent_exports: Mutex<VecDeque<ServedCut>>,
    #[cfg(test)]
    pub(crate) export_builds: std::sync::atomic::AtomicUsize,
}

/// Only small served-subject descriptors are retained, not snapshot entry values.
struct ServedCut {
    height: u64,
    subject: [u8; 32],
    units: u64,
    served_at: Instant,
}

impl SnapshotServiceState {
    pub(crate) fn remember_cut(&self, cut: &SnapshotExportCut, now: Instant) {
        let mut recent = self
            .recent_exports
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        recent.retain(|c| now.saturating_duration_since(c.served_at) < RECEIPT_RETENTION);
        let subject = snapshot_subject_hash(&cut.snapshot_root_hex);
        recent.retain(|c| !(c.height == cut.height && c.subject == subject));
        recent.push_back(ServedCut {
            height: cut.height,
            subject,
            units: cut.entries.len() as u64,
            served_at: now,
        });
        while recent.len() > RECENT_EXPORT_CUTS_MAX {
            recent.pop_front();
        }
    }

    pub(crate) fn matches_recent_receipt(
        &self,
        receipt: &NodeServiceReceipt,
        now: Instant,
    ) -> bool {
        let mut recent = self
            .recent_exports
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        recent.retain(|c| now.saturating_duration_since(c.served_at) < RECEIPT_RETENTION);
        receipt.service == NodeService::SnapshotJoin
            && recent.iter().any(|c| {
                c.height == receipt.chain_height
                    && c.subject == receipt.subject_hash
                    && c.units == receipt.units
            })
    }
}

pub(crate) struct SnapshotExportCut {
    pub(crate) height: u64,
    pub(crate) tip_hash_hex: String,
    pub(crate) finalized_height: u64,
    pub(crate) stored_state_root: String,
    pub(crate) state_root_hex: String,
    pub(crate) snapshot_root_hex: String,
    pub(crate) entries: Vec<(Vec<u8>, Vec<u8>)>,
}

impl SnapshotExportCut {
    pub(crate) fn matches_chain(&self, chain: &Blockchain) -> bool {
        self.height == chain.height
            && self.tip_hash_hex == chain.tip.to_hex()
            && self.finalized_height == chain.finalized_height()
            && self.stored_state_root == hex::encode(chain.state.state_root.0)
    }

    pub(crate) fn payload(&self) -> SnapshotSyncPayload {
        SnapshotSyncPayload {
            height: self.height,
            state_root_hex: self.state_root_hex.clone(),
            snapshot_root_hex: self.snapshot_root_hex.clone(),
            entry_count: self.entries.len() as u64,
            tip_hash_hex: self.tip_hash_hex.clone(),
            finalized_height: self.finalized_height,
        }
    }
}

#[derive(Default)]
pub(crate) struct ReceiptLedger {
    pub(crate) receipts: Vec<SignedNodeServiceReceipt>,
    admitted: Vec<(Instant, [u8; 32])>,
    dedup: HashSet<[u8; 32]>,
    per_consumer: HashMap<[u8; 32], usize>,
    per_caller: HashMap<[u8; 32], usize>,
    caller_windows: HashMap<[u8; 32], (Instant, usize)>,
    key_windows: HashMap<[u8; 32], (Instant, usize)>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReceiptLedgerError {
    Duplicate,
    Capacity,
    RateLimited,
}

impl ReceiptLedger {
    pub(crate) fn expire(&mut self, now: Instant) {
        let receipts = std::mem::take(&mut self.receipts);
        let admitted = std::mem::take(&mut self.admitted);
        self.dedup.clear();
        self.per_consumer.clear();
        self.per_caller.clear();
        for (receipt, (at, caller)) in receipts.into_iter().zip(admitted) {
            if now.saturating_duration_since(at) >= RECEIPT_RETENTION {
                continue;
            }
            self.dedup.insert(receipt.dedup_key());
            *self
                .per_consumer
                .entry(receipt.receipt.consumer_pubkey)
                .or_default() += 1;
            *self.per_caller.entry(caller).or_default() += 1;
            self.receipts.push(receipt);
            self.admitted.push((at, caller));
        }
        self.caller_windows
            .retain(|_, (at, _)| now.saturating_duration_since(*at) < Duration::from_secs(60));
        self.key_windows
            .retain(|_, (at, _)| now.saturating_duration_since(*at) < Duration::from_secs(60));
    }

    pub(crate) fn record(
        &mut self,
        receipt: SignedNodeServiceReceipt,
        caller: [u8; 32],
        now: Instant,
    ) -> Result<usize, ReceiptLedgerError> {
        self.expire(now);
        let key = receipt.dedup_key();
        if self.dedup.contains(&key) {
            return Err(ReceiptLedgerError::Duplicate);
        }
        let consumer = receipt.receipt.consumer_pubkey;
        if self.receipts.len() >= RECEIPT_STORE_MAX
            || self.per_consumer.get(&consumer).copied().unwrap_or(0) >= RECEIPTS_PER_CONSUMER_MAX
            || self.per_caller.get(&caller).copied().unwrap_or(0) >= RECEIPTS_PER_CALLER_MAX
        {
            return Err(ReceiptLedgerError::Capacity);
        }
        // Key rotation cannot bypass one authenticated caller's budget. Failed
        // admissions charge neither bucket; other callers retain their budgets.
        if self
            .caller_windows
            .get(&caller)
            .is_some_and(|(_, n)| *n >= RECEIPTS_PER_MINUTE_MAX)
            || self
                .key_windows
                .get(&consumer)
                .is_some_and(|(_, n)| *n >= RECEIPTS_PER_KEY_PER_MINUTE_MAX)
        {
            return Err(ReceiptLedgerError::RateLimited);
        }
        self.caller_windows.entry(caller).or_insert((now, 0)).1 += 1;
        self.key_windows.entry(consumer).or_insert((now, 0)).1 += 1;
        self.dedup.insert(key);
        *self.per_consumer.entry(consumer).or_default() += 1;
        *self.per_caller.entry(caller).or_default() += 1;
        self.receipts.push(receipt);
        self.admitted.push((now, caller));
        Ok(self.receipts.len())
    }
}

#[cfg(test)]
mod receipt_admission_tests {
    use super::*;

    fn claim(consumer: u64, subject: u64) -> SignedNodeServiceReceipt {
        let mut key = [0; 32];
        key[..8].copy_from_slice(&consumer.to_le_bytes());
        let mut root = [0; 32];
        root[..8].copy_from_slice(&subject.to_le_bytes());
        SignedNodeServiceReceipt {
            receipt: NodeServiceReceipt {
                service: NodeService::SnapshotJoin,
                server_id: "synthetic-server".into(),
                consumer_pubkey: key,
                chain_height: 5,
                subject_hash: root,
                units: 1,
                timestamp: 1,
                nonce: [0; 32],
            },
            signature: Vec::new(), // Internal store test; API verifies signatures before admission.
        }
    }

    #[test]
    fn caller_budget_survives_key_rotation_without_starving_other_callers() {
        let mut ledger = ReceiptLedger::default();
        let now = Instant::now();
        for i in 0..32 {
            ledger.record(claim(i, 1), [1; 32], now).unwrap();
        }
        assert_eq!(
            ledger.record(claim(999, 1), [1; 32], now),
            Err(ReceiptLedgerError::RateLimited)
        );
        assert_eq!(ledger.record(claim(999, 1), [2; 32], now), Ok(33));
        assert_eq!(
            ledger.record(claim(0, 1), [1; 32], now),
            Err(ReceiptLedgerError::Duplicate)
        );
        assert_eq!(
            ledger.record(claim(1_000, 1), [1; 32], now + Duration::from_secs(60)),
            Ok(34)
        );
    }

    #[test]
    fn key_limit_charges_neither_other_key_nor_caller_on_refusal() {
        let mut ledger = ReceiptLedger::default();
        let now = Instant::now();
        for subject in 0..8 {
            ledger.record(claim(1, subject), [1; 32], now).unwrap();
        }
        assert_eq!(
            ledger.record(claim(1, 999), [2; 32], now),
            Err(ReceiptLedgerError::RateLimited)
        );
        assert!(!ledger.caller_windows.contains_key(&[2; 32]));
        for subject in 8..16 {
            ledger
                .record(claim(1, subject), [1; 32], now + Duration::from_secs(60))
                .unwrap();
        }
        assert_eq!(
            ledger.record(claim(1, 999), [2; 32], now + Duration::from_secs(60)),
            Err(ReceiptLedgerError::Capacity)
        );
    }

    #[test]
    fn capacity_retains_unexpired_evidence_and_recovers_without_restart() {
        let mut ledger = ReceiptLedger::default();
        let start = Instant::now();
        for round in 0..2 {
            for i in 0..2048u64 {
                let mut caller = [0; 32];
                caller[..8].copy_from_slice(&(i / 32).to_le_bytes());
                ledger
                    .record(
                        claim(i + round * 2048, 1),
                        caller,
                        start + Duration::from_secs(round * 60),
                    )
                    .unwrap();
            }
        }
        let first = ledger.receipts.first().unwrap().clone();
        let last = ledger.receipts.last().unwrap().clone();
        assert_eq!(
            ledger.record(
                claim(99_999, 1),
                [255; 32],
                start + Duration::from_secs(899)
            ),
            Err(ReceiptLedgerError::Capacity)
        );
        assert_eq!(ledger.receipts.len(), RECEIPT_STORE_MAX);
        assert_eq!(ledger.receipts.first(), Some(&first));
        assert_eq!(ledger.receipts.last(), Some(&last));
        assert_eq!(
            ledger.record(first, [255; 32], start + Duration::from_secs(899)),
            Err(ReceiptLedgerError::Duplicate)
        );
        assert_eq!(
            ledger.record(claim(99_999, 1), [255; 32], start + RECEIPT_RETENTION),
            Ok(2049)
        );
        assert_eq!(
            ledger.receipts.first().unwrap().receipt.consumer_pubkey,
            claim(2048, 1).receipt.consumer_pubkey
        );
        ledger.expire(start + RECEIPT_RETENTION + Duration::from_secs(60));
        assert_eq!(ledger.receipts.len(), 1);
        assert_eq!(ledger.dedup.len(), 1);
        assert_eq!(ledger.per_consumer.len(), 1);
        assert_eq!(ledger.per_caller.len(), 1);
        assert!(ledger.caller_windows.is_empty());
        assert!(ledger.key_windows.is_empty());
    }

    #[test]
    fn one_caller_cannot_fill_the_evidence_window() {
        let mut ledger = ReceiptLedger::default();
        let now = Instant::now();
        for i in 0..64 {
            ledger
                .record(claim(i, 1), [1; 32], now + Duration::from_secs(i / 32 * 60))
                .unwrap();
        }
        assert_eq!(
            ledger.record(claim(65, 1), [1; 32], now + Duration::from_secs(120)),
            Err(ReceiptLedgerError::Capacity)
        );
        assert_eq!(
            ledger.record(claim(65, 1), [2; 32], now + Duration::from_secs(120)),
            Ok(65)
        );
        assert_eq!(
            ledger.record(claim(66, 1), [1; 32], now + RECEIPT_RETENTION),
            Ok(34)
        );
    }

    #[test]
    fn served_cut_history_is_bounded_and_expires() {
        let service = SnapshotServiceState::default();
        let now = Instant::now();
        let mut cut = SnapshotExportCut {
            height: 5,
            tip_hash_hex: "00".into(),
            finalized_height: 0,
            stored_state_root: "00".into(),
            state_root_hex: "00".into(),
            snapshot_root_hex: "00".repeat(32),
            entries: vec![(vec![1], vec![2])],
        };
        service.remember_cut(&cut, now);
        let mut receipt = claim(1, 1).receipt;
        receipt.subject_hash = snapshot_subject_hash(&cut.snapshot_root_hex);
        assert!(service.matches_recent_receipt(&receipt, now));
        for height in 6..=36 {
            cut.height = height;
            service.remember_cut(&cut, now);
        }
        assert!(service.matches_recent_receipt(&receipt, now));
        cut.height = 37;
        service.remember_cut(&cut, now);
        assert!(!service.matches_recent_receipt(&receipt, now));
        assert_eq!(
            service.recent_exports.lock().unwrap().len(),
            RECENT_EXPORT_CUTS_MAX
        );
        receipt.chain_height = 37;
        assert!(service.matches_recent_receipt(&receipt, now));
        receipt.units += 1;
        assert!(!service.matches_recent_receipt(&receipt, now));
        receipt.units -= 1;
        assert!(!service.matches_recent_receipt(&receipt, now + RECEIPT_RETENTION));
        assert!(service.recent_exports.lock().unwrap().is_empty());
    }
}
