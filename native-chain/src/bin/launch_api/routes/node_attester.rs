//! `GET /node/attester` — this node's own rung on the attester step of the
//! participation ladder, for the operator status page (`/dashboard`).
//!
//! The node never holds the oracle signing key (the separate attester process
//! does). It knows only the key's PUBLIC half, which is the oracle id:
//!
//! - `WRFCOIN_ORACLE_PUBLIC_KEY` (64 hex), or
//! - `WRFCOIN_ORACLE_PUBLIC_KEY_FILE`, a file whose first line is that hex. The
//!   Windows launcher points it at `%LOCALAPPDATA%\WRFCoin\oracle-public.txt`,
//!   which "Create Attester Key" writes; the file is read on every request, so
//!   a key created while the node runs shows up without a restart.
//!
//! Everything else comes from the node's own state and blocks: whether that id
//! is in the CURRENT seated oracle set, since which block (from the
//! `OracleRegistration` transactions in the node's stored blocks), how many
//! attestations it signed in the last [`ATTESTATION_WINDOW_BLOCKS`] blocks, and
//! when the local attester last polled `/transactions/oracle/pending` with the
//! Oracle-role token (the attester polls every few seconds while it runs).
//!
//! Read only: no state, validity or schedule change. Operator token required
//! (the page already holds it for host metrics); the answer carries only public
//! data (the oracle id is already on chain once seated).

use std::sync::LazyLock;
use std::sync::atomic::Ordering;

use actix_web::{HttpRequest, HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::helpers::require_role;
use crate::state::AppState;
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::transaction::TransactionType;
use wrfcoin_native_chain::security::api_auth::ApiRole;

/// Attestations are counted over this many most recent blocks (about two
/// hours at the chain 888 cadence of ~77 s per block).
pub const ATTESTATION_WINDOW_BLOCKS: u64 = 100;
/// How far back the "seated since" search reads stored blocks.
pub const REGISTRATION_SCAN_BLOCKS: u64 = 50_000;
/// Blocks read from storage per batch during that search.
const SCAN_BATCH_BLOCKS: u64 = 500;
/// The attester polls every 5 s by default; a poll within this many seconds
/// means it is running.
pub const ATTESTER_POLL_FRESH_SECS: u64 = 120;
/// Largest key file the node reads (the real one is 65 or 66 bytes).
const MAX_KEY_FILE_BYTES: u64 = 1024;

/// Where this node's oracle public key comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalOracleKey {
    /// No key configured: the node has no attester.
    None,
    /// A configured source that does not hold a usable key yet.
    Unreadable(&'static str),
    /// The oracle id (64 lowercase hex).
    Key(String),
}

/// A 64-hex ed25519 public key, lowercased; anything else is refused.
pub fn parse_public_key_text(text: &str) -> Option<String> {
    let first = text.trim_start_matches('\u{feff}').lines().next()?.trim();
    if first.len() == 64 && first.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(first.to_ascii_lowercase())
    } else {
        None
    }
}

/// Resolve the key from the two variables' values (`env_key`, `env_file`).
pub fn resolve_local_key(env_key: Option<String>, env_file: Option<String>) -> LocalOracleKey {
    if let Some(v) = env_key.filter(|v| !v.trim().is_empty()) {
        return match parse_public_key_text(&v) {
            Some(k) => LocalOracleKey::Key(k),
            None => {
                LocalOracleKey::Unreadable("WRFCOIN_ORACLE_PUBLIC_KEY is not a 64-hex public key")
            },
        };
    }
    let Some(path) = env_file.filter(|v| !v.trim().is_empty()) else {
        return LocalOracleKey::None;
    };
    let path = std::path::Path::new(path.trim());
    match std::fs::metadata(path) {
        // The launcher always names the file; until "Create Attester Key" runs it
        // does not exist, which is simply "no attester key yet".
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LocalOracleKey::None,
        Err(_) => LocalOracleKey::Unreadable("the oracle public key file cannot be read"),
        Ok(m) if !m.is_file() || m.len() > MAX_KEY_FILE_BYTES => {
            LocalOracleKey::Unreadable("the oracle public key file is not a public key")
        },
        Ok(_) => match std::fs::read_to_string(path) { // key-file-exempt: the PUBLIC oracle key (oracle id), never key material
            Ok(text) => match parse_public_key_text(&text) {
                Some(k) => LocalOracleKey::Key(k),
                None => {
                    LocalOracleKey::Unreadable("the oracle public key file is not a public key")
                },
            },
            Err(_) => LocalOracleKey::Unreadable("the oracle public key file cannot be read"),
        },
    }
}

fn local_key_from_env() -> LocalOracleKey {
    resolve_local_key(
        // determinism-ok: operator config for the status page, never a consensus input.
        std::env::var("WRFCOIN_ORACLE_PUBLIC_KEY").ok(),
        // determinism-ok: operator config for the status page, never a consensus input.
        std::env::var("WRFCOIN_ORACLE_PUBLIC_KEY_FILE").ok(),
    )
}

/// The rung the page shows. Serialized as the `status` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttesterStatus {
    /// No oracle key on this node: the ladder stays as the bonded/sync logic says.
    NoKey,
    /// A key source is configured but holds no usable key.
    KeyUnreadable,
    /// Key created, not in the seated set (never seated in the blocks read).
    WaitingToBeSeated,
    /// Was seated, then a registration removed it: back to the rung below.
    Removed,
    /// Seated and signed attestations recently (or has some in the mempool).
    Attesting,
    /// Seated, the attester is running, but no attestation in the window
    /// (nothing needed attesting, or it is refusing). Not a warning by itself.
    SeatedQuiet,
    /// Seated, no recent attestation, and no sign of the attester running.
    SeatedIdle,
}

/// The rung decision, from facts the handler gathered.
pub fn decide_status(
    key: &LocalOracleKey,
    seated: bool,
    ever_seated_in_scan: bool,
    recent_attestations: u64,
    mempool_attestations: u64,
    attester_polling: bool,
) -> AttesterStatus {
    match key {
        LocalOracleKey::None => AttesterStatus::NoKey,
        LocalOracleKey::Unreadable(_) => AttesterStatus::KeyUnreadable,
        LocalOracleKey::Key(_) if !seated => {
            if ever_seated_in_scan {
                AttesterStatus::Removed
            } else {
                AttesterStatus::WaitingToBeSeated
            }
        },
        LocalOracleKey::Key(_) => {
            if recent_attestations > 0 || mempool_attestations > 0 {
                AttesterStatus::Attesting
            } else if attester_polling {
                AttesterStatus::SeatedQuiet
            } else {
                AttesterStatus::SeatedIdle
            }
        },
    }
}

/// What the registration history says about one oracle id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeatHistory {
    /// Seated now: the first block of the current unbroken run of
    /// registrations that include the id. `None` when the run starts before
    /// the blocks read (or at genesis, with no registration at all).
    pub seated_since: Option<u64>,
    /// Not seated now: the registration that last removed it.
    pub removed_at: Option<u64>,
    /// Some registration in the blocks read included the id.
    pub ever_included: bool,
}

/// `registrations`: `(height, includes_id)` for every OracleRegistration in the
/// blocks read, NEWEST FIRST.
pub fn seat_history(registrations: &[(u64, bool)], seated_now: bool) -> SeatHistory {
    let mut h = SeatHistory::default();
    if seated_now {
        for &(height, includes) in registrations {
            if !includes {
                break;
            }
            h.ever_included = true;
            h.seated_since = Some(height);
        }
    } else {
        let mut newer_excluding: Option<u64> = None;
        for &(height, includes) in registrations {
            if includes {
                h.ever_included = true;
                h.removed_at = newer_excluding;
                break;
            }
            newer_excluding = Some(height);
        }
    }
    h
}

/// Count this oracle's attestations in `blocks`; returns (count, newest height).
pub fn count_attestations<'a>(
    blocks: impl IntoIterator<Item = &'a Block>,
    oracle_id: &str,
) -> (u64, Option<u64>) {
    let mut count = 0u64;
    let mut newest: Option<u64> = None;
    for block in blocks {
        for tx in &block.transactions {
            if tx.tx_type != TransactionType::OracleAttestation {
                continue;
            }
            let Some(p) = tx.parse_oracle_attestation_payload() else {
                continue;
            };
            if p.oracle_id.eq_ignore_ascii_case(oracle_id) {
                count += 1;
                newest = Some(newest.map_or(block.height, |n| n.max(block.height)));
            }
        }
    }
    (count, newest)
}

/// `(height, includes_id)` of each OracleRegistration in `block`.
fn registrations_in(block: &Block, oracle_id: &str) -> Vec<(u64, bool)> {
    block
        .transactions
        .iter()
        .filter(|tx| tx.tx_type == TransactionType::OracleRegistration)
        .filter_map(|tx| tx.parse_oracle_registration())
        .map(|p| {
            let includes = p
                .oracle_set
                .iter()
                .any(|s| s.eq_ignore_ascii_case(oracle_id));
            (block.height, includes)
        })
        .collect()
}

/// Read stored blocks backwards from `tip` (at most [`REGISTRATION_SCAN_BLOCKS`])
/// and collect `(height, includes_id)` for each OracleRegistration, newest
/// first, stopping once the seat history is decided. Returns the list and the
/// lowest height read.
fn scan_registrations(
    storage: &wrfcoin_native_chain::storage::Storage,
    oracle_id: &str,
    tip: u64,
    seated: bool,
) -> (Vec<(u64, bool)>, u64) {
    let floor = tip.saturating_sub(REGISTRATION_SCAN_BLOCKS - 1).max(1);
    let mut regs: Vec<(u64, bool)> = Vec::new();
    let mut hi = tip;
    let mut lowest = tip;
    while hi >= floor && hi > 0 {
        let lo = hi.saturating_sub(SCAN_BATCH_BLOCKS - 1).max(floor);
        let Ok(blocks) = storage.load_blocks_range(lo, hi) else {
            break;
        };
        regs.extend(
            blocks
                .iter()
                .rev()
                .flat_map(|b| registrations_in(b, oracle_id).into_iter().rev()),
        );
        lowest = lo;
        // Stop as soon as the answer is known: a seated id's run has a start,
        // or an unseated id was once included.
        let decided = if seated {
            regs.iter().any(|r| !r.1)
        } else {
            seat_history(&regs, false).ever_included
        };
        if decided || lo == floor {
            break;
        }
        hi = lo - 1;
    }
    (regs, lowest)
}

/// The registration scan is cached: it only changes with the key or the set.
#[derive(Clone)]
struct ScanCache {
    oracle_id: String,
    set_fingerprint: Vec<String>,
    history: SeatHistory,
    scanned_from: u64,
}
static SCAN_CACHE: LazyLock<Mutex<Option<ScanCache>>> = LazyLock::new(|| Mutex::new(None));

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeAttesterResponse {
    pub status: AttesterStatus,
    /// Why a configured key cannot be used (`key_unreadable` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_problem: Option<String>,
    /// The oracle id (public key). Public data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oracle_id: Option<String>,
    pub seated: bool,
    pub oracle_set_size: usize,
    pub oracle_quorum: usize,
    pub tip_height: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seated_since_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_at_height: Option<u64>,
    /// Lowest block the registration search read (0 when it did not run).
    pub history_scanned_from: u64,
    pub window_blocks: u64,
    pub attestations_in_window: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attestation_height: Option<u64>,
    pub mempool_attestations: u64,
    /// Seconds since the local attester last polled; `None` = not since start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attester_last_poll_secs_ago: Option<u64>,
    pub attester_polling: bool,
}

/// `GET /node/attester` (Operator).
pub async fn get_node_attester(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let key = local_key_from_env();
    let oracle_id = match &key {
        LocalOracleKey::Key(k) => Some(k.clone()),
        _ => None,
    };

    // Mempool first, blockchain second, never both locks at once.
    let mempool_attestations = match &oracle_id {
        Some(id) => {
            let mempool = state.mempool.read().await;
            mempool
                .get_transactions_by_type(TransactionType::OracleAttestation, 10_000)
                .iter()
                .filter_map(|tx| tx.parse_oracle_attestation_payload())
                .filter(|p| p.oracle_id.eq_ignore_ascii_case(id))
                .count() as u64
        },
        None => 0,
    };

    let (tip, seated, set_size, quorum, mut fingerprint, in_memory) = {
        let chain = state.blockchain.read().await;
        let mut set: Vec<String> = chain
            .state
            .oracle_set
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        set.sort();
        let seated = oracle_id
            .as_ref()
            .is_some_and(|id| set.binary_search(id).is_ok());
        let tip = chain.height;
        let from = tip.saturating_sub(ATTESTATION_WINDOW_BLOCKS - 1);
        // Attestations in the window from the in-memory blocks; heights not in
        // memory are read from storage below.
        let in_memory: Vec<Block> = match &oracle_id {
            Some(_) => chain.blocks_range(from, tip).cloned().collect(),
            None => Vec::new(),
        };
        (
            tip,
            seated,
            set.len(),
            chain.state.oracle_quorum,
            set,
            in_memory,
        )
    };
    fingerprint.push(format!("seated={seated}"));

    let mut recent = (0u64, None);
    let mut history = SeatHistory::default();
    let mut scanned_from = 0u64;
    if let Some(id) = &oracle_id {
        let from = tip.saturating_sub(ATTESTATION_WINDOW_BLOCKS - 1);
        let have: std::collections::BTreeSet<u64> = in_memory.iter().map(|b| b.height).collect();
        let mut window = in_memory;
        if (from..=tip).any(|h| !have.contains(&h)) {
            let storage = state.storage.read().await;
            if let Ok(stored) = storage.load_blocks_range(from, tip) {
                window.extend(stored.into_iter().filter(|b| !have.contains(&b.height)));
            }
        }
        recent = count_attestations(window.iter(), id);

        let mut cache = SCAN_CACHE.lock().await;
        let hit = cache
            .as_ref()
            .filter(|c| &c.oracle_id == id && c.set_fingerprint == fingerprint)
            .cloned();
        let entry = match hit {
            Some(c) => c,
            None => {
                let storage = state.storage.clone().read_owned().await;
                let scan_id = id.clone();
                let (regs, lowest) =
                    web::block(move || scan_registrations(&storage, &scan_id, tip, seated))
                        .await
                        .unwrap_or_default();
                let c = ScanCache {
                    oracle_id: id.clone(),
                    set_fingerprint: fingerprint.clone(),
                    history: seat_history(&regs, seated),
                    scanned_from: lowest,
                };
                *cache = Some(c.clone());
                c
            },
        };
        history = entry.history;
        scanned_from = entry.scanned_from;
    }

    let poll_at = crate::state::ORACLE_POLL_AT.load(Ordering::Relaxed);
    let poll_ago = (poll_at > 0).then(|| crate::state::unix_now_secs().saturating_sub(poll_at));
    let polling = poll_ago.is_some_and(|s| s <= ATTESTER_POLL_FRESH_SECS);
    let status = decide_status(
        &key,
        seated,
        history.ever_included,
        recent.0,
        mempool_attestations,
        polling,
    );

    HttpResponse::Ok().json(NodeAttesterResponse {
        status,
        key_problem: match key {
            LocalOracleKey::Unreadable(why) => Some(why.to_string()),
            _ => None,
        },
        oracle_id,
        seated,
        oracle_set_size: set_size,
        oracle_quorum: quorum,
        tip_height: tip,
        seated_since_height: if seated { history.seated_since } else { None },
        removed_at_height: if seated { None } else { history.removed_at },
        history_scanned_from: scanned_from,
        window_blocks: ATTESTATION_WINDOW_BLOCKS,
        attestations_in_window: recent.0,
        last_attestation_height: recent.1,
        mempool_attestations,
        attester_last_poll_secs_ago: poll_ago,
        attester_polling: polling,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: &str = "c4dbd69bc84a4651db0a03ccc142a019c0856126ac3c920e361cba2269034725"; // pragma: allowlist secret -- public oracle id fixture

    fn key() -> LocalOracleKey {
        LocalOracleKey::Key(K.to_string())
    }

    #[test]
    fn no_key_keeps_the_rung_below() {
        assert_eq!(
            decide_status(&LocalOracleKey::None, false, false, 0, 0, false),
            AttesterStatus::NoKey
        );
        // Even a seated set or a poll cannot make a node without a key an attester.
        assert_eq!(
            decide_status(&LocalOracleKey::None, true, true, 5, 1, true),
            AttesterStatus::NoKey
        );
    }

    #[test]
    fn unreadable_key_is_reported_not_guessed() {
        let k = LocalOracleKey::Unreadable("x");
        assert_eq!(
            decide_status(&k, false, false, 0, 0, false),
            AttesterStatus::KeyUnreadable
        );
    }

    #[test]
    fn key_not_in_the_set_waits_to_be_seated() {
        assert_eq!(
            decide_status(&key(), false, false, 0, 0, true),
            AttesterStatus::WaitingToBeSeated
        );
    }

    #[test]
    fn key_removed_from_the_set_goes_back_down() {
        assert_eq!(
            decide_status(&key(), false, true, 0, 0, true),
            AttesterStatus::Removed
        );
        // Old attestations in the window do not keep a removed oracle seated.
        assert_eq!(
            decide_status(&key(), false, true, 3, 0, true),
            AttesterStatus::Removed
        );
    }

    #[test]
    fn seated_with_recent_attestations_is_attesting() {
        assert_eq!(
            decide_status(&key(), true, true, 4, 0, false),
            AttesterStatus::Attesting
        );
        assert_eq!(
            decide_status(&key(), true, false, 0, 1, false),
            AttesterStatus::Attesting
        );
    }

    #[test]
    fn seated_without_attestations_warns_only_when_the_attester_is_not_polling() {
        assert_eq!(
            decide_status(&key(), true, true, 0, 0, true),
            AttesterStatus::SeatedQuiet
        );
        assert_eq!(
            decide_status(&key(), true, true, 0, 0, false),
            AttesterStatus::SeatedIdle
        );
    }

    #[test]
    fn status_strings_are_what_the_page_reads() {
        for (s, text) in [
            (AttesterStatus::NoKey, "no_key"),
            (AttesterStatus::KeyUnreadable, "key_unreadable"),
            (AttesterStatus::WaitingToBeSeated, "waiting_to_be_seated"),
            (AttesterStatus::Removed, "removed"),
            (AttesterStatus::Attesting, "attesting"),
            (AttesterStatus::SeatedQuiet, "seated_quiet"),
            (AttesterStatus::SeatedIdle, "seated_idle"),
        ] {
            assert_eq!(serde_json::to_value(s).unwrap(), serde_json::json!(text));
            let page = super::super::status_page::STATUS_PAGE_HTML;
            assert!(
                page.contains(&format!("'{text}'")),
                "the dashboard must handle {text}"
            );
        }
    }

    #[test]
    fn seat_history_finds_the_start_of_the_current_run() {
        // Newest first: seated at 28015 (epoch 3 batch), still in a later rekey.
        let regs = [(28100, true), (28015, true), (27000, false), (26000, true)];
        let h = seat_history(&regs, true);
        assert_eq!(h.seated_since, Some(28015));
        assert!(h.ever_included);
        assert_eq!(h.removed_at, None);
        // No registration read: seated from genesis or before the scan.
        assert_eq!(seat_history(&[], true).seated_since, None);
    }

    #[test]
    fn seat_history_finds_the_removal() {
        let regs = [(29000, false), (28500, false), (28015, true)];
        let h = seat_history(&regs, false);
        assert_eq!(h.removed_at, Some(28500));
        assert!(h.ever_included);
        let never = seat_history(&[(28015, false)], false);
        assert!(!never.ever_included);
        assert_eq!(never.removed_at, None);
    }

    #[test]
    fn public_key_text_is_strict() {
        assert_eq!(
            parse_public_key_text(&format!("{}\r\n", K.to_uppercase())),
            Some(K.to_string())
        );
        assert_eq!(
            parse_public_key_text(&format!("\u{feff}{K}\n")),
            Some(K.to_string())
        );
        assert_eq!(parse_public_key_text(&K[..63]), None);
        assert_eq!(parse_public_key_text(&format!("{K}00")), None);
        assert_eq!(parse_public_key_text("zz"), None);
        assert_eq!(parse_public_key_text(""), None);
    }

    #[test]
    fn local_key_sources() {
        assert_eq!(resolve_local_key(None, None), LocalOracleKey::None);
        assert_eq!(resolve_local_key(Some(K.into()), None), key());
        assert!(matches!(
            resolve_local_key(Some("nope".into()), None),
            LocalOracleKey::Unreadable(_)
        ));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oracle-public.txt");
        let p = path.to_string_lossy().to_string();
        // The launcher names the file before the key exists: no key yet.
        assert_eq!(
            resolve_local_key(None, Some(p.clone())),
            LocalOracleKey::None
        );
        std::fs::write(&path, format!("{K}\r\n")).unwrap();
        assert_eq!(resolve_local_key(None, Some(p.clone())), key());
        std::fs::write(&path, "not a key").unwrap();
        assert!(matches!(
            resolve_local_key(None, Some(p.clone())),
            LocalOracleKey::Unreadable(_)
        ));
        std::fs::write(&path, "a".repeat(4096)).unwrap();
        assert!(matches!(
            resolve_local_key(None, Some(p)),
            LocalOracleKey::Unreadable(_)
        ));
    }
}
