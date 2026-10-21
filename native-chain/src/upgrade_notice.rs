//! Node-side consumption of governance-signed upgrade notices (#3095).
//!
//! #3094 defines the artifact and its signature rule; #3093 defines the clean
//! halt. This module is the node's half: it holds the one notice this process
//! trusts, decides what the operator is told during the run-up, answers
//! `GET /network/upgrade`, and hands the activation height to #3093's
//! [`UpgradeDeadline`] so a node whose *genesis* schedule predates the feature
//! still stops at `H − 1` instead of forking silently.
//!
//! **Transport is untrusted by construction.** A notice arrives from a file
//! path or a peer's HTTP API — neither is authenticated — so nothing is stored
//! until `verify_upgrade_notice` has passed it against this node's chain
//! identity and the governance allowlist it already trusts
//! (`WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS`). A notice that fails for any reason —
//! tampered, foreign chain, expired, unsigned, unknown signer — is logged once
//! and dropped, and the node behaves exactly as if it had never arrived. That
//! is the property `upgrade_notice_runtime.rs` pins.
//!
//! Process-wide, like [`crate::upgrade`]'s deadline and the activation table:
//! which release this chain is moving to is a property of the chain, not of a
//! request, and the route, the run-up logger and the deadline must all see the
//! same answer. Replaceable (not `OnceLock`) because a later notice with a
//! higher activation height supersedes an earlier one, and because tests
//! install and clear it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::collections::VecDeque;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use wrfcoin_shared_protocol::crypto::protocol_support::PROTOCOL_VERSION;
use wrfcoin_shared_protocol::governance::upgrade_notice::{
    verify_upgrade_notice, SignedUpgradeNotice, UpgradeNotice, UpgradeNoticeError,
};

use crate::upgrade::{install_upgrade_deadline, upgrade_deadline, UpgradeDeadline};

/// Optional path to a signed notice file, read once at boot.
pub const NOTICE_FILE_ENV: &str = "WRFCOIN_UPGRADE_NOTICE_FILE";
/// `1` enables the periodic peer fetch (opt-in; see [`PEER_URLS_ENV`]).
pub const PEER_FETCH_ENV: &str = "WRFCOIN_UPGRADE_NOTICE_PEER_FETCH";
/// Comma-separated peer node API base URLs polled when the fetch is enabled.
pub const PEER_URLS_ENV: &str = "WRFCOIN_UPGRADE_NOTICE_PEER_URLS";

/// How far ahead of the activation the run-up warnings begin.
pub const RUN_UP_BLOCKS: u64 = 2_000;
/// Cadence of the WARN-level run-up line, in blocks.
pub const RUN_UP_INTERVAL: u64 = 50;
/// Within this many blocks of the activation every block logs at ERROR.
pub const URGENT_BLOCKS: u64 = 100;

/// What `GET /network/upgrade` reports, and what the attester keys off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeStatus {
    /// No verified notice is installed. The node is unconstrained by this
    /// module; the genesis schedule may still hold a #3093 deadline.
    None,
    /// A notice is installed and this binary already carries the release
    /// (`PROTOCOL_VERSION >= min_protocol_version`). Nothing to do.
    Ready,
    /// A notice is installed, this binary is older than the release it names,
    /// and the tip has not reached the activation yet.
    UpgradeRequired,
    /// As [`Self::UpgradeRequired`], but the tip is at or beyond the
    /// activation height — this binary must no longer produce, apply, relay
    /// or attest.
    PastDeadline,
}

impl UpgradeStatus {
    /// The stable wire spelling used by the route and read by the attester.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Ready => "ready",
            Self::UpgradeRequired => "upgrade_required",
            Self::PastDeadline => "past_deadline",
        }
    }

    /// Whether this binary must refuse consensus work at or beyond the
    /// activation height. Both non-`Ready` notice states qualify: the
    /// attester asks this before signing for a height.
    pub fn requires_upgrade(self) -> bool {
        matches!(self, Self::UpgradeRequired | Self::PastDeadline)
    }
}

impl std::fmt::Display for UpgradeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Log severity the run-up cadence asks for at a given height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunUpLevel {
    /// Once: this binary already satisfies the notice.
    Satisfied,
    /// Every [`RUN_UP_INTERVAL`] blocks inside the run-up window.
    Warn,
    /// Every block inside [`URGENT_BLOCKS`], and at/after the activation.
    Error,
}

static NOTICE: RwLock<Option<SignedUpgradeNotice>> = RwLock::new(None);
static SATISFIED_LOGGED: AtomicBool = AtomicBool::new(false);
/// Last height the run-up logger emitted a line for, so a poll that sees the
/// same tip twice does not repeat itself. `u64::MAX` = nothing logged yet.
static LAST_LOGGED_HEIGHT: AtomicU64 = AtomicU64::new(u64::MAX);

const REJECTION_LOG_INTERVAL: Duration = Duration::from_secs(600);
const REJECTION_LOG_CAPACITY: usize = 64;
static REJECTION_LOGS: Mutex<VecDeque<([u8; 32], Instant)>> = Mutex::new(VecDeque::new());

/// Bounded, process-local warning history. Verification never consults it.
/// Include the complete signed candidate and refusal reason, so a new signer,
/// changed notice/signature, or changed verification failure logs immediately.
fn log_rejection(candidate: &SignedUpgradeNotice, error: &UpgradeNoticeError) -> bool {
    let Ok(bytes) = serde_json::to_vec(candidate) else {
        return true;
    };
    let mut digest = Sha256::new();
    digest.update(bytes);
    // Expiry's diagnostic includes the polling time; it is still the same
    // refusal on each poll and must not defeat deduplication.
    let reason = match error {
        UpgradeNoticeError::Expired { expires_at, .. } => format!("expired at {expires_at}"),
        _ => error.to_string(),
    };
    digest.update(reason.as_bytes());
    let key = digest.finalize().into();
    // determinism-ok: process-local log throttling only; verification and consensus state never depend on this clock
    let now = Instant::now();
    REJECTION_LOGS
        .lock()
        .map(|mut history| rejection_log_due(&mut history, key, now))
        .unwrap_or(true)
}

fn rejection_log_due(
    history: &mut VecDeque<([u8; 32], Instant)>,
    key: [u8; 32],
    now: Instant,
) -> bool {
    if let Some(index) = history.iter().position(|(held, _)| *held == key) {
        if now.duration_since(history[index].1) < REJECTION_LOG_INTERVAL {
            return false;
        }
        history.remove(index);
    }
    if history.len() == REJECTION_LOG_CAPACITY {
        history.pop_front();
    }
    history.push_back((key, now));
    true
}

/// Install (or clear) the process-wide notice without verifying it. Callers
/// that received the notice over a transport must use
/// [`accept_upgrade_notice`] instead; this is the boot/test installer.
pub fn install_upgrade_notice(signed: Option<SignedUpgradeNotice>) {
    if let Ok(mut slot) = NOTICE.write() {
        *slot = signed;
    }
}

/// The installed notice, if any.
pub fn upgrade_notice() -> Option<SignedUpgradeNotice> {
    NOTICE.read().ok().and_then(|slot| slot.clone())
}

/// Blocks between `tip_height` and the activation. Saturates at zero, so a tip
/// past the activation reports `0` rather than wrapping.
pub fn blocks_remaining(notice: &UpgradeNotice, tip_height: u64) -> u64 {
    notice.activation_height.saturating_sub(tip_height)
}

/// The status of an explicit notice — pure, so the route, the run-up logger
/// and the tests all read the same rule.
pub fn status_of(
    notice: Option<&UpgradeNotice>,
    tip_height: u64,
    local_protocol_version: u32,
) -> UpgradeStatus {
    match notice {
        None => UpgradeStatus::None,
        Some(notice) if local_protocol_version >= notice.min_protocol_version => {
            UpgradeStatus::Ready
        },
        Some(notice) if tip_height >= notice.activation_height => UpgradeStatus::PastDeadline,
        Some(_) => UpgradeStatus::UpgradeRequired,
    }
}

/// [`status_of`] over the installed notice and this binary's version.
pub fn upgrade_status(tip_height: u64) -> UpgradeStatus {
    let notice = upgrade_notice();
    status_of(
        notice.as_ref().map(|s| &s.notice),
        tip_height,
        PROTOCOL_VERSION,
    )
}

/// The one operator-facing run-up line, exactly as #3095 specifies it.
pub fn run_up_message(notice: &UpgradeNotice, tip_height: u64, local_protocol_version: u32) -> String {
    format!(
        "UPGRADE REQUIRED by height {} (in {} blocks): running protocol v{}, release {} needs v{} \
         — see /network/upgrade",
        notice.activation_height,
        blocks_remaining(notice, tip_height),
        local_protocol_version,
        notice.release_commit,
        notice.min_protocol_version
    )
}

/// The cadence rule, pure and total: what severity (if any) height
/// `tip_height` deserves. `None` means "silent at this height".
///
/// - Satisfied binary → [`RunUpLevel::Satisfied`] (the caller emits it once).
/// - At or past the activation → ERROR, every block.
/// - Within [`URGENT_BLOCKS`] → ERROR, every block.
/// - Within [`RUN_UP_BLOCKS`] → WARN, every [`RUN_UP_INTERVAL`] blocks.
/// - Further out → silent.
pub fn run_up_level(
    notice: &UpgradeNotice,
    tip_height: u64,
    local_protocol_version: u32,
) -> Option<RunUpLevel> {
    if local_protocol_version >= notice.min_protocol_version {
        return Some(RunUpLevel::Satisfied);
    }
    match run_up_severity(notice.activation_height, tip_height)? {
        RunUpLevel::Warn if tip_height % RUN_UP_INTERVAL != 0 => None,
        level => Some(level),
    }
}

/// The run-up severity of an activation this binary cannot follow, with no
/// logging cadence applied: ERROR at or past the activation and within
/// [`URGENT_BLOCKS`], WARN within [`RUN_UP_BLOCKS`], `None` further out.
///
/// One rule for every source of an unfollowable activation — a signed
/// notice ([`run_up_level`] adds the log cadence on top) and a pending entry
/// of the effective feature schedule (`GET /network/upgrade`'s `scheduled`
/// view, #3277 S6) — so the two can never disagree about the window.
pub fn run_up_severity(activation_height: u64, tip_height: u64) -> Option<RunUpLevel> {
    let remaining = activation_height.saturating_sub(tip_height);
    if tip_height >= activation_height || remaining <= URGENT_BLOCKS {
        return Some(RunUpLevel::Error);
    }
    if remaining <= RUN_UP_BLOCKS {
        return Some(RunUpLevel::Warn);
    }
    None
}

/// Emit the run-up line for `tip_height` against the installed notice, at the
/// cadence [`run_up_level`] dictates. Idempotent per height, so a poller that
/// sees an unchanged tip stays quiet, and the "satisfied" line is emitted at
/// most once per process. Returns what was logged, for the tests.
pub fn log_run_up(tip_height: u64) -> Option<(RunUpLevel, String)> {
    let signed = upgrade_notice()?;
    let notice = &signed.notice;
    let level = run_up_level(notice, tip_height, PROTOCOL_VERSION)?;

    if level == RunUpLevel::Satisfied {
        if SATISFIED_LOGGED.swap(true, Ordering::SeqCst) {
            return None;
        }
        let message = format!(
            "upgrade notice satisfied: notice {} activates at height {} needing protocol v{}, \
             this binary is v{} — no upgrade action required",
            notice.notice_id,
            notice.activation_height,
            notice.min_protocol_version,
            PROTOCOL_VERSION
        );
        log::info!("{message}");
        return Some((level, message));
    }

    // One line per height, never one per poll.
    if LAST_LOGGED_HEIGHT.swap(tip_height, Ordering::SeqCst) == tip_height {
        return None;
    }
    let message = run_up_message(notice, tip_height, PROTOCOL_VERSION);
    match level {
        RunUpLevel::Error => log::error!("{message}"),
        RunUpLevel::Warn => log::warn!("{message}"),
        RunUpLevel::Satisfied => unreachable!("handled above"),
    }
    Some((level, message))
}

/// The #3093 deadline a notice implies for this binary: the activation height,
/// when the binary is older than the release the notice names. A binary that
/// already carries the release gets no deadline from it.
pub fn deadline_from_notice(
    notice: &UpgradeNotice,
    local_protocol_version: u32,
) -> Option<UpgradeDeadline> {
    if local_protocol_version >= notice.min_protocol_version {
        return None;
    }
    let feature = notice
        .features
        .first()
        .cloned()
        .unwrap_or_else(|| format!("protocol_v{}", notice.min_protocol_version));
    Some(UpgradeDeadline {
        feature,
        height: notice.activation_height,
        running_protocol_version: local_protocol_version,
    })
}

/// Hand the installed notice's activation to #3093, keeping the EARLIER of the
/// two deadlines.
///
/// The genesis schedule and a notice are independent sources for the same
/// question — "what is the first height this binary must not touch?" — and the
/// safe composition of two answers is the lower one: stopping early costs an
/// operator a restart, while stopping late is the silent fork #1972 was. A
/// notice can therefore only ever *tighten* the deadline, never relax one the
/// genesis schedule already installed.
pub fn sync_deadline_from_notice() {
    let Some(signed) = upgrade_notice() else {
        return;
    };
    let Some(from_notice) = deadline_from_notice(&signed.notice, PROTOCOL_VERSION) else {
        return;
    };
    match upgrade_deadline() {
        Some(existing) if existing.height <= from_notice.height => {},
        Some(existing) => {
            log::warn!(
                "Upgrade notice {} tightens the clean-halt deadline from height {} (feature {:?}) \
                 to height {} (feature {:?}) (#3095)",
                signed.notice.notice_id,
                existing.height,
                existing.feature,
                from_notice.height,
                from_notice.feature
            );
            install_upgrade_deadline(Some(from_notice));
        },
        None => {
            log::warn!(
                "Upgrade notice {} installs a clean-halt deadline at height {}: this binary is \
                 protocol v{} and the announced release {} carries v{}. This node will stop \
                 cleanly at height {} (#3093/#3095)",
                signed.notice.notice_id,
                from_notice.height,
                PROTOCOL_VERSION,
                signed.notice.release_commit,
                signed.notice.min_protocol_version,
                from_notice.height.saturating_sub(1)
            );
            install_upgrade_deadline(Some(from_notice));
        },
    }
}

/// Why a candidate did not become the installed notice. Not an error — a
/// well-formed notice that simply does not supersede the one already held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeOutcome {
    /// Verified and installed (first notice, or a higher activation height).
    Installed,
    /// Verified but not installed: the held notice activates at or above it.
    NotSuperseding,
}

/// Verify a candidate notice and install it if it supersedes the held one.
///
/// This is the ONLY door a transport-delivered notice may enter by. Every
/// candidate is checked against this node's chain identity, the governance
/// allowlist and `now` before it can change anything. Rejections leave the node
/// exactly as it was; identical candidates/reasons warn at most once per ten
/// minutes while retained in the bounded process-local log history.
///
/// Highest `activation_height` wins, so a later announcement supersedes an
/// earlier one and a replayed older notice cannot walk the deadline backwards.
pub fn accept_upgrade_notice(
    candidate: SignedUpgradeNotice,
    chain_id: &str,
    network_id: &str,
    allowlist: &[String],
    now: i64,
    source: &str,
) -> Result<NoticeOutcome, UpgradeNoticeError> {
    if let Err(e) = verify_upgrade_notice(&candidate, chain_id, network_id, allowlist, now) {
        if log_rejection(&candidate, &e) {
            log::warn!(
                "Ignoring upgrade notice from {source}: {e} (#3095) — node behaviour is unchanged"
            );
        }
        return Err(e);
    }

    let superseding = match upgrade_notice() {
        Some(held) => candidate.notice.activation_height > held.notice.activation_height,
        None => true,
    };
    if !superseding {
        log::debug!(
            "Upgrade notice {} from {source} verified but does not supersede the held notice",
            candidate.notice.notice_id
        );
        return Ok(NoticeOutcome::NotSuperseding);
    }

    log::info!(
        "Accepted governance-signed upgrade notice {} from {source}: activates at height {}, \
         needs protocol v{} (this binary is v{}), release {}, features {:?} (#3095)",
        candidate.notice.notice_id,
        candidate.notice.activation_height,
        candidate.notice.min_protocol_version,
        PROTOCOL_VERSION,
        candidate.notice.release_commit,
        candidate.notice.features
    );
    install_upgrade_notice(Some(candidate));
    SATISFIED_LOGGED.store(false, Ordering::SeqCst);
    sync_deadline_from_notice();
    Ok(NoticeOutcome::Installed)
}

/// Parse a signed notice from JSON bytes. A malformed file is a refusal, never
/// a panic and never a partial install.
pub fn parse_notice_json(bytes: &[u8]) -> Result<SignedUpgradeNotice, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("not a signed upgrade notice: {e}"))
}

/// Read and parse the notice file at `path`.
pub fn read_notice_file(path: &std::path::Path) -> Result<SignedUpgradeNotice, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse_notice_json(&bytes)
}

/// The peer node API base URLs to poll, from [`PEER_URLS_ENV`]. Empty unless
/// the operator listed them: the node has no HTTP address for its libp2p
/// peers, so the fetch targets are configuration, not discovery.
pub fn configured_peer_urls() -> Vec<String> {
    // determinism-ok: operator configuration for an out-of-band notice transport; nothing here reaches consensus state
    std::env::var(PEER_URLS_ENV)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().trim_end_matches('/').to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the operator asked for the periodic peer fetch.
pub fn peer_fetch_enabled() -> bool {
    // determinism-ok: operator configuration, as above
    std::env::var(PEER_FETCH_ENV)
        .map(|raw| raw.trim() == "1" || raw.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Test hook: clear the notice and the once-only log flags.
pub fn reset_for_tests() {
    install_upgrade_notice(None);
    SATISFIED_LOGGED.store(false, Ordering::SeqCst);
    LAST_LOGGED_HEIGHT.store(u64::MAX, Ordering::SeqCst);
    if let Ok(mut history) = REJECTION_LOGS.lock() {
        history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn rejection_warning_cadence_and_history_are_bounded() {
        let mut history = VecDeque::new();
        let start = Instant::now();
        assert!(rejection_log_due(&mut history, [0; 32], start));
        for seconds in [5, 10, 300, 599] {
            assert!(!rejection_log_due(
                &mut history,
                [0; 32],
                start + Duration::from_secs(seconds)
            ));
        }
        assert!(rejection_log_due(&mut history, [1; 32], start));
        assert!(rejection_log_due(
            &mut history,
            [0; 32],
            start + REJECTION_LOG_INTERVAL
        ));
        for key in 2..=REJECTION_LOG_CAPACITY as u8 {
            assert!(rejection_log_due(
                &mut history,
                [key; 32],
                start + REJECTION_LOG_INTERVAL
            ));
        }
        assert_eq!(history.len(), REJECTION_LOG_CAPACITY);
        // Evicted entries warn again rather than hiding a refusal forever.
        assert!(rejection_log_due(
            &mut history,
            [1; 32],
            start + REJECTION_LOG_INTERVAL
        ));
        assert_eq!(history.len(), REJECTION_LOG_CAPACITY);
    }

    /// A notice is pure data here — these tests exercise the cadence and
    /// status rules, which never look at the signatures. Signature-gated
    /// behaviour lives in `tests/upgrade_notice_runtime.rs`, where real
    /// governance keys sign real notices.
    fn notice(activation_height: u64, min_protocol_version: u32) -> UpgradeNotice {
        UpgradeNotice {
            notice_id: UpgradeNotice::notice_id_for(activation_height, "abcdef0123456789"),
            chain_id: "wrfcoin-testnet".into(),
            network_id: "testnet".into(),
            activation_height,
            min_protocol_version,
            features: vec!["future_feature".into()],
            release_commit: "abcdef0123456789".into(),
            image_digests: BTreeMap::new(),
            issued_at: 1_700_000_000,
            expires_at: None,
        }
    }

    #[test]
    fn status_names_the_four_states() {
        assert_eq!(status_of(None, 0, 2), UpgradeStatus::None);
        // This binary already carries the release.
        assert_eq!(status_of(Some(&notice(100, 2)), 0, 2), UpgradeStatus::Ready);
        assert_eq!(status_of(Some(&notice(100, 2)), 0, 3), UpgradeStatus::Ready);
        // Older binary, tip short of the activation.
        assert_eq!(
            status_of(Some(&notice(100, 3)), 99, 2),
            UpgradeStatus::UpgradeRequired
        );
        // Older binary, tip at/past the activation.
        assert_eq!(
            status_of(Some(&notice(100, 3)), 100, 2),
            UpgradeStatus::PastDeadline
        );
        assert_eq!(
            status_of(Some(&notice(100, 3)), u64::MAX, 2),
            UpgradeStatus::PastDeadline
        );
        assert!(!UpgradeStatus::None.requires_upgrade());
        assert!(!UpgradeStatus::Ready.requires_upgrade());
        assert!(UpgradeStatus::UpgradeRequired.requires_upgrade());
        assert!(UpgradeStatus::PastDeadline.requires_upgrade());
        assert_eq!(UpgradeStatus::PastDeadline.as_str(), "past_deadline");
    }

    #[test]
    fn blocks_remaining_saturates_past_the_activation() {
        assert_eq!(blocks_remaining(&notice(100, 3), 0), 100);
        assert_eq!(blocks_remaining(&notice(100, 3), 99), 1);
        assert_eq!(blocks_remaining(&notice(100, 3), 100), 0);
        assert_eq!(blocks_remaining(&notice(100, 3), 10_000), 0);
    }

    #[test]
    fn the_run_up_cadence_is_silent_far_out_warns_every_fifty_and_errors_inside_a_hundred() {
        let n = notice(10_000, 3);
        // Outside the run-up window: silent even on a multiple of 50.
        assert_eq!(run_up_level(&n, 7_000, 2), None, "8000 blocks out");
        assert_eq!(run_up_level(&n, 7_999, 2), None);
        // Inside the 2000-block window: every 50th height warns, others silent.
        assert_eq!(run_up_level(&n, 8_000, 2), Some(RunUpLevel::Warn));
        assert_eq!(run_up_level(&n, 8_001, 2), None);
        assert_eq!(run_up_level(&n, 8_049, 2), None);
        assert_eq!(run_up_level(&n, 8_050, 2), Some(RunUpLevel::Warn));
        assert_eq!(run_up_level(&n, 9_850, 2), Some(RunUpLevel::Warn));
        // The last 100 blocks: every block, at ERROR.
        assert_eq!(run_up_level(&n, 9_900, 2), Some(RunUpLevel::Error));
        assert_eq!(run_up_level(&n, 9_901, 2), Some(RunUpLevel::Error));
        assert_eq!(run_up_level(&n, 9_999, 2), Some(RunUpLevel::Error));
        // At and past the activation: still every block, at ERROR.
        assert_eq!(run_up_level(&n, 10_000, 2), Some(RunUpLevel::Error));
        assert_eq!(run_up_level(&n, 10_500, 2), Some(RunUpLevel::Error));
        // A binary that carries the release is never warned, at any height.
        for height in [0, 8_000, 9_999, 10_000, 10_500] {
            assert_eq!(
                run_up_level(&n, height, 3),
                Some(RunUpLevel::Satisfied),
                "height {height}"
            );
        }
    }

    #[test]
    fn the_run_up_message_names_height_blocks_versions_and_the_release() {
        let message = run_up_message(&notice(10_000, 3), 9_950, 2);
        assert!(message.contains("UPGRADE REQUIRED by height 10000"), "{message}");
        assert!(message.contains("(in 50 blocks)"), "{message}");
        assert!(message.contains("running protocol v2"), "{message}");
        assert!(message.contains("needs v3"), "{message}");
        assert!(message.contains("abcdef0123456789"), "{message}");
        assert!(message.contains("/network/upgrade"), "{message}");
    }

    #[test]
    fn a_notice_yields_a_deadline_only_for_an_older_binary() {
        assert_eq!(deadline_from_notice(&notice(10_000, 2), 2), None, "satisfied");
        assert_eq!(deadline_from_notice(&notice(10_000, 2), 3), None, "newer");
        let deadline = deadline_from_notice(&notice(10_000, 3), 2).expect("older binary");
        assert_eq!(deadline.height, 10_000);
        assert_eq!(deadline.feature, "future_feature");
        assert_eq!(deadline.running_protocol_version, 2);
        // With no feature names the deadline still names something actionable.
        let mut featureless = notice(10_000, 3);
        featureless.features.clear();
        assert_eq!(
            deadline_from_notice(&featureless, 2).expect("older binary").feature,
            "protocol_v3"
        );
    }

    #[test]
    fn a_malformed_notice_file_is_a_refusal_not_a_panic() {
        assert!(parse_notice_json(b"").is_err());
        assert!(parse_notice_json(b"{}").is_err());
        assert!(parse_notice_json(b"not json at all").is_err());
        assert!(read_notice_file(std::path::Path::new("/nonexistent/notice.json")).is_err());
    }
}
