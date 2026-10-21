//! #2712 fix — a multi-key oracle-governance allowlist must be DECLARED, or the
//! node refuses to start.
//!
//! ## Constraint 1 — fail-CLOSED: the AMBIGUOUS state refuses
//!
//! The ambiguous state is not "more than one key". A two-entry allowlist is
//! syntactically legitimate *and* is exactly what an appending rotation produces,
//! and from the value alone the two are indistinguishable. **That
//! indistinguishability is the defect**, so it is what refuses: more than one key
//! that nobody declared.
//!
//! Refusing every multi-key list would have been the easy version and the wrong
//! one — it picks an interpretation (`>1 is always a mistake`) rather than
//! demanding the operator state theirs, which is the same class of error as
//! picking a plausible default.
//!
//! ## Constraint 2 — no behaviour change for a correctly configured node
//!
//! Verified by `single_key_allowlist_needs_no_declaration` and
//! `empty_allowlist_is_unchanged`: the two configurations that exist anywhere in
//! this workspace are accepted with no new variable. Every setter — the demo
//! scripts, the two-node verifier, the ceremony tests — configures exactly one
//! governance key, and `scripts/verify-oracle-governance-allowlist.sh` cannot even
//! express a multi-key expectation (`--expected-pubkey` is singular). There is no
//! deliberate multi-signer deployment for this to break.
//!
//! The per-transaction resolver is untouched, which is the load-bearing half:
//! `oracle_governance_pubkeys()` is called by
//! `verify_oracle_registration_authorization` on the producer's self-screen and on
//! every peer's `verify_block`. `resolver_is_untouched_by_the_declaration` asserts
//! a declaration mismatch changes nothing about who authorizes, so this cannot
//! partition a network.

use wrfcoin_native_chain::crypto::domain_signing::{
    ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV, ORACLE_GOVERNANCE_PUBKEYS_ENV,
    check_oracle_governance_allowlist_declaration, is_authorized_oracle_governance,
    oracle_governance_pubkeys,
};

const KEY_A: &str = "aa11111111111111111111111111111111111111111111111111111111111111";
const KEY_B: &str = "bb22222222222222222222222222222222222222222222222222222222222222";

fn with_env<T>(allowlist: Option<&str>, count: Option<&str>, run: impl FnOnce() -> T) -> T {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let prev = (
        std::env::var(ORACLE_GOVERNANCE_PUBKEYS_ENV).ok(),
        std::env::var(ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV).ok(),
    );
    let set = |key: &str, value: Option<&str>| unsafe {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    };
    set(ORACLE_GOVERNANCE_PUBKEYS_ENV, allowlist);
    set(ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV, count);
    let out = run();
    set(ORACLE_GOVERNANCE_PUBKEYS_ENV, prev.0.as_deref());
    set(ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV, prev.1.as_deref());
    out
}

// ---------------------------------------------------------------------------
// CONTROL — the check accepts well-formed configurations
// ---------------------------------------------------------------------------

/// Without this, every refusal below could be a check that refuses everything,
/// and "fail-closed" would be indistinguishable from "broken".
#[test]
fn control_a_declared_multi_key_allowlist_is_accepted() {
    let n = with_env(
        Some(&format!("{KEY_A},{KEY_B}")),
        Some("2"),
        check_oracle_governance_allowlist_declaration,
    )
    .expect(
        "CONTROL A FAILED — a correctly DECLARED two-key allowlist was refused, so the \
         refusals below are not evidence of fail-closed behaviour, only of a broken check",
    );
    assert_eq!(n, 2);
}

// ---------------------------------------------------------------------------
// CONSTRAINT 1 — the ambiguous state refuses
// ---------------------------------------------------------------------------

/// THE FIX. `=<old>,<new>` with nothing declared is the appending rotation, and it
/// is now a refusal instead of a silent widening.
#[test]
fn appended_rotation_without_a_declaration_refuses() {
    let err = with_env(
        Some(&format!("{KEY_A},{KEY_B}")),
        None,
        check_oracle_governance_allowlist_declaration,
    )
    .expect_err("an undeclared multi-key allowlist is the #2712 state and must refuse");
    assert!(err.contains(ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV));
    assert!(
        err.contains("APPENDED") && err.contains("REPLACING"),
        "the refusal must name the mistake it suspects, or an operator cannot act on \
         it in thirty seconds: {err}"
    );
}

/// A declaration that disagrees with reality refuses — including the case where
/// the operator declared 1 and the list grew to 2, which is the appending rotation
/// performed by someone who *had* declared their intent.
#[test]
fn declared_count_mismatch_refuses_in_both_directions() {
    let grew = with_env(
        Some(&format!("{KEY_A},{KEY_B}")),
        Some("1"),
        check_oracle_governance_allowlist_declaration,
    )
    .expect_err("declared 1, resolved 2 — must refuse");
    assert!(grew.contains("declares 1"), "{grew}");

    let shrank = with_env(
        Some(KEY_A),
        Some("2"),
        check_oracle_governance_allowlist_declaration,
    )
    .expect_err("declared 2, resolved 1 — must refuse; a key went missing");
    assert!(shrank.contains("declares 2"), "{shrank}");
}

/// An unparseable declaration must not read as "no declaration".
///
/// That silent downgrade is the shape of the bug being fixed, one level up: a
/// malformed expectation that falls back to "unconstrained" is worse than none,
/// because it looks like the operator declared something.
#[test]
fn unparseable_declaration_refuses_rather_than_reading_as_absent() {
    for bad in ["two", "-1", "2.0", "0x2"] {
        let err = with_env(
            Some(&format!("{KEY_A},{KEY_B}")),
            Some(bad),
            check_oracle_governance_allowlist_declaration,
        )
        .expect_err(&format!("{bad:?} must refuse, not fall back to unconstrained"));
        assert!(err.contains(ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV), "{err}");
    }
}

/// A list holding the same key twice is a list nobody read. It refuses regardless
/// of any declaration, because a duplicate makes the count itself ambiguous.
#[test]
fn duplicate_entries_refuse_even_when_the_count_would_match() {
    let err = with_env(
        Some(&format!("{KEY_A},{KEY_A}")),
        Some("2"),
        check_oracle_governance_allowlist_declaration,
    )
    .expect_err("a duplicated key must refuse even though the raw count matches");
    assert!(err.contains("distinct"), "{err}");

    // Case and whitespace do not launder a duplicate: the resolver lowercases and
    // trims, so `A, a` is one key written twice.
    let laundered = with_env(
        Some(&format!("{}, {KEY_A}", KEY_A.to_uppercase())),
        None,
        check_oracle_governance_allowlist_declaration,
    )
    .expect_err("a case-and-space duplicate must still refuse");
    assert!(laundered.contains("distinct"), "{laundered}");
}

// ---------------------------------------------------------------------------
// CONSTRAINT 2 — no behaviour change for a correctly configured node
// ---------------------------------------------------------------------------

/// The configuration every setter in this workspace uses: exactly one key, no new
/// variable. Accepted unchanged.
///
/// A single-entry allowlist is unambiguous — there is nothing a rotation could
/// have silently added — so requiring a declaration there would be ceremony that
/// buys nothing and breaks every existing deployment.
#[test]
fn single_key_allowlist_needs_no_declaration() {
    let n = with_env(Some(KEY_A), None, check_oracle_governance_allowlist_declaration)
        .expect("a single-key allowlist must remain valid with no new variable");
    assert_eq!(n, 1);

    // And it is still accepted when the operator does declare it.
    let declared = with_env(Some(KEY_A), Some("1"), check_oracle_governance_allowlist_declaration)
        .expect("declaring the truth must never refuse");
    assert_eq!(declared, 1);
}

/// An unset or empty allowlist is unchanged — still permitted at startup, still
/// fail-closed at authorization.
///
/// Refusing here would have been a tempting over-reach: it breaks every node that
/// legitimately runs without oracle governance, including the deployed testnet,
/// which sets this variable nowhere.
#[test]
fn empty_allowlist_is_unchanged() {
    for value in [None, Some(""), Some("  "), Some(" , , ")] {
        let n = with_env(value, None, check_oracle_governance_allowlist_declaration)
            .unwrap_or_else(|e| panic!("an empty allowlist must still start ({value:?}): {e}"));
        assert_eq!(n, 0);
        // Still fail-closed where it counts.
        assert!(!with_env(value, None, || is_authorized_oracle_governance(KEY_A)));
    }
}

/// The per-transaction resolver is untouched by any declaration.
///
/// This is what keeps the fix off the consensus path. `oracle_governance_pubkeys`
/// and `is_authorized_oracle_governance` are called for every registration by
/// `verify_oracle_registration_authorization`; if a declaration mismatch changed
/// *who authorizes*, a misdeclared node would reject registrations its peers
/// accept, and the fix would partition the network instead of refusing one node.
#[test]
fn resolver_is_untouched_by_the_declaration() {
    let both = format!("{KEY_A},{KEY_B}");
    let baseline = with_env(Some(&both), None, oracle_governance_pubkeys);

    for declared in [None, Some("1"), Some("2"), Some("nonsense")] {
        assert_eq!(
            with_env(Some(&both), declared, oracle_governance_pubkeys),
            baseline,
            "the declaration changed the resolved allowlist for {declared:?} — it must \
             only gate STARTUP, never authorization, or a misdeclared node partitions"
        );
        assert!(
            with_env(Some(&both), declared, || is_authorized_oracle_governance(KEY_A)),
            "authorization changed for {declared:?}"
        );
    }
}
