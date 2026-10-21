//! #2713 fix — chain identity must be DECLARED, or the node refuses to start.
//!
//! Two constraints were placed on this fix, and this file exists to verify each of
//! them rather than assert them in prose.
//!
//! ## Constraint 1 — fail-CLOSED: unset must REFUSE, never default
//!
//! Verified by `unset_*` / `empty_*` below: `declared_chain_identity()` returns
//! `Err` when either variable is unset or empty-after-trim, and the error names the
//! variables that are missing.
//!
//! Defaulting the unset case to something plausible — testnet, say — is the
//! inverse mistake #2523 deleted `GENESIS_ORACLE_SET_ENV` for: "unset" validated
//! nothing and said nothing, and one forgotten export on cut day would have let a
//! node accept a unanimous-but-wrong block 1 in silence. `refusal_names_the_missing_variables`
//! pins that the message is actionable, because a refusal a human cannot act on in
//! thirty seconds is not the trade this fix claims.
//!
//! ## Constraint 2 — NO behaviour change for a correctly configured node
//!
//! Verified two ways, because "I didn't change it" is not evidence:
//!
//! * `configured_and_signing_agree_whenever_the_variable_is_set` — for a spread of
//!   values including whitespace-padded and mainnet-looking ones, `signing_chain_id()`
//!   and `signing_network_id()` return exactly the environment value. That is the
//!   post-refactor behaviour, and it is the behaviour a configured node had before:
//!   `signing_chain_id()` is now literally
//!   `configured_chain_id().unwrap_or_else(|| DEFAULT_CHAIN_ID.to_string())`, the
//!   same expression it always evaluated.
//! * `fallback_is_unchanged_when_the_variable_is_absent` — the *defaulting* path is
//!   also byte-identical (`wrfcoin-main` / `mainnet`). The fix does not alter what
//!   `signing_chain_id()` returns in any case; it adds a separate startup gate that
//!   makes the defaulting case unreachable **in the node**. Off-node signers
//!   (wallet, faucet, collector) are untouched, which matters: changing their
//!   resolution would break signature verification against configured peers.
//!
//! ## What is deliberately NOT fixed here
//!
//! The divergent *reporting* literals at `routes/network.rs:191-192` and
//! `helpers.rs:1039-1041` are left in place. Routing them through the shared
//! resolver is the structural fix, and it changes `Config::from_env()`'s output in
//! the unset case — behaviour currently pinned by `helpers.rs:1445-1446` and
//! `main.rs:10294`. Editing pinned expectations is not something to ride along
//! under a genesis-cut freeze. Those literals are now unreachable in a running
//! node, and `chain_identity_split_2713.rs` stays RED to say they still exist.

use wrfcoin_native_chain::crypto::domain_signing::{
    CHAIN_ID_ENV, DEFAULT_CHAIN_ID, DEFAULT_NETWORK_ID, NETWORK_ID_ENV, configured_chain_id,
    configured_network_id, declared_chain_identity, signing_chain_id, signing_network_id,
};

fn with_identity<T>(chain: Option<&str>, network: Option<&str>, run: impl FnOnce() -> T) -> T {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let prev = (
        std::env::var(CHAIN_ID_ENV).ok(),
        std::env::var(NETWORK_ID_ENV).ok(),
    );
    let set = |key: &str, value: Option<&str>| unsafe {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    };
    set(CHAIN_ID_ENV, chain);
    set(NETWORK_ID_ENV, network);
    let out = run();
    set(CHAIN_ID_ENV, prev.0.as_deref());
    set(NETWORK_ID_ENV, prev.1.as_deref());
    out
}

// ---------------------------------------------------------------------------
// CONTROL — the resolver accepts a well-formed declaration
// ---------------------------------------------------------------------------

/// Without this, every `Err` below could be a resolver that refuses everything,
/// and "fail-closed" would be indistinguishable from "broken".
#[test]
fn control_a_declared_identity_is_accepted() {
    let declared = with_identity(Some("777"), Some("testnet"), declared_chain_identity)
        .expect("CONTROL A FAILED — a fully declared identity was refused, so the \
                 refusals below are not evidence of fail-closed behaviour");
    assert_eq!(declared.chain_id, "777");
    assert_eq!(declared.network_id, "testnet");
}

// ---------------------------------------------------------------------------
// CONSTRAINT 1 — fail-CLOSED
// ---------------------------------------------------------------------------

#[test]
fn unset_chain_id_refuses() {
    let err = with_identity(None, Some("testnet"), declared_chain_identity)
        .expect_err("an unset WRFCOIN_CHAIN_ID must REFUSE, not default");
    assert!(err.contains(CHAIN_ID_ENV), "refusal must name the variable: {err}");
}

#[test]
fn unset_network_id_refuses() {
    let err = with_identity(Some("777"), None, declared_chain_identity)
        .expect_err("an unset WRFCOIN_NETWORK_ID must REFUSE, not default");
    assert!(err.contains(NETWORK_ID_ENV), "refusal must name the variable: {err}");
}

#[test]
fn both_unset_refuses_and_names_both() {
    let err = with_identity(None, None, declared_chain_identity)
        .expect_err("the #2713 configuration must refuse");
    assert!(
        err.contains(CHAIN_ID_ENV) && err.contains(NETWORK_ID_ENV),
        "both missing variables must be named: {err}"
    );
}

/// `WRFCOIN_CHAIN_ID=` is how "unset" is written in a compose file or a systemd
/// unit. Accepting it would reintroduce the silent default one layer down — and it
/// is the second divergence `chain_identity_split_2713.rs` found, where the
/// reporting side reports `""` while signing falls back to mainnet.
#[test]
fn empty_and_whitespace_declarations_refuse() {
    for (chain, network) in [("", "testnet"), ("   ", "testnet"), ("777", ""), ("777", "\t")] {
        let err = with_identity(Some(chain), Some(network), declared_chain_identity).expect_err(
            &format!("an empty/whitespace declaration ({chain:?}, {network:?}) must refuse"),
        );
        assert!(!err.is_empty());
    }
}

/// A refusal a human cannot act on in thirty seconds is not the trade this fix
/// claims. The message must say what to set and why it is not defaulted.
#[test]
fn refusal_names_the_missing_variables_and_the_reason() {
    let err = with_identity(None, None, declared_chain_identity).expect_err("must refuse");
    for expected in [
        CHAIN_ID_ENV,
        NETWORK_ID_ENV,
        DEFAULT_CHAIN_ID,
        "#2713",
        "#2717",
        "no default",
    ] {
        assert!(
            err.contains(expected),
            "the refusal must mention {expected:?} so an operator can act on it \
             without reading the source. Message was:\n{err}"
        );
    }
}

// ---------------------------------------------------------------------------
// CONSTRAINT 2 — no behaviour change for a correctly configured node
// ---------------------------------------------------------------------------

/// With the variable set, `signing_*` returns the environment value verbatim.
///
/// This is the property a correctly configured node depends on, and it is what
/// makes this a misconfiguration fix rather than a consensus change: the signing
/// domain of every signature a configured node produces is unchanged.
#[test]
fn configured_and_signing_agree_whenever_the_variable_is_set() {
    for (chain, network) in [
        ("777", "testnet"),
        ("wrfcoin-main", "mainnet"),
        ("wrfcoin-test-2", "dev"),
        ("  padded  ", "  spaced  "),
    ] {
        with_identity(Some(chain), Some(network), || {
            assert_eq!(
                signing_chain_id(),
                chain,
                "signing_chain_id() must return the configured value verbatim"
            );
            assert_eq!(signing_network_id(), network);
            assert_eq!(configured_chain_id().as_deref(), Some(chain));
            assert_eq!(configured_network_id().as_deref(), Some(network));

            let declared = declared_chain_identity().expect("declared");
            assert_eq!(declared.chain_id, signing_chain_id());
            assert_eq!(declared.network_id, signing_network_id());
        });
    }
}

/// The DEFAULTING path is unchanged too.
///
/// The fix adds a startup gate; it does not alter what `signing_chain_id()`
/// returns in any case. That matters for off-node signers — the wallet, faucet and
/// collector call these functions and never reach `launch_api`'s `main()`. Had the
/// refactor changed their resolution, they would sign under a different domain than
/// the nodes verifying them, which is a worse bug than the one being fixed.
#[test]
fn fallback_is_unchanged_when_the_variable_is_absent() {
    with_identity(None, None, || {
        assert_eq!(signing_chain_id(), DEFAULT_CHAIN_ID);
        assert_eq!(signing_network_id(), DEFAULT_NETWORK_ID);
        assert_eq!(configured_chain_id(), None);
        assert_eq!(configured_network_id(), None);
    });
}

/// The refactor is behaviour-preserving by construction, asserted rather than
/// claimed: `signing_chain_id()` must equal
/// `configured_chain_id().unwrap_or(DEFAULT_CHAIN_ID)` for set, unset and empty.
#[test]
fn signing_is_exactly_configured_or_default_in_every_case() {
    for value in [Some("777"), Some(""), Some("  "), None] {
        with_identity(value, value, || {
            assert_eq!(
                signing_chain_id(),
                configured_chain_id().unwrap_or_else(|| DEFAULT_CHAIN_ID.to_string()),
                "signing_chain_id() diverged from configured-or-default for {value:?}"
            );
            assert_eq!(
                signing_network_id(),
                configured_network_id().unwrap_or_else(|| DEFAULT_NETWORK_ID.to_string()),
                "signing_network_id() diverged from configured-or-default for {value:?}"
            );
        });
    }
}
