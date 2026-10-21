//! On-chain slashing-report submitter (#1393).
//!
//! Periodically scans the durable evidence store for equivocation
//! records that have not yet been turned into `SlashingReport`
//! transactions and enqueues one per new record into the local mempool.
//! The state-machine apply arm in `blockchain::state::State` then
//! debits the offender's stake via `StateTransition::Slash` and stages
//! the validator for epoch-boundary eviction (commit 6 of this roadmap).
//!
//! ## Replay-protection: belt-and-suspenders
//!
//! Two layers cooperate so the same piece of evidence never debits
//! stake twice:
//!
//! 1. **Submitter cursor** (this module) — split into an in-memory
//!    *in-flight* watermark and an on-disk *confirmed* watermark
//!    (#1393 round-4 P1):
//!    - the in-flight cursor advances on successful mempool enqueue so
//!      the same evidence is not re-queued on every scan tick during a
//!      single process lifetime;
//!    - the confirmed cursor advances only when a block containing the
//!      SlashingReport is observed via [`confirm_inclusion`], and only
//!      *that* cursor is persisted to
//!      `<data_dir>/slashing_submitter_cursor.txt`.
//!    On restart, the in-flight cursor is reset to the on-disk
//!    confirmed cursor — so mempool-evicted evidence is automatically
//!    re-submitted on the first scan.
//!
//! 2. **State-layer `slashing_evidence_seen`** (commit 3) — an
//!    `FxHashSet<[u8; 32]>` keyed by `evidence_hash`. Any
//!    `SlashingReport` transaction whose hash has already been applied
//!    is rejected at the state machine with
//!    `StateError::SlashingApplication(SP_SLASHING_REPORT_REPLAY)`.
//!
//! If the cursor file is lost or corrupted, layer (2) still prevents
//! double application. If the state-root is ever rewound past a slashing
//! apply, layer (1) stops the submitter from re-flooding the mempool.
//!
//! ## Scope (as landed)
//!
//! This task constructs an **unsigned** `SlashingReport` transaction
//! (no `reporter_pubkey` signature) and enqueues it via
//! `Mempool::add_transaction`. The apply handler already verifies the
//! deterministic `evidence_hash` against the dedup set, so the
//! economic safety of the slashing path does not depend on the
//! submitter being authenticated.
//!
//! A future commit will plumb the node's validator keypair through
//! `AppState` and have the submitter populate `reporter_pubkey` + sign
//! the transaction envelope. Until then, relay-layer authentication
//! (peer trust) and the state-layer dedup together are sufficient.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::time::interval;

use wrfcoin_consensus_engines::equivocation::{
    EquivocationEvidence, EvidenceStore, SubmitterCursor,
};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::crypto::hash::blake3_hash;
use wrfcoin_shared_protocol::crypto::signer::BlockSigner;
use wrfcoin_shared_protocol::monitoring::CORE_METRICS;
use wrfcoin_shared_protocol::security::key_registry::SigningKeyRegistry;
use wrfcoin_shared_protocol::security::slashing::{
    BASE_PENALTY_DOUBLE_ATTESTATION, SlashingEvidenceType, SlashingReportPayload, scaled_penalty,
};

use crate::websocket::WsEvent;

/// Filename (under `data_dir`) that holds the last-queued evidence
/// storage key. Kept as a constant so operator tooling can locate it.
pub const CURSOR_FILENAME: &str = "slashing_submitter_cursor.txt";

/// How often the submitter wakes up to poll the evidence store.
///
/// Short enough that a double-sign is applied within one block-interval
/// of a typical chain cadence; long enough that the background scan
/// does not compete meaningfully with block production for the redb
/// reader lock.
pub const SCAN_INTERVAL_SECS: u64 = 5;

/// Derive the deterministic evidence hash that anchors replay
/// protection. Hashes the validator id + height + both block hashes so
/// duplicate observations of the same equivocation map to the same
/// hash regardless of wall-clock `detected_at`.
pub fn evidence_hash_of(evidence: &EquivocationEvidence) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 + evidence.validator.len() + 16);
    buf.extend_from_slice(evidence.validator.as_bytes());
    buf.push(0);
    buf.extend_from_slice(&evidence.height.to_le_bytes());
    buf.extend_from_slice(evidence.first_block_hash.as_bytes());
    buf.extend_from_slice(evidence.second_block_hash.as_bytes());
    *blake3_hash(&buf).as_bytes()
}

/// Build a `SlashingReportPayload` from a piece of durable evidence.
///
/// Penalty math uses `scaled_penalty(BASE_PENALTY_DOUBLE_ATTESTATION, oracle_count)`
/// so the amount is network-size-aware. The exact oracle count is not
/// visible in the submitter's scope (it would require a read lock on
/// the validator set); `1` is passed as a conservative lower bound
/// matching the detector's emitted severity. The apply handler
/// re-derives this value via
/// `expected_penalty_for_evidence_type(evidence_type)` and rejects any
/// report whose wire `penalty_amount` does not match (#1393 round-4
/// P0), so both sides must agree on the `(base, oracle_count)` pair.
///
/// `reporter_pubkey` and `reporter_signature` are left empty — use
/// [`sign_payload`] to populate them once a reporter key is plumbed in.
pub fn payload_from_evidence(evidence: &EquivocationEvidence) -> SlashingReportPayload {
    SlashingReportPayload {
        validator_id: evidence.validator.clone(),
        evidence_type: SlashingEvidenceType::DoubleSign,
        evidence_hash: evidence_hash_of(evidence),
        reported_at_height: evidence.height,
        penalty_amount: scaled_penalty(BASE_PENALTY_DOUBLE_ATTESTATION, 1),
        reporter_pubkey: String::new(),
        reporter_signature: None,
    }
}

/// Sign `payload` using `signer` and populate `reporter_pubkey` +
/// `reporter_signature` in place. Returns an error if the signer fails
/// to produce a signature. Must be called on every payload the
/// submitter enqueues — the apply handler rejects unsigned payloads
/// with `SP_SLASHING_REPORT_UNAUTHENTICATED` (#1393 round-4 P0).
pub fn sign_payload(
    signer: &dyn wrfcoin_shared_protocol::crypto::signer::BlockSigner,
    payload: &mut SlashingReportPayload,
) -> Result<(), anyhow::Error> {
    payload.reporter_pubkey = signer.public_key().to_hex();
    payload.reporter_signature = None; // signature must not cover itself
    let message = payload.canonical_signing_message();
    // #2130: sign at the height the report is about. A slashing report is a
    // claim about misbehaviour at `reported_at_height`, so that is the height
    // at which the reporter's key must be authoritative. Under an active
    // rotation plan, a key past its boundary is refused here rather than
    // submitting a report the apply handler will reject as unauthenticated.
    let signature = signer.sign_at_height(message.as_bytes(), payload.reported_at_height)?;
    payload.reporter_signature = Some(signature.to_hex());
    Ok(())
}

/// Wrap a payload into a `Transaction` carrying the JSON-encoded
/// `SlashingReportPayload` in `Transaction::data` and
/// `tx_type = SlashingReport`.
///
/// Uses a coinbase-style input + a zero-amount placeholder output
/// addressed to the validator being slashed so the transaction passes
/// `Transaction::validate_structure` when handed to the mempool. The
/// output carries no economic meaning — the state-machine apply arm
/// reads the payload in `Transaction::data`, not the UTXO fields.
pub fn build_slashing_report_tx(payload: &SlashingReportPayload) -> Option<Transaction> {
    let body = serde_json::to_string(payload).ok()?;
    // Per-evidence unique outpoint. Using the evidence_hash as the
    // input txid means each SlashingReport has a distinct
    // `(txid, vout)` pair, so the mempool's double-spend tracker does
    // not collide evidence reports against each other the way three
    // otherwise-identical coinbase inputs would.
    let input = TransactionInput {
        txid: payload.evidence_hash.into(),
        vout: 0,
        signature: None,
        public_key: None,
        coinbase_data: Some(format!(
            "slashing-report:{}:{}",
            payload.validator_id, payload.reported_at_height
        )),
    };
    let output = TransactionOutput {
        amount: 0,
        address: payload.validator_id.clone(),
        script: "OP_RETURN".to_string(),
        nft_data: None,
    };
    let mut tx = Transaction::new(vec![input], vec![output]);
    tx.tx_type = TransactionType::SlashingReport;
    tx.data = Some(body);
    tx.set_id();
    Some(tx)
}

/// Cursor file I/O. Returns an empty cursor when the file is missing,
/// empty, or malformed (which is the correct "scan from beginning"
/// signal for a fresh node). Any other error is swallowed and logged at
/// WARN so a corrupt cursor file cannot take the node down.
///
/// The on-disk format is JSON: `{"validator_id": last_height, ...}`.
/// The earlier scheme stored a single `validator:height` string and
/// compared candidates lexicographically, which silently dropped
/// evidence whose validator id sorted before the cursor's validator.
/// Per-validator watermarks remove that failure mode (#1393).
pub fn load_cursor(data_dir: &std::path::Path) -> SubmitterCursor {
    let path = data_dir.join(CURSOR_FILENAME);
    match std::fs::read_to_string(&path) { // key-file-exempt: reads a JSON slashing-cursor watermark file, not a key
        Ok(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return SubmitterCursor::new();
            }
            match serde_json::from_str::<SubmitterCursor>(trimmed) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!(
                        "Malformed slashing submitter cursor at {}: {e}. Starting scan from beginning.",
                        path.display()
                    );
                    SubmitterCursor::new()
                },
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SubmitterCursor::new(),
        Err(e) => {
            log::warn!(
                "Failed to read slashing submitter cursor at {}: {e}. Starting scan from beginning.",
                path.display()
            );
            SubmitterCursor::new()
        },
    }
}

/// Write the cursor atomically. Writes to `<name>.tmp` then renames,
/// so a crash in the middle of a write cannot leave a truncated cursor.
pub fn save_cursor(
    data_dir: &std::path::Path,
    cursor: &SubmitterCursor,
) -> std::io::Result<()> {
    let path = data_dir.join(CURSOR_FILENAME);
    let tmp = data_dir.join(format!("{CURSOR_FILENAME}.tmp"));
    let body = serde_json::to_vec(cursor).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, &body)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// One scan pass. Returns the (possibly advanced) **in-flight** cursor.
/// Visible as a standalone fn so tests can drive the submitter
/// deterministically without owning a tokio interval.
///
/// Important (#1393 round-4 P1): this function advances only the
/// in-memory in-flight cursor. Disk persistence happens elsewhere —
/// specifically [`confirm_inclusion`] + [`save_cursor`] on block
/// import. If the tx is evicted from the mempool or the node restarts
/// before mining, the confirmed cursor is unchanged, so the next scan
/// picks the evidence up again and re-submits.
///
/// If `signer` is `Some`, every enqueued payload is signed via
/// [`sign_payload`] so the apply handler accepts it. If `None`, the
/// submitter runs in a degraded "unsigned" mode (the apply arm rejects
/// with `SP_SLASHING_REPORT_UNAUTHENTICATED` — nothing applies, but we
/// log and still advance the in-flight cursor to avoid log spam).
pub async fn scan_once(
    evidence_store: &Arc<dyn EvidenceStore>,
    mempool: &Arc<RwLock<Mempool>>,
    signer: Option<&Arc<dyn BlockSigner>>,
    signing_keys: Option<&SigningKeyRegistry>,
    mut cursor: SubmitterCursor,
) -> SubmitterCursor {
    let new_evidence = match evidence_store.list_since(&cursor).await {
        Ok(v) => v,
        Err(e) => {
            log::warn!("slashing submitter list_since failed: {e}");
            return cursor;
        },
    };

    if new_evidence.is_empty() {
        return cursor;
    }

    for ev in new_evidence {
        let mut payload = payload_from_evidence(&ev);
        if let Some(signer) = signer {
            if let Err(e) = sign_payload(signer.as_ref(), &mut payload) {
                log::warn!(
                    "slashing submitter failed to sign payload for validator={} height={}: {e}. \
                     Skipping this record; will retry on next scan.",
                    ev.validator,
                    ev.height
                );
                // Do not advance the in-flight cursor — next scan retries.
                break;
            }
        } else {
            log::warn!(
                "slashing submitter has no reporter signer configured — emitting unsigned \
                 SlashingReport for validator={} height={}. Apply handler will reject with \
                 SP_SLASHING_REPORT_UNAUTHENTICATED; advancing in-flight cursor to avoid log spam.",
                ev.validator,
                ev.height
            );
        }
        let Some(tx) = build_slashing_report_tx(&payload) else {
            log::warn!(
                "slashing submitter failed to build tx for validator={} height={}",
                ev.validator,
                ev.height
            );
            continue;
        };

        let result = {
            let mut guard = mempool.write().await;
            guard.add_transaction_with_signing_keys(tx, signing_keys)
        };
        match result {
            Ok(()) => {
                log::info!(
                    "slashing submitter: queued SlashingReport validator={} height={} penalty_base_units={}",
                    ev.validator,
                    ev.height,
                    payload.penalty_amount
                );
                CORE_METRICS
                    .slashing_applied_total
                    .with_label_values(&["double_sign_queued"])
                    .inc();
                // In-flight watermark only. Confirmed cursor advances
                // from `confirm_inclusion` on block import.
                cursor.advance(&ev.validator, ev.height);
            },
            Err(e) => {
                // "already in mempool" is expected between scan ticks
                // inside a single process — not an error.
                if e.to_string().contains("already in mempool") {
                    cursor.advance(&ev.validator, ev.height);
                } else {
                    log::warn!(
                        "slashing submitter mempool enqueue failed validator={} height={}: {e}. \
                         Not advancing cursor; will retry on next scan.",
                        ev.validator,
                        ev.height
                    );
                    break;
                }
            },
        }
    }

    cursor
}

/// Scan `block` for `SlashingReport` transactions and advance
/// `confirmed_cursor` for every `(validator_id, reported_at_height)`
/// pair observed. Returns `true` if any advance happened.
///
/// This is the **only** code path that may advance the on-disk cursor.
/// `scan_once` touches an in-memory cursor for in-process dedup; disk
/// persistence gates on actual block inclusion so mempool-evicted or
/// crash-lost SlashingReports cannot silently escape retry on the
/// next scan (#1393 round-4 P1).
pub fn confirm_inclusion(confirmed_cursor: &mut SubmitterCursor, block: &Block) -> bool {
    let mut advanced = false;
    for tx in &block.transactions {
        if tx.tx_type != TransactionType::SlashingReport {
            continue;
        }
        let Some(payload) = tx.parse_slashing_report_payload() else {
            // Unparseable payload would have been rejected by the apply
            // arm, so nothing to confirm. Safe to skip.
            continue;
        };
        let before = confirmed_cursor.get(&payload.validator_id);
        confirmed_cursor.advance(&payload.validator_id, payload.reported_at_height);
        if confirmed_cursor.get(&payload.validator_id) != before {
            advanced = true;
        }
    }
    advanced
}

/// Spawn the submitter background task. Runs until the node process
/// exits. The task is best-effort: any error inside is logged and
/// scanning continues on the next tick.
///
/// `signer` is the reporter's ed25519 key used to sign outgoing
/// `SlashingReportPayload`s. If `None`, the submitter emits unsigned
/// payloads — the apply arm rejects those with
/// `SP_SLASHING_REPORT_UNAUTHENTICATED` so no stake is ever burned, but
/// the slashing path is effectively dormant. Always configure a signer
/// in production.
///
/// `blockchain` is used to fetch full blocks on `WsEvent::NewBlock`
/// notifications so [`confirm_inclusion`] can advance and persist the
/// confirmed cursor. The pre-existing `event_tx` broadcast reaches every
/// block-apply site (miner + P2P replay) so no per-caller plumbing is
/// required.
pub fn spawn_submitter_task(
    evidence_store: Arc<dyn EvidenceStore>,
    mempool: Arc<RwLock<Mempool>>,
    data_dir: PathBuf,
    signer: Option<Arc<dyn BlockSigner>>,
    blockchain: Arc<RwLock<Blockchain>>,
    mut block_event_rx: tokio::sync::broadcast::Receiver<WsEvent>,
) {
    tokio::spawn(async move {
        // Confirmed cursor: loaded from disk, only advanced by
        // `confirm_inclusion`. In-flight: starts equal to confirmed so
        // mempool-evicted evidence is re-submitted after restart.
        let mut confirmed = load_cursor(&data_dir);
        let mut in_flight = confirmed.clone();
        let mut ticker = interval(Duration::from_secs(SCAN_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        log::info!(
            "Slashing evidence submitter started (data_dir={}, interval={}s, confirmed_validators={}, signer={}) (#1393)",
            data_dir.display(),
            SCAN_INTERVAL_SECS,
            confirmed.len(),
            if signer.is_some() { "configured" } else { "NONE (unsigned mode)" },
        );

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let signing_keys = {
                        let bc = blockchain.read().await;
                        bc.state.signing_keys.clone()
                    };
                    in_flight = scan_once(
                        &evidence_store,
                        &mempool,
                        signer.as_ref(),
                        Some(&signing_keys),
                        in_flight,
                    )
                    .await;
                },
                Ok(event) = block_event_rx.recv() => {
                    let height = match event {
                        WsEvent::NewBlock { height, .. } => height,
                        _ => continue,
                    };
                    let block = {
                        let bc = blockchain.read().await;
                        bc.block_at_height(height).cloned()
                    };
                    let Some(block) = block else {
                        continue;
                    };
                    if confirm_inclusion(&mut confirmed, &block) {
                        if let Err(e) = save_cursor(&data_dir, &confirmed) {
                            log::warn!(
                                "slashing submitter failed to persist confirmed cursor to {}: {e}",
                                data_dir.display()
                            );
                        }
                    }
                },
                else => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_consensus_engines::equivocation::InMemoryEvidenceStore;
    use wrfcoin_native_chain::config::settings::Config;
    use wrfcoin_shared_protocol::crypto::hash::Hash;

    fn sample_evidence(validator: &str, height: u64, tag: u8) -> EquivocationEvidence {
        let mut first = [0u8; 32];
        first[0] = tag;
        let mut second = [0u8; 32];
        second[0] = tag ^ 0xFF;
        EquivocationEvidence {
            validator: validator.to_string(),
            height,
            first_block_hash: Hash::from(first),
            second_block_hash: Hash::from(second),
            detected_at: 1_700_000_000 + height,
        }
    }

    #[test]
    fn evidence_hash_is_deterministic_and_ignores_detected_at() {
        let a = sample_evidence("alice", 10, 1);
        let mut b = a.clone();
        b.detected_at = a.detected_at + 99_999;
        assert_eq!(
            evidence_hash_of(&a),
            evidence_hash_of(&b),
            "detected_at is advisory — duplicate observations must hash the same"
        );
    }

    #[test]
    fn evidence_hash_distinguishes_different_equivocations() {
        let a = sample_evidence("alice", 10, 1);
        let b = sample_evidence("alice", 11, 1);
        let c = sample_evidence("bob", 10, 1);
        assert_ne!(evidence_hash_of(&a), evidence_hash_of(&b));
        assert_ne!(evidence_hash_of(&a), evidence_hash_of(&c));
    }

    #[test]
    fn build_slashing_report_tx_round_trips_payload() {
        let ev = sample_evidence("alice", 42, 7);
        let payload = payload_from_evidence(&ev);
        let tx = build_slashing_report_tx(&payload).expect("tx");
        assert_eq!(tx.tx_type, TransactionType::SlashingReport);
        let parsed = tx.parse_slashing_report_payload().expect("parse");
        assert_eq!(parsed, payload);
        assert_eq!(parsed.validator_id, "alice");
        assert_eq!(parsed.reported_at_height, 42);
    }

    #[tokio::test]
    async fn scan_once_queues_one_tx_per_new_evidence_and_advances_cursor() {
        let store: Arc<dyn EvidenceStore> = Arc::new(InMemoryEvidenceStore::new());
        store
            .persist(sample_evidence("alice", 10, 1))
            .await
            .unwrap();
        store
            .persist(sample_evidence("alice", 20, 2))
            .await
            .unwrap();
        store.persist(sample_evidence("bob", 5, 3)).await.unwrap();

        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let cursor = scan_once(&store, &mempool, None, None, SubmitterCursor::new()).await;
        assert_eq!(
            cursor.get("alice"),
            Some(20),
            "alice watermark must reach the highest processed height"
        );
        assert_eq!(
            cursor.get("bob"),
            Some(5),
            "bob watermark must be recorded independently of alice"
        );

        let size = mempool.read().await.size();
        assert_eq!(size, 3, "three new evidence records → three mempool entries");
    }

    #[tokio::test]
    async fn scan_once_is_idempotent_across_calls_with_cursor() {
        let store: Arc<dyn EvidenceStore> = Arc::new(InMemoryEvidenceStore::new());
        store
            .persist(sample_evidence("alice", 10, 1))
            .await
            .unwrap();

        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let cursor1 = scan_once(&store, &mempool, None, None, SubmitterCursor::new()).await;
        assert_eq!(mempool.read().await.size(), 1);
        let cursor2 = scan_once(&store, &mempool, None, None, cursor1.clone()).await;
        assert_eq!(cursor1, cursor2, "no new evidence → cursor unchanged");
        assert_eq!(
            mempool.read().await.size(),
            1,
            "second scan must not re-queue"
        );
    }

    #[test]
    fn cursor_round_trips_through_disk() {
        let tmp = std::env::temp_dir().join(format!(
            "wrfcoin-slashing-cursor-test-{}",
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let empty = load_cursor(&tmp);
        assert!(empty.is_empty(), "missing file → empty cursor");

        let mut cursor = SubmitterCursor::new();
        cursor.advance("alice", 10);
        cursor.advance("bob", 42);
        save_cursor(&tmp, &cursor).unwrap();

        let reloaded = load_cursor(&tmp);
        assert_eq!(reloaded.get("alice"), Some(10));
        assert_eq!(reloaded.get("bob"), Some(42));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── #1393: submitter restart idempotency ──────────────────────────
    //
    // Simulates a node restart by running `scan_once`, persisting the
    // cursor to disk, dropping the mempool, then reloading the cursor
    // from disk and running `scan_once` again against a *fresh* mempool.
    // The second pass must NOT re-queue the already-processed evidence
    // even though the mempool is empty. This is the cursor's job —
    // layer 1 of the belt-and-suspenders replay defence.
    #[tokio::test]
    async fn submitter_cursor_survives_simulated_restart() {
        let store: Arc<dyn EvidenceStore> = Arc::new(InMemoryEvidenceStore::new());
        store
            .persist(sample_evidence("alice", 10, 1))
            .await
            .unwrap();
        store
            .persist(sample_evidence("bob", 20, 2))
            .await
            .unwrap();

        let tmp = std::env::temp_dir().join(format!(
            "wrfcoin-submitter-restart-test-{}",
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        // Pre-restart: scan from scratch, both records get queued.
        let mempool_v1 = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let cursor_v1 = scan_once(&store, &mempool_v1, None, None, SubmitterCursor::new()).await;
        assert_eq!(
            cursor_v1.get("alice"),
            Some(10),
            "alice watermark must advance on first scan"
        );
        assert_eq!(
            cursor_v1.get("bob"),
            Some(20),
            "bob watermark must advance on first scan"
        );
        assert_eq!(
            mempool_v1.read().await.size(),
            2,
            "two evidence records → two mempool entries"
        );
        save_cursor(&tmp, &cursor_v1).unwrap();

        // Simulate restart: drop the mempool, reload the cursor from disk.
        drop(mempool_v1);
        let cursor_v2 = load_cursor(&tmp);
        assert_eq!(
            cursor_v2, cursor_v1,
            "reloaded cursor must equal pre-restart cursor"
        );

        // Post-restart with fresh mempool: no new evidence arrived, so
        // the submitter must enqueue nothing — the cursor is what keeps
        // the node from re-flooding its own mempool with proofs the
        // state machine has already applied.
        let mempool_v2 = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let cursor_v3 = scan_once(&store, &mempool_v2, None, None, cursor_v2).await;
        assert_eq!(
            cursor_v3, cursor_v1,
            "cursor must not advance when no new evidence is present"
        );
        assert_eq!(
            mempool_v2.read().await.size(),
            0,
            "post-restart scan must not re-queue already-processed evidence"
        );

        // Now add one *new* piece of evidence post-restart. Only that
        // new record should land in the post-restart mempool.
        store
            .persist(sample_evidence("carol", 30, 3))
            .await
            .unwrap();
        let cursor_v4 = scan_once(&store, &mempool_v2, None, None, cursor_v3).await;
        assert_ne!(
            cursor_v4, cursor_v1,
            "cursor must advance when new evidence arrives"
        );
        assert_eq!(
            mempool_v2.read().await.size(),
            1,
            "only the newly-persisted record must enter the post-restart mempool"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── #1393 round-4 P1: confirm-on-inclusion + mempool-eviction retry ───
    //
    // The submitter's on-disk cursor must reflect *confirmed inclusion*,
    // not *mempool enqueue*. Otherwise a tx that gets evicted from the
    // mempool (or a node that crashes before the block mines) silently
    // loses the evidence: the cursor advanced, so the next scan skips
    // the record and never re-submits. These tests pin the intended
    // semantics by driving `scan_once` + `confirm_inclusion` directly.

    fn slashing_report_tx_in_block(
        payload: &SlashingReportPayload,
    ) -> Transaction {
        build_slashing_report_tx(payload).expect("build tx")
    }

    #[test]
    fn confirm_inclusion_advances_cursor_for_reports_in_block() {
        use wrfcoin_shared_protocol::blockchain::block::Block;
        use wrfcoin_shared_protocol::crypto::hash::Hash;

        let payload_a = payload_from_evidence(&sample_evidence("alice", 10, 1));
        let payload_b = payload_from_evidence(&sample_evidence("bob", 50, 2));
        let block = Block::new(
            3,
            Hash::default(),
            vec![
                slashing_report_tx_in_block(&payload_a),
                slashing_report_tx_in_block(&payload_b),
            ],
            1,
            100,
        );

        let mut cursor = SubmitterCursor::new();
        let advanced = confirm_inclusion(&mut cursor, &block);
        assert!(advanced, "block with SlashingReports must advance cursor");
        assert_eq!(cursor.get("alice"), Some(10));
        assert_eq!(cursor.get("bob"), Some(50));
    }

    #[test]
    fn confirm_inclusion_is_monotonic_and_ignores_non_slashing_txs() {
        use wrfcoin_shared_protocol::blockchain::block::Block;
        use wrfcoin_shared_protocol::crypto::hash::Hash;

        // Cursor pre-advanced past alice@20 (e.g. replayed a different
        // block). A SlashingReport for alice@5 in a later block must
        // NOT regress alice's watermark.
        let mut cursor = SubmitterCursor::new();
        cursor.advance("alice", 20);

        let stale = payload_from_evidence(&sample_evidence("alice", 5, 3));
        let mut regular = Transaction::new(vec![], vec![]);
        regular.tx_type = TransactionType::Regular; // irrelevant to submitter
        let block = Block::new(
            3,
            Hash::default(),
            vec![slashing_report_tx_in_block(&stale), regular],
            1,
            100,
        );

        let advanced = confirm_inclusion(&mut cursor, &block);
        assert!(
            !advanced,
            "older SlashingReport must not advance the watermark"
        );
        assert_eq!(
            cursor.get("alice"),
            Some(20),
            "alice watermark must not regress below 20"
        );
    }

    #[tokio::test]
    async fn mempool_evicted_evidence_is_resubmitted_after_restart() {
        use wrfcoin_shared_protocol::blockchain::block::Block;
        use wrfcoin_shared_protocol::crypto::hash::Hash;

        let tmp = std::env::temp_dir().join(format!(
            "wrfcoin-submitter-eviction-test-{}",
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let store: Arc<dyn EvidenceStore> = Arc::new(InMemoryEvidenceStore::new());
        store
            .persist(sample_evidence("alice", 10, 1))
            .await
            .unwrap();

        // Pre-restart scan: evidence gets enqueued, in-flight advances.
        // Confirmed cursor (on disk) stays empty because no block was
        // observed containing the SlashingReport.
        let mempool_v1 = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let in_flight_v1 = scan_once(&store, &mempool_v1, None, None, SubmitterCursor::new()).await;
        assert_eq!(
            in_flight_v1.get("alice"),
            Some(10),
            "in-flight cursor must advance on enqueue"
        );
        assert_eq!(mempool_v1.read().await.size(), 1);

        // No block confirms the tx — simulate mempool eviction by
        // dropping the mempool entirely (this is what happens on a
        // node crash too).
        let confirmed_v1 = SubmitterCursor::new();
        save_cursor(&tmp, &confirmed_v1).unwrap();
        drop(mempool_v1);

        // Simulate restart: load the *confirmed* cursor from disk
        // (still empty) and use it as the starting point for the new
        // in-flight cursor.
        let confirmed_v2 = load_cursor(&tmp);
        assert!(
            confirmed_v2.is_empty(),
            "no block confirmed the tx — confirmed cursor must stay empty on disk"
        );

        // Post-restart scan with a fresh mempool. Evidence must be
        // re-queued because the confirmed cursor has no watermark for
        // alice. This is the whole point of the round-4 P1 fix — the
        // old single-cursor design would have advanced + persisted the
        // cursor on enqueue, so the post-restart scan would have
        // skipped this evidence and the slash would have been lost.
        let mempool_v2 = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let in_flight_v2 = scan_once(&store, &mempool_v2, None, None, confirmed_v2.clone()).await;
        assert_eq!(
            in_flight_v2.get("alice"),
            Some(10),
            "post-restart scan must re-queue the evicted evidence"
        );
        assert_eq!(
            mempool_v2.read().await.size(),
            1,
            "evicted evidence must reappear in the fresh mempool"
        );

        // Now a block containing the SlashingReport is observed.
        // `confirm_inclusion` advances the confirmed cursor; save to
        // disk. A later scan must then NOT re-queue.
        let payload = payload_from_evidence(&sample_evidence("alice", 10, 1));
        let block = Block::new(
            3,
            Hash::default(),
            vec![slashing_report_tx_in_block(&payload)],
            1,
            100,
        );
        let mut confirmed_v3 = confirmed_v2;
        let advanced = confirm_inclusion(&mut confirmed_v3, &block);
        assert!(advanced);
        save_cursor(&tmp, &confirmed_v3).unwrap();

        // A second restart: confirmed cursor now carries alice@10.
        let confirmed_v4 = load_cursor(&tmp);
        assert_eq!(confirmed_v4.get("alice"), Some(10));
        let mempool_v3 = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let _in_flight_v3 = scan_once(&store, &mempool_v3, None, None, confirmed_v4).await;
        assert_eq!(
            mempool_v3.read().await.size(),
            0,
            "confirmed evidence must NOT be re-queued on a subsequent restart"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
