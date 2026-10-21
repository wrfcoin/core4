//! The restore path accepts whatever producer set is on disk, and never compares
//! it to the set the deployment declared.
//!
//! Handed off as "`check_restored_chain_producer_set` at `main.rs:1663`,
//! unexamined so far, and a restore path that re-seats a producer set is exactly
//! where a retired key would come back." This file is that examination.
//!
//! ## Result, stated before the assertions
//!
//! **Two parts confirmed, one part REFUTED.** The refutation is the more useful
//! half and is not buried:
//!
//! 1. **CONFIRMED — the check is identity- and cardinality-blind by signature.**
//!    `check_restored_chain_producer_set(raw_require, producer_set_is_empty: bool)`
//!    takes a *boolean*. It can answer "is there a set?" and nothing else. Two
//!    restored chains — one holding the declared committee, one holding a
//!    different committee — are indistinguishable to it.
//!
//! 2. **CONFIRMED — the declared set is never read on restore.**
//!    `genesis_producers::resolve()` has exactly one call site
//!    (`genesis_producer_members()`, `main.rs:721`), itself called from exactly
//!    one place: inside the `Ok(None)` fresh-genesis arm at `main.rs:1430`. The
//!    `Ok(Some(chain))` arm takes persisted state as-is. So
//!    `WRFCOIN_GENESIS_PRODUCER_SET` is not consulted on any restart.
//!
//! 3. **REFUTED — "a retired key comes back."** It does not come back by replay.
//!    `producer_set_history` records changes by height and a retirement is itself
//!    a change; replaying it preserves the retirement. The exposure is narrower
//!    than the handoff's phrasing: a **stale or foreign data volume** whose
//!    persisted set predates the retirement is adopted wholesale. That is a
//!    different failure with a different fix, and saying "a retired key comes
//!    back" would send the next reader to look at replay, where there is nothing
//!    to find.
//!
//! ## And a second refutation: a compensating control DOES exist
//!
//! A foreign volume is not entirely unguarded. The #2035 genesis-identity check
//! (`main.rs:1748`) compares the locally-resolved genesis hash against a pinned
//! expectation and refuses to start on mismatch. A volume from another chain has
//! a different genesis hash and is caught there — **not** by the producer check.
//!
//! Three conditions gate it, all recorded here because each is a way it does not
//! run:
//!
//! * it is `#[cfg(feature = "p2p")]`, so a build without that feature has it not
//!   at all;
//! * it fires only when a *committing* expectation exists — an explicit
//!   `WRFCOIN_GENESIS_HASH`, or the pinned testnet hash reached via
//!   `WRFCOIN_NETWORK_ID=testnet`;
//! * therefore, with `WRFCOIN_NETWORK_ID` **unset** and no explicit hash, there is
//!   no expectation and the check is skipped.
//!
//! That last condition is #2713's default. **The unset-network-id split disables
//! the check that would otherwise catch this one.** Neither issue is wrong on its
//! own; the composition is what leaves the gap, and it is why these were worth
//! examining in the same pass.
//!
//! ## Status of this file
//!
//! Characterisation, and it PASSES. The property that would make it a real gate —
//! "the restored producer set matches the declared one" — has no API to assert
//! against, and designing one is a change to startup semantics, which the genesis
//! cut freeze excludes. What this buys: the two confirmed facts stop being a
//! source read, and if anyone later wires the declared set into the restore path,
//! `restore_path_never_reads_the_declared_producer_set` fails and forces this
//! file to be revisited rather than quietly outlived.

use std::path::{Path, PathBuf};

fn launch_api(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/launch_api").join(file)
}

fn read(file: &str) -> String {
    let path = launch_api(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Count occurrences of `needle` on lines that are not comments.
///
/// A doc comment naming a function is not a call to it, and this whole file
/// turns on call *counts*.
fn code_occurrences(text: &str, needle: &str) -> usize {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with("*") && !t.starts_with("/*")
        })
        .filter(|l| l.contains(needle))
        .count()
}

// ---------------------------------------------------------------------------
// CONTROL — the scan is looking at real files with the expected shape
// ---------------------------------------------------------------------------

/// Without this, every "we found no binding" assertion below could be a wrong
/// path or a renamed symbol reporting absence, which is the vacuous pass this
/// file most needs to avoid.
#[test]
fn control_scan_reads_the_real_startup_sources() {
    let main = read("main.rs");
    let producers = read("genesis_producers.rs");

    assert!(
        main.len() > 50_000,
        "CONTROL FAILED — main.rs is {} bytes, far smaller than expected; the scan \
         is reading the wrong tree",
        main.len()
    );
    assert!(
        producers.contains("pub fn check_restored_chain_producer_set"),
        "CONTROL FAILED — check_restored_chain_producer_set not found in \
         genesis_producers.rs; it moved or was renamed, and nothing below is \
         evidence about it"
    );
    assert!(
        producers.contains("pub fn resolve()"),
        "CONTROL FAILED — genesis_producers::resolve() not found"
    );
    assert!(
        code_occurrences(&main, "check_restored_chain_producer_set") >= 1,
        "CONTROL FAILED — main.rs does not call the restore-path check at all, \
         which would be a bigger finding than the one this file reports"
    );
}

// ---------------------------------------------------------------------------
// CONFIRMED 1 — blind by signature
// ---------------------------------------------------------------------------

/// The restore-path check's only set-related input is a `bool`.
///
/// A function that receives one bit about the producer set cannot compare
/// membership, cannot compare cardinality, and cannot notice that the set on disk
/// is not the set the operator declared. This is not a bug in its implementation
/// — it is the limit of its signature, which is why reading its body would not
/// have revealed it.
#[test]
fn restore_path_check_receives_only_whether_the_set_is_empty() {
    let producers = read("genesis_producers.rs");
    let start = producers
        .find("pub fn check_restored_chain_producer_set")
        .expect("control_scan_reads_the_real_startup_sources guards this");
    let signature: String = producers[start..].chars().take(200).collect();

    assert!(
        signature.contains("producer_set_is_empty: bool"),
        "the restore-path check no longer takes `producer_set_is_empty: bool`. If \
         it now receives the set itself, this finding may be fixed — verify, then \
         retire this file. Signature seen:\n{signature}"
    );
    assert!(
        !signature.contains("members") && !signature.contains("ProducerMember"),
        "the check now receives producer members, so it CAN compare identity. \
         Re-examine: this file's premise no longer holds.\n{signature}"
    );
}

// ---------------------------------------------------------------------------
// CONFIRMED 2 — the declared set is never read on restore
// ---------------------------------------------------------------------------

/// `resolve()` reaches startup through exactly one call site, and that site is on
/// the fresh-genesis branch.
///
/// If a second call site ever appears, this fails — and it should, because a
/// second one is either the fix (good, retire this file) or an unreviewed second
/// resolution path (bad, and worth catching).
#[test]
fn restore_path_never_reads_the_declared_producer_set() {
    let main = read("main.rs");

    assert_eq!(
        code_occurrences(&main, "genesis_producers::resolve()"),
        1,
        "expected exactly one `genesis_producers::resolve()` call site in main.rs. \
         A second one is either the fix for this finding or an unreviewed second \
         resolution path; either way this file must be revisited."
    );
    assert_eq!(
        code_occurrences(&main, "genesis_producer_members()"),
        1,
        "expected exactly one call to genesis_producer_members(); it is reached \
         only from the fresh-genesis (None) arm"
    );

    // The fresh arm seeds; the restore arm does not. Assert the seeding call
    // falls inside the fresh arm of the loaded-state match specifically.
    //
    // Since #3301 the state is loaded by `gate_then_load_state` (gate first,
    // then `Storage::load_state`) and main matches on its result,
    // `match loaded_chain { Some(chain) => … None => … }`. `None => {` is far
    // from unique, so anchor on that match — which is unique — and take the
    // restore arm and then the FIRST fresh arm after it. Getting this wrong is
    // the same class of error this file reports: a check that appears to
    // measure something and does not.
    let load_match = main
        .find("match loaded_chain {")
        .expect("the loaded-state match must exist");
    assert_eq!(
        code_occurrences(&main, "match loaded_chain {"),
        1,
        "the loaded-state match anchor is no longer unique; re-derive this test's anchors"
    );
    let restore_arm = load_match
        + main[load_match..]
            .find("Some(chain) => {")
            .expect("the restore arm must follow the loaded-state match");
    let fresh_arm = restore_arm
        + main[restore_arm..]
            .find("None => {")
            .expect("the fresh-genesis arm must follow the restore arm");
    let seeding = main
        .find("let genesis_producers = genesis_producer_members()")
        .expect("seeding call");
    assert!(
        fresh_arm < seeding,
        "the seeding call is no longer inside the fresh-genesis arm — the branch \
         structure changed and this finding must be re-derived rather than trusted"
    );
    assert!(
        seeding.saturating_sub(fresh_arm) < 2_000,
        "the seeding call is {} bytes after the fresh-genesis arm opens, which is \
         too far to conclude it is inside that arm; re-derive rather than trust",
        seeding - fresh_arm
    );
}

// ---------------------------------------------------------------------------
// REFUTED — the compensating control, and the condition that disables it
// ---------------------------------------------------------------------------

/// The #2035 genesis-identity check is what actually catches a foreign volume —
/// and it is feature-gated and expectation-gated.
///
/// Recorded as a test rather than a comment because the honest version of this
/// finding depends on it: without this check the exposure would be "any foreign
/// volume is adopted silently", which is **false**. With it, the exposure is "a
/// foreign volume is adopted silently *when no committing genesis expectation
/// exists*" — which, per #2713, is the default when `WRFCOIN_NETWORK_ID` is unset.
#[test]
fn foreign_volume_is_caught_by_the_genesis_identity_check_not_the_producer_check() {
    let main = read("main.rs");

    let guard = main
        .find("wrfcoin_p2p::expected_genesis_hash()")
        .expect("the #2035 genesis-identity check must exist; if it is gone, the \
                 exposure this file describes is strictly worse than reported");

    // The nearest `#[cfg(...)]` above the check, within the enclosing block.
    let window_start = guard.saturating_sub(600);
    let window = &main[window_start..guard];
    assert!(
        window.contains(r#"#[cfg(feature = "p2p")]"#),
        "the #2035 check is no longer behind `#[cfg(feature = \"p2p\")]` within the \
         600 bytes above it. If it now runs unconditionally the compensating \
         control is STRONGER than this file reports and the finding should be \
         downgraded — verify before assuming the gate weakened."
    );

    assert!(
        main.contains("genesis hash mismatch"),
        "the mismatch branch no longer refuses to start"
    );
}
