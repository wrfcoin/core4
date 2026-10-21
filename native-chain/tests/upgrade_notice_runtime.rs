//! #3095 — the node consumes a governance-signed upgrade notice.
//!
//! The property under test is a negative one, and it is the reason the story
//! exists: **a notice changes node behaviour only when its governance
//! signatures verify.** Transport is a file path and a peer's HTTP response,
//! neither authenticated, so every path below feeds the real
//! `accept_upgrade_notice` door and asserts what `GET /network/upgrade` — the
//! real route handler, on the real `AppState` — reports afterwards.
//!
//! Covered here: a valid notice is reported and drives the run-up cadence and
//! the #3093 deadline handoff; a tampered, foreign-chain, expired, unsigned or
//! wrong-signer notice is inert; a later notice supersedes an earlier one and a
//! replayed earlier one does not walk the deadline backwards.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[cfg(feature = "p2p")]
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/peer_reputation.rs"]
mod peer_reputation;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/network.rs"]
mod network;
// `network.rs` resolves the client IP through `crate::rate_limit` (#2136), so
// the module must exist in this target too.
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;

include!("support/launch_api_test_support.inc.rs");

use std::collections::BTreeMap;

use actix_web::{App, http::StatusCode, test, web};
use wrfcoin_native_chain::upgrade::{reset_for_tests as reset_deadline, upgrade_deadline};
use wrfcoin_native_chain::upgrade_notice::{
    NoticeOutcome, RunUpLevel, accept_upgrade_notice, log_run_up, parse_notice_json,
    read_notice_file, reset_for_tests as reset_notice, upgrade_notice,
};
use wrfcoin_shared_protocol::crypto::protocol_support::PROTOCOL_VERSION;
use wrfcoin_shared_protocol::governance::upgrade_notice::{
    GovernanceSignature, SignedUpgradeNotice, UpgradeNotice, UpgradeNoticeError,
    sign_upgrade_notice_with_key,
};

const CHAIN_ID: &str = "wrfcoin-testnet";
const NETWORK_ID: &str = "testnet";
const RELEASE_COMMIT: &str = "8d29ea85b0123456789abcdef0123456789abcde";  // pragma: allowlist secret -- fixture release commit SHA matching the docs example; a git SHA, not key material
/// A time inside every fixture's validity window.
const NOW: i64 = 1_760_000_000;

/// Both the notice store and the #3093 deadline are process-wide, so these
/// tests take turns — the same discipline `upgrade_required_halt.rs` uses.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct RejectionLogger(std::sync::atomic::AtomicUsize);
static REJECTION_LOGGER: RejectionLogger = RejectionLogger(std::sync::atomic::AtomicUsize::new(0));

impl log::Log for RejectionLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() == log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata())
            && record
                .args()
                .to_string()
                .contains("Ignoring upgrade notice from dedupe-fixture:")
        {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn flush(&self) {}
}

#[actix_web::test]
async fn repeated_refusals_warn_once_and_still_verify_every_candidate() {
    let _guard = SERIAL.lock().unwrap();
    reset_notice();
    reset_deadline();
    log::set_logger(&REJECTION_LOGGER).expect("test logger installed once");
    log::set_max_level(log::LevelFilter::Warn);
    REJECTION_LOGGER
        .0
        .store(0, std::sync::atomic::Ordering::SeqCst);
    let candidate = sign(future_notice(20_000));
    let reject = |candidate| {
        accept_upgrade_notice(candidate, CHAIN_ID, NETWORK_ID, &[], NOW, "dedupe-fixture")
    };
    for _ in 0..128 {
        assert!(matches!(
            reject(candidate.clone()),
            Err(UpgradeNoticeError::SignerNotAuthorized { .. })
        ));
        assert!(upgrade_notice().is_none());
        assert!(upgrade_deadline().is_none());
    }
    assert_eq!(
        REJECTION_LOGGER.0.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(network_upgrade(20_000).await["status"], "none");

    // Same notice ID, changed signed content: a distinct warning immediately.
    let mut changed = future_notice(20_000);
    changed.issued_at -= 1;
    assert!(reject(sign(changed)).is_err());
    // Same notice, different signer: also a distinct warning.
    let (other_key, _) = keypair(0x3096);
    let other = SignedUpgradeNotice {
        notice: candidate.notice.clone(),
        signatures: vec![sign_upgrade_notice_with_key(&candidate.notice, &other_key)],
    };
    assert!(reject(other).is_err());
    assert_eq!(
        REJECTION_LOGGER.0.load(std::sync::atomic::Ordering::SeqCst),
        3
    );

    // Advancing poll time in an expiry diagnostic is still the same refusal.
    for now in [NOW + 31 * 24 * 3_600, NOW + 32 * 24 * 3_600] {
        assert!(matches!(
            accept_upgrade_notice(
                candidate.clone(), CHAIN_ID, NETWORK_ID, &allowlist(), now, "dedupe-fixture"
            ),
            Err(UpgradeNoticeError::Expired { .. })
        ));
    }
    assert_eq!(REJECTION_LOGGER.0.load(std::sync::atomic::Ordering::SeqCst), 4);

    // Cached rejection cannot prevent acceptance when governance authorizes it.
    assert_eq!(offer(candidate.clone()), Ok(NoticeOutcome::Installed));
    let held_deadline = upgrade_deadline();
    assert!(reject(candidate.clone()).is_err());
    assert_eq!(upgrade_notice(), Some(candidate));
    assert_eq!(upgrade_deadline(), held_deadline);
    assert_eq!(
        REJECTION_LOGGER.0.load(std::sync::atomic::Ordering::SeqCst),
        4
    );
    reset_notice();
    reset_deadline();
}

/// A notice needing a protocol version this binary does not have, so every
/// test exercises the interesting branch rather than the `ready` shortcut.
fn future_notice(activation_height: u64) -> UpgradeNotice {
    UpgradeNotice {
        notice_id: UpgradeNotice::notice_id_for(activation_height, RELEASE_COMMIT),
        chain_id: CHAIN_ID.into(),
        network_id: NETWORK_ID.into(),
        activation_height,
        min_protocol_version: PROTOCOL_VERSION + 1,
        features: vec!["future_feature".into()],
        release_commit: RELEASE_COMMIT.into(),
        image_digests: BTreeMap::from([(
            "native-chain-amd64".to_string(),
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        )]),
        issued_at: NOW - 3_600,
        expires_at: Some(NOW + 30 * 24 * 3_600),
    }
}

/// The governance key the allowlist below authorizes. Deterministic, so a
/// failure is reproducible.
fn governance_key() -> (PrivateKey, String) {
    keypair(0x3095)
}

fn allowlist() -> Vec<String> {
    vec![governance_key().1.to_ascii_lowercase()]
}

fn sign(notice: UpgradeNotice) -> SignedUpgradeNotice {
    let (key, _) = governance_key();
    let signature = sign_upgrade_notice_with_key(&notice, &key);
    SignedUpgradeNotice {
        notice,
        signatures: vec![signature],
    }
}

/// Offer a candidate through the real door.
fn offer(candidate: SignedUpgradeNotice) -> Result<NoticeOutcome, UpgradeNoticeError> {
    accept_upgrade_notice(
        candidate,
        CHAIN_ID,
        NETWORK_ID,
        &allowlist(),
        NOW,
        "test-fixture",
    )
}

/// Drive the real route handler and return its JSON body.
async fn network_upgrade(tip_height: u64) -> serde_json::Value {
    let state = test_state(None);
    state.blockchain.write().await.height = tip_height;
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .route("/network/upgrade", web::get().to(network::get_network_upgrade)),
    )
    .await;
    let req = test::TestRequest::get().uri("/network/upgrade").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK, "the route is unauthenticated");
    test::read_body_json(resp).await
}

fn clean() {
    reset_notice();
    reset_deadline();
}

#[actix_web::test]
async fn with_no_notice_the_endpoint_reports_none_and_nothing_is_gated() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    clean();

    let body = network_upgrade(1_234).await;
    assert_eq!(body["status"], "none", "{body}");
    assert_eq!(body["tip_height"], 1_234, "{body}");
    assert_eq!(body["local_protocol_version"], PROTOCOL_VERSION, "{body}");
    assert!(body["blocks_remaining"].is_null(), "{body}");
    assert!(body["notice"].is_null(), "{body}");
    assert_eq!(upgrade_deadline(), None, "no notice installs no deadline");
    assert_eq!(log_run_up(1_234), None, "nothing to warn about");
    clean();
}

#[actix_web::test]
async fn a_valid_notice_is_reported_and_hands_the_activation_to_the_3093_deadline() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    clean();
    const H: u64 = 14_000;

    assert_eq!(offer(sign(future_notice(H))), Ok(NoticeOutcome::Installed));

    let body = network_upgrade(H - 500).await;
    assert_eq!(body["status"], "upgrade_required", "{body}");
    assert_eq!(body["blocks_remaining"], 500, "{body}");
    assert_eq!(body["activation_height"], H, "{body}");
    assert_eq!(body["min_protocol_version"], PROTOCOL_VERSION + 1, "{body}");
    assert_eq!(body["release_commit"], RELEASE_COMMIT, "{body}");
    // The signed artifact travels verbatim, signatures included, so a peer
    // polling this endpoint re-verifies rather than trusting us.
    let echoed: SignedUpgradeNotice =
        serde_json::from_value(body["notice"].clone()).expect("the signed notice round-trips");
    assert_eq!(echoed, upgrade_notice().expect("installed"));
    assert_eq!(echoed.signatures.len(), 1);

    // #3093 handoff: a node whose genesis schedule knows nothing about this
    // feature still stops at H − 1.
    let deadline = upgrade_deadline().expect("the notice installed a deadline");
    assert_eq!(deadline.height, H);
    assert_eq!(deadline.feature, "future_feature");

    // Past the activation the same endpoint says so.
    let body = network_upgrade(H).await;
    assert_eq!(body["status"], "past_deadline", "{body}");
    assert_eq!(body["blocks_remaining"], 0, "{body}");
    clean();
}

#[actix_web::test]
async fn a_notice_this_binary_already_satisfies_is_ready_and_installs_no_deadline() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    clean();
    let mut notice = future_notice(14_000);
    notice.min_protocol_version = PROTOCOL_VERSION;
    notice.notice_id = UpgradeNotice::notice_id_for(14_000, RELEASE_COMMIT);
    assert_eq!(offer(sign(notice)), Ok(NoticeOutcome::Installed));

    let body = network_upgrade(13_999).await;
    assert_eq!(body["status"], "ready", "{body}");
    assert_eq!(upgrade_deadline(), None, "a satisfied notice halts nothing");

    // The satisfied line is emitted once, then never again.
    assert!(matches!(log_run_up(13_999), Some((RunUpLevel::Satisfied, _))));
    assert_eq!(log_run_up(13_999), None);
    assert_eq!(log_run_up(14_500), None);
    clean();
}

/// The core negative: every rejected shape leaves the node exactly as it was.
#[actix_web::test]
async fn tampered_foreign_expired_unsigned_and_unknown_signer_notices_are_all_inert() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    const H: u64 = 14_000;

    // 1. Tampered after signing: the height is raised, the signature is not.
    let mut tampered = sign(future_notice(H));
    tampered.notice.activation_height = H + 5_000;
    assert!(matches!(
        offer(tampered),
        Err(UpgradeNoticeError::BadSignature { .. })
    ));

    // 2. A notice for another chain.
    let mut foreign = future_notice(H);
    foreign.chain_id = "wrfcoin-mainnet".into();
    assert!(matches!(
        offer(sign(foreign)),
        Err(UpgradeNoticeError::WrongChain { .. })
    ));

    // 3. Another network on this chain.
    let mut other_network = future_notice(H);
    other_network.network_id = "devnet".into();
    assert!(matches!(
        offer(sign(other_network)),
        Err(UpgradeNoticeError::WrongChain { .. })
    ));

    // 4. Expired.
    let mut expired = future_notice(H);
    expired.expires_at = Some(NOW - 1);
    assert!(matches!(
        offer(sign(expired)),
        Err(UpgradeNoticeError::Expired { .. })
    ));

    // 5. No signatures at all.
    assert_eq!(
        offer(SignedUpgradeNotice {
            notice: future_notice(H),
            signatures: vec![],
        }),
        Err(UpgradeNoticeError::NoSignatures)
    );

    // 6. Correctly signed, by a key nobody authorized.
    let stranger = PrivateKey::generate(&mut StdRng::seed_from_u64(0xDEAD));
    let notice = future_notice(H);
    let signature = sign_upgrade_notice_with_key(&notice, &stranger);
    assert!(matches!(
        offer(SignedUpgradeNotice {
            notice,
            signatures: vec![signature]
        }),
        Err(UpgradeNoticeError::SignerNotAuthorized { .. })
    ));

    // 7. Garbage in the signature field.
    let mut malformed = sign(future_notice(H));
    malformed.signatures[0] = GovernanceSignature {
        signer_pubkey: governance_key().1.to_ascii_lowercase(),
        signature: "not-hex".into(),
    };
    assert!(matches!(
        offer(malformed),
        Err(UpgradeNoticeError::BadSignature { .. })
    ));

    // Nothing above touched the node.
    let body = network_upgrade(H - 1).await;
    assert_eq!(body["status"], "none", "{body}");
    assert!(body["notice"].is_null(), "{body}");
    assert_eq!(upgrade_deadline(), None, "no rejected notice installs a deadline");
    clean();
}

#[actix_web::test]
async fn the_highest_activation_wins_and_a_replayed_older_notice_cannot_walk_it_back() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    clean();

    assert_eq!(offer(sign(future_notice(14_000))), Ok(NoticeOutcome::Installed));
    assert_eq!(upgrade_deadline().map(|d| d.height), Some(14_000));

    // A later announcement supersedes.
    assert_eq!(offer(sign(future_notice(20_000))), Ok(NoticeOutcome::Installed));
    assert_eq!(upgrade_notice().unwrap().notice.activation_height, 20_000);

    // A replay of the earlier one is verified but does not supersede...
    assert_eq!(
        offer(sign(future_notice(14_000))),
        Ok(NoticeOutcome::NotSuperseding)
    );
    assert_eq!(upgrade_notice().unwrap().notice.activation_height, 20_000);
    // ...and the same notice offered twice is likewise inert.
    assert_eq!(
        offer(sign(future_notice(20_000))),
        Ok(NoticeOutcome::NotSuperseding)
    );

    let body = network_upgrade(19_000).await;
    assert_eq!(body["activation_height"], 20_000, "{body}");
    assert_eq!(body["blocks_remaining"], 1_000, "{body}");
    clean();
}

/// A notice may tighten the #3093 deadline but never relax one the genesis
/// schedule already installed: the safe composition of two answers to "what is
/// the first height this binary must not touch" is the lower one.
#[actix_web::test]
async fn a_notice_tightens_an_existing_deadline_and_never_relaxes_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    clean();

    use wrfcoin_native_chain::upgrade::{UpgradeDeadline, install_upgrade_deadline};
    install_upgrade_deadline(Some(UpgradeDeadline {
        feature: "from_genesis_schedule".into(),
        height: 12_000,
        running_protocol_version: PROTOCOL_VERSION,
    }));

    // A LATER notice must not push the genesis deadline out.
    assert_eq!(offer(sign(future_notice(14_000))), Ok(NoticeOutcome::Installed));
    let deadline = upgrade_deadline().expect("still installed");
    assert_eq!(deadline.height, 12_000, "the earlier genesis deadline stands");
    assert_eq!(deadline.feature, "from_genesis_schedule");

    // An EARLIER notice tightens it.
    assert_eq!(offer(sign(future_notice(20_000))), Ok(NoticeOutcome::Installed));
    clean();
    install_upgrade_deadline(Some(UpgradeDeadline {
        feature: "from_genesis_schedule".into(),
        height: 12_000,
        running_protocol_version: PROTOCOL_VERSION,
    }));
    assert_eq!(offer(sign(future_notice(9_000))), Ok(NoticeOutcome::Installed));
    assert_eq!(
        upgrade_deadline().map(|d| d.height),
        Some(9_000),
        "the notice tightened the halt"
    );
    clean();
}

/// The cadence, driven through the store rather than the pure rule: WARN every
/// 50 blocks inside the run-up window, ERROR every block inside the last 100,
/// and at most one line per height however often the poller ticks.
#[actix_web::test]
async fn the_run_up_cadence_is_one_line_per_height_at_the_specified_severities() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    clean();
    const H: u64 = 14_000;
    assert_eq!(offer(sign(future_notice(H))), Ok(NoticeOutcome::Installed));

    // Far out: silent.
    assert_eq!(log_run_up(H - 5_000), None);

    // Inside 2000 blocks, on a multiple of 50: one WARN naming everything.
    let (level, message) = log_run_up(H - 1_000).expect("a warning is due");
    assert_eq!(level, RunUpLevel::Warn);
    assert!(message.contains(&format!("UPGRADE REQUIRED by height {H}")), "{message}");
    assert!(message.contains("(in 1000 blocks)"), "{message}");
    assert!(message.contains(&format!("running protocol v{PROTOCOL_VERSION}")), "{message}");
    assert!(message.contains(RELEASE_COMMIT), "{message}");
    assert!(message.contains(&format!("needs v{}", PROTOCOL_VERSION + 1)), "{message}");
    assert!(message.contains("/network/upgrade"), "{message}");

    // The poller ticks faster than blocks arrive; the same height says it once.
    assert_eq!(log_run_up(H - 1_000), None, "one line per height, not per poll");

    // Off-cadence heights inside the window are silent.
    assert_eq!(log_run_up(H - 999), None);

    // Inside the last 100 blocks: every block, at ERROR.
    for height in (H - 100)..H {
        let (level, _) = log_run_up(height).unwrap_or_else(|| panic!("height {height} must log"));
        assert_eq!(level, RunUpLevel::Error, "height {height}");
    }
    // And at the activation itself.
    assert!(matches!(log_run_up(H), Some((RunUpLevel::Error, _))));
    clean();
}

/// The file transport: what a boot with `WRFCOIN_UPGRADE_NOTICE_FILE` does.
#[actix_web::test]
async fn a_notice_file_is_parsed_and_a_corrupt_one_is_refused_without_panicking() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    clean();
    const H: u64 = 14_000;

    let dir = tempfile::tempdir().expect("tempdir");
    let good = dir.path().join("upgrade-notice.json");
    std::fs::write(
        &good,
        serde_json::to_vec_pretty(&sign(future_notice(H))).expect("serialize"),
    )
    .expect("write");
    let loaded = read_notice_file(&good).expect("a well-formed notice file parses");
    assert_eq!(offer(loaded), Ok(NoticeOutcome::Installed));
    assert_eq!(network_upgrade(H - 10).await["status"], "upgrade_required");

    // Truncated / corrupt / absent files are refusals, never panics, and never
    // disturb the notice already held.
    let corrupt = dir.path().join("corrupt.json");
    std::fs::write(&corrupt, b"{\"notice\": ").expect("write");
    assert!(read_notice_file(&corrupt).is_err());
    assert!(read_notice_file(&dir.path().join("absent.json")).is_err());
    assert!(parse_notice_json(b"[]").is_err());
    assert_eq!(
        upgrade_notice().map(|s| s.notice.activation_height),
        Some(H),
        "a corrupt file does not clear the held notice"
    );
    clean();
}
