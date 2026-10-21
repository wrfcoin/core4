//! #2713 — with `WRFCOIN_CHAIN_ID` / `WRFCOIN_NETWORK_ID` unset, a node REPORTS
//! testnet and SIGNS mainnet.
//!
//! ## Status: this file is RED by design, and that is the point
//!
//! #2713 is already PROVEN by execution — a node with both variables unset reports
//! `chain_id: "777"` / `network_id: "unknown"` from `/network/info` while every
//! signature it produces is bound to `wrfcoin-main` / `mainnet`. This file is not
//! a second proof. It is the **regression artifact** the proof did not leave
//! behind: something that fails until the split is closed, and passes the moment
//! it is.
//!
//! The assertions below state the property the system should have — **the identity
//! a node reports MUST equal the identity it signs under** — so they fail today.
//! Do not "fix" them by asserting the current values. The split is the bug.
//!
//! ## The two sides
//!
//! | Side | Unset default | Where |
//! |---|---|---|
//! | signing | `wrfcoin-main` / `mainnet` | `domain_signing.rs:9,12` via `signing_chain_id()` / `signing_network_id()` |
//! | reporting | `777` / `unknown` | inline literals at `routes/network.rs:191-192` and `helpers.rs:1039-1041` |
//!
//! `signing_chain_id()` is not incidental: it is length-prefixed into the signing
//! preimage (`transaction.rs`), and consumed by the oracle ceremony and slashing.
//! The domain a signature commits to comes from that function, never from the
//! value the node reports.
//!
//! ## Control-first (#2711 shape)
//!
//! The controls run before the observations and must pass:
//!
//! * `control_a_...` — the reporting fallbacks are still written where this file
//!   thinks they are. A source scan that matched nothing would make every
//!   comparison below trivially "agree" by comparing against a value it never
//!   found. That is the vacuous-pass this control exists to prevent.
//! * `control_b_...` — when both variables ARE exported, the two sides agree.
//!   Without it, a red below could mean "identity resolution is broken in
//!   general" rather than "the UNSET case diverges", and the finding would be
//!   unattributable. A red must never be the harness wearing a finding's clothes.

use std::path::{Path, PathBuf};
use wrfcoin_native_chain::crypto::domain_signing::{
    CHAIN_ID_ENV, DEFAULT_CHAIN_ID, DEFAULT_NETWORK_ID, NETWORK_ID_ENV, signing_chain_id,
    signing_network_id,
};

/// The fallback the REPORTING side substitutes when `WRFCOIN_CHAIN_ID` is unset.
/// Guarded by `control_a_...` below — if the source moves, that control fails
/// rather than these comparisons silently passing.
const REPORTED_CHAIN_ID_FALLBACK: &str = "777";
/// The reporting fallback for `WRFCOIN_NETWORK_ID`.
const REPORTED_NETWORK_ID_FALLBACK: &str = "unknown";

/// Source sites carrying the reporting fallbacks, as `(path, needle)`.
fn reporting_fallback_sites() -> Vec<(PathBuf, &'static str)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    vec![
        (
            root.join("src/bin/launch_api/routes/network.rs"),
            r#"std::env::var("WRFCOIN_CHAIN_ID").unwrap_or_else(|_| "777".into())"#,
        ),
        (
            root.join("src/bin/launch_api/routes/network.rs"),
            r#"std::env::var("WRFCOIN_NETWORK_ID").unwrap_or_else(|_| "unknown".into())"#,
        ),
        (
            root.join("src/bin/launch_api/helpers.rs"),
            r#"std::env::var("WRFCOIN_CHAIN_ID").unwrap_or_else(|_| "777".to_string())"#,
        ),
    ]
}

/// Run `run` with both identity variables removed, restoring them afterwards.
///
/// `setenv`/`unsetenv` mutate the process-wide `environ`, so a local lock
/// serialises the threads in this binary that actually race.
fn with_identity_unset<T>(run: impl FnOnce() -> T) -> T {
    with_identity(None, None, run)
}

fn with_identity<T>(chain: Option<&str>, network: Option<&str>, run: impl FnOnce() -> T) -> T {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let prev_chain = std::env::var(CHAIN_ID_ENV).ok();
    let prev_network = std::env::var(NETWORK_ID_ENV).ok();
    unsafe {
        match chain {
            Some(v) => std::env::set_var(CHAIN_ID_ENV, v),
            None => std::env::remove_var(CHAIN_ID_ENV),
        }
        match network {
            Some(v) => std::env::set_var(NETWORK_ID_ENV, v),
            None => std::env::remove_var(NETWORK_ID_ENV),
        }
    }
    let out = run();
    unsafe {
        match prev_chain {
            Some(v) => std::env::set_var(CHAIN_ID_ENV, v),
            None => std::env::remove_var(CHAIN_ID_ENV),
        }
        match prev_network {
            Some(v) => std::env::set_var(NETWORK_ID_ENV, v),
            None => std::env::remove_var(NETWORK_ID_ENV),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// CONTROLS
// ---------------------------------------------------------------------------

/// CONTROL A. The reporting fallbacks are still written where this file claims.
///
/// Anti-vacuity: the constants above are hand-copied from the source, so if the
/// source changes and this control does not fail, the observations below would be
/// comparing the signing side against a value nothing produces any more.
#[test]
fn control_a_reporting_fallbacks_are_still_written_where_this_test_claims() {
    let sites = reporting_fallback_sites();
    assert!(
        sites.len() >= 3,
        "CONTROL A FAILED — the site list is empty or truncated; the scan below \
         would examine nothing"
    );

    for (path, needle) in sites {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "CONTROL A FAILED — cannot read {}: {e}. The reporting side may have \
                 moved; update this test rather than deleting it.",
                path.display()
            )
        });
        assert!(
            text.contains(needle),
            "CONTROL A FAILED — {} no longer contains `{needle}`. Either the split \
             was fixed (check, then retire this file) or the fallback moved. Until \
             this is resolved the assertions below prove nothing.",
            path.display()
        );
    }
}

/// CONTROL B. With both variables exported, the two sides agree.
///
/// This is what makes a red below attributable to the UNSET case specifically
/// rather than to identity resolution being broken everywhere.
#[test]
fn control_b_explicit_configuration_makes_both_sides_agree() {
    with_identity(Some("wrfcoin-test"), Some("testnet"), || {
        let reported_chain = std::env::var(CHAIN_ID_ENV)
            .unwrap_or_else(|_| REPORTED_CHAIN_ID_FALLBACK.to_string());
        let reported_network = std::env::var(NETWORK_ID_ENV)
            .unwrap_or_else(|_| REPORTED_NETWORK_ID_FALLBACK.to_string());

        assert_eq!(
            signing_chain_id(),
            reported_chain,
            "CONTROL B FAILED — the two sides disagree even when explicitly \
             configured, so the divergence is not specific to the unset case and \
             nothing below is attributable to #2713."
        );
        assert_eq!(
            signing_network_id(),
            reported_network,
            "CONTROL B FAILED — network ids disagree under explicit configuration"
        );
    });
}

// ---------------------------------------------------------------------------
// OBSERVATIONS — RED until #2713 is closed
// ---------------------------------------------------------------------------

/// RED. A node that reports one chain id MUST sign under that chain id.
#[test]
fn unset_chain_id_reports_and_signs_the_same_identity() {
    let (reported, signed) = with_identity_unset(|| {
        let reported = std::env::var(CHAIN_ID_ENV)
            .unwrap_or_else(|_| REPORTED_CHAIN_ID_FALLBACK.to_string());
        (reported, signing_chain_id())
    });

    assert_eq!(
        reported, signed,
        "#2713 — with {CHAIN_ID_ENV} unset this node REPORTS chain_id `{reported}` \
         while every signature it produces is bound to `{signed}`. An operator, a \
         dashboard, or a script reading /network/info sees testnet; the signing \
         domain is mainnet. Resolve chain identity from ONE place and make the \
         unset case refuse to start rather than default."
    );
}

/// RED. The same split on the network id.
#[test]
fn unset_network_id_reports_and_signs_the_same_identity() {
    let (reported, signed) = with_identity_unset(|| {
        let reported = std::env::var(NETWORK_ID_ENV)
            .unwrap_or_else(|_| REPORTED_NETWORK_ID_FALLBACK.to_string());
        (reported, signing_network_id())
    });

    assert_eq!(
        reported, signed,
        "#2713 — with {NETWORK_ID_ENV} unset this node REPORTS network_id \
         `{reported}` while signing under `{signed}`."
    );
}

/// The declared defaults themselves disagree — the split stated without any env
/// manipulation, so it survives even if the harness's env handling is doubted.
#[test]
fn declared_signing_defaults_disagree_with_the_reporting_fallbacks() {
    assert_eq!(
        DEFAULT_CHAIN_ID, REPORTED_CHAIN_ID_FALLBACK,
        "#2713 — the signing fallback (`{DEFAULT_CHAIN_ID}`) and the reporting \
         fallback (`{REPORTED_CHAIN_ID_FALLBACK}`) are different constants. Two \
         subsystems each picking their own fallback IS the defect; that they \
         currently disagree is only what makes it visible."
    );
    assert_eq!(
        DEFAULT_NETWORK_ID, REPORTED_NETWORK_ID_FALLBACK,
        "#2713 — signing falls back to `{DEFAULT_NETWORK_ID}`, reporting to \
         `{REPORTED_NETWORK_ID_FALLBACK}`"
    );
}

/// A SECOND divergence #2713 does not mention: an **empty** export.
///
/// The signing side filters empty values (`.filter(|s| !s.trim().is_empty())`,
/// `domain_signing.rs:32`) and falls back; the reporting side uses a bare
/// `std::env::var(..).unwrap_or_else(..)` with no such filter, so it reports the
/// empty string. `WRFCOIN_CHAIN_ID=` — an ordinary way to write "unset" in a
/// compose file or a systemd unit — therefore diverges too, and differently: the
/// node reports nothing at all while signing mainnet.
///
/// Worth fixing in the same change; a single resolver removes both cases at once.
#[test]
fn empty_string_export_diverges_by_a_different_mechanism() {
    let (reported, signed) = with_identity(Some(""), Some(""), || {
        let reported = std::env::var(CHAIN_ID_ENV)
            .unwrap_or_else(|_| REPORTED_CHAIN_ID_FALLBACK.to_string());
        (reported, signing_chain_id())
    });

    assert_eq!(
        reported, signed,
        "#2713 (extension) — `{CHAIN_ID_ENV}=` reports `{reported}` (the empty \
         string, because the reporting side applies no emptiness filter) while \
         signing under `{signed}`. The unset case is not the only one."
    );
}
