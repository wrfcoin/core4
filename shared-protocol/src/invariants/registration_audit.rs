//! Registration-completeness audit for the invariant harness (#2100, #2260).
//!
//! # The failure mode this exists for
//!
//! [`InvariantHarness`] guarantees that everything **registered** runs after
//! every applied block. It cannot, by itself, notice that something was never
//! registered — and a registry that fails to notice an omission reports a
//! healthy system while a safety predicate runs nowhere.
//!
//! That is not hypothetical. Sibling #2250 (#2097, global supply
//! conservation) adds `SupplyLedger` and `check_supply` to
//! [`super::balance`] and wires them into block finalization, but adds no
//! [`Invariant`](super::harness::Invariant) implementation and no harness
//! registration. Every harness test stays green, the supply identifier never
//! appears in `ids()`, and the harness's own report says everything is fine.
//! The regression test in this module is that exact scenario.
//!
//! (That identifier is deliberately not spelled out anywhere in this file:
//! this file is itself inside the scanned tree, so writing it here would make
//! the shipped audit demand that #2250's invariant be registered by #2260.
//! The tests assemble the identifiers they need at runtime. The audit caught
//! this module's own first draft doing it wrong, which is the over-counting
//! direction working as intended.)
//!
//! # How completeness is enforced
//!
//! Rust has no runtime reflection over trait implementations, so "did you
//! register everything?" cannot be answered from the type system without a
//! link-section registry crate (see "Rejected designs" below). What *can* be
//! done is to derive the expected set from the source tree instead of
//! hand-listing it — the same discipline #2278 applied to its ratchet, where
//! three consecutive defects all came from a hand-maintained list:
//!
//! 1. Scan the crate source for declared invariant identifiers — the
//!    `INV-<DOMAIN>-<n>` token, wherever it appears.
//! 2. Require every declared identifier to be either registered in
//!    [`InvariantHarness::with_defaults`] or listed in
//!    [`REGISTRATION_WAIVERS`] with a stated reason.
//! 3. Reject waivers for identifiers that are no longer declared anywhere, so
//!    the waiver list cannot rot into a permanent exemption.
//!
//! Writing a predicate and forgetting to register it therefore fails a test
//! rather than shipping silently. The author's two exits are both explicit:
//! register it, or waive it on the record.
//!
//! # Counting direction
//!
//! The scan deliberately does **not** exclude `#[cfg(test)]` modules, doc
//! comments or ordinary comments. A mention is treated as a declaration.
//! That over-counts — a doc comment naming a not-yet-written invariant trips
//! the audit — and over-counting costs a waiver line, while under-counting
//! costs exactly the silent omission this module exists to prevent. #2278 set
//! this standard on the unwrap ratchet and it is the same trade here.
//!
//! # Known bounds, stated rather than assumed
//!
//! * The scan is textual. An invariant whose identifier never appears in
//!   `shared-protocol/src` — declared in another crate, or assembled at
//!   runtime from fragments — is not seen. Identifiers are written as
//!   literals in every current invariant, and [`super::harness`] requires the
//!   `INV-<DOMAIN>-<n>` form, so this is a convention the audit enforces
//!   rather than a hole it ignores.
//! * The audit proves an identifier is *registered*, not that its predicate
//!   is *correct* or that its `check` body is non-trivial. `with_defaults`
//!   running it is what the harness tests cover.
//! * It runs as a test, so it gates CI and local runs, not compilation.
//!
//! # Rejected designs
//!
//! A distributed slice (`linkme`, `inventory`) would let each module register
//! itself with no central list at all, which would solve completeness and the
//! shared-hunk problem together. It was not adopted here: both crates rely on
//! link-section tricks that vary by platform and linker, and this is the
//! consensus crate — a silently-empty slice under an unusual linker would
//! reintroduce exactly this bug with no test able to see it. That trade is
//! worth revisiting deliberately, not as a side effect of this PR.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Why a declared identifier is not in the harness.
///
/// Categorised rather than prose-only so the waiver list stays readable as it
/// grows: a reader can tell at a glance which entries are harmless fixtures
/// and which are real invariants living somewhere the harness cannot see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaiverKind {
    /// Defined inside a `#[cfg(test)]` module; never a shipped predicate.
    TestFixture,
    /// A real, shipped invariant registered into a different registry —
    /// [`StatefulInvariantRegistry`](super::StatefulInvariantRegistry) — which
    /// the harness does not and should not own.
    OtherRegistry,
}

/// An identifier that is knowingly declared but not registered in the harness.
///
/// Every field is required: a waiver without a reason is how an omission
/// becomes permanent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistrationWaiver {
    /// The bare token, e.g. `"INV-EXAMPLE-1"` — not the full titled id.
    pub id: &'static str,
    /// Which category of exemption this is.
    pub kind: WaiverKind,
    /// Why this identifier is not registered here. Stated, not implied.
    pub reason: &'static str,
}

/// Identifiers that appear in the source but are deliberately not registered
/// in the harness.
///
/// Keep this list short and keep the reasons true. Every [`WaiverKind::OtherRegistry`]
/// entry is an invariant whose registration this audit does **not** cover.
pub const REGISTRATION_WAIVERS: &[RegistrationWaiver] = &[
    RegistrationWaiver {
        id: "INV-EXAMPLE-1",
        kind: WaiverKind::TestFixture,
        reason: "fixture in invariants::harness::tests demonstrating the \
                 downstream-epic extension path; defined inside a #[cfg(test)] \
                 module and never shipped",
    },
    RegistrationWaiver {
        id: "INV-A-1",
        kind: WaiverKind::TestFixture,
        reason: "AlwaysBroken fixture in invariants::finality::tests, used to \
                 prove StatefulInvariantRegistry::check_all aggregates rather \
                 than short-circuits",
    },
    RegistrationWaiver {
        id: "INV-B-1",
        kind: WaiverKind::TestFixture,
        reason: "second AlwaysBroken fixture alongside INV-A-1, same test",
    },
    RegistrationWaiver {
        id: "INV-SUPPLY-1",
        kind: WaiverKind::OtherRegistry,
        reason: "a StatefulInvariant, not an Invariant: the supply identity is a \
                 property of the issuance journal versus the summed stores, which \
                 no single-State predicate can express. Deliberately NOT \
                 registered in the harness: the harness gates block validity, and \
                 gating consensus on the issuance journal — which is not in \
                 state_root_material and is not snapshotted — would let two \
                 honest nodes with the same committed state root disagree about \
                 validity (#2250 review). It lives in StatefulInvariantRegistry, \
                 and its registration there is asserted by launch_api's \
                 stateful_invariants module, not by this waiver: \
                 build_shipped_registry is the node's only registry \
                 constructor, assert_shipped_registry_complete refuses to boot \
                 without INV-SUPPLY-1 in it, and \
                 shipped_registry_arms_every_required_invariant plus \
                 an_unjournalled_mint_reaches_the_shipped_registry test that \
                 constructor. THIS AUDIT STILL DOES NOT VERIFY ANY OF THAT — it \
                 only records where the verification lives",
    },
    RegistrationWaiver {
        id: "INV-FINALITY-1",
        kind: WaiverKind::OtherRegistry,
        reason: "a StatefulInvariant, not an Invariant: it tracks history no \
                 single State records, so the harness cannot express it. It \
                 registers into StatefulInvariantRegistry at the launch_api call \
                 site. A with_defaults() on that registry would be actively \
                 wrong, because the instance must be the same one fed at its \
                 event site; a freshly-constructed one would poll a tracker \
                 nobody feeds and pass forever. Instead launch_api's \
                 stateful_invariants::build_shipped_registry takes the live \
                 handles by reference and the boot gate refuses to start without \
                 this id (#2097) — outside this crate, so this audit still does \
                 not verify it",
    },
];

/// The directory the audit scans by default: this crate's `src`.
///
/// Resolved from `CARGO_MANIFEST_DIR`, so it is correct under `cargo test`
/// regardless of the working directory the test runner was launched from.
pub fn default_scan_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Extract the invariant identifier token starting at `bytes[i]`, if one is
/// there.
///
/// Matches `INV-<DOMAIN>-<n>` where `<DOMAIN>` is one or more uppercase
/// alphanumerics and `<n>` is one or more digits. Returns the token and the
/// index just past it. Hand-rolled rather than pulling in a regex dependency
/// for one pattern in one test-time helper.
fn token_at(bytes: &[u8], i: usize) -> Option<(String, usize)> {
    const PREFIX: &[u8] = b"INV-";
    if !bytes[i..].starts_with(PREFIX) {
        return None;
    }
    // A token must start on a word boundary, so `XINV-BAL-1` is not one.
    if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        return None;
    }

    let mut j = i + PREFIX.len();
    let domain_start = j;
    while j < bytes.len() && (bytes[j].is_ascii_uppercase() || bytes[j].is_ascii_digit()) {
        j += 1;
    }
    if j == domain_start || j >= bytes.len() || bytes[j] != b'-' {
        return None;
    }
    j += 1;

    let number_start = j;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
    }
    if j == number_start {
        return None;
    }

    Some((String::from_utf8_lossy(&bytes[i..j]).into_owned(), j))
}

/// Every invariant identifier declared in `text`.
pub fn ids_in_text(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut found = BTreeSet::new();
    let mut i = 0;
    while i < bytes.len() {
        match token_at(bytes, i) {
            Some((id, next)) => {
                found.insert(id);
                i = next;
            }
            None => i += 1,
        }
    }
    found
}

/// Every invariant identifier declared under `root`, mapped to the files that
/// declare it, so a failure can name the file the author must look at.
pub fn scan_declared_ids(root: &Path) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let mut declared: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("cannot read entry in {}: {e}", dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                let shown = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                for id in ids_in_text(&text) {
                    declared.entry(id).or_default().insert(shown.clone());
                }
            }
        }
    }

    if declared.is_empty() {
        return Err(format!(
            "no invariant identifiers found under {} — the audit would pass \
             vacuously, which is indistinguishable from a working audit. \
             Check the scan root.",
            root.display()
        ));
    }
    Ok(declared)
}

/// Reduce a registered id (`"INV-BAL-2: Balance Accounting Identity"`) to its
/// token (`"INV-BAL-2"`).
fn token_of(registered_id: &str) -> Option<String> {
    let bytes = registered_id.as_bytes();
    (0..bytes.len()).find_map(|i| token_at(bytes, i).map(|(id, _)| id))
}

/// Assert that every declared invariant identifier is registered or waived.
///
/// `registered_ids` is normally [`InvariantHarness::ids`](super::harness::InvariantHarness::ids).
pub fn audit_registration(root: &Path, registered_ids: &[&str]) -> Result<(), String> {
    let declared = scan_declared_ids(root)?;

    let registered: BTreeSet<String> = registered_ids.iter().filter_map(|id| token_of(id)).collect();

    let mut problems: Vec<String> = Vec::new();

    for waiver in REGISTRATION_WAIVERS {
        if waiver.reason.trim().is_empty() {
            problems.push(format!(
                "waiver for {} states no reason — an unexplained waiver is how \
                 an omission becomes permanent",
                waiver.id
            ));
        }
        if !declared.contains_key(waiver.id) {
            problems.push(format!(
                "waiver for {} is stale: that identifier is no longer declared \
                 anywhere under {}. Remove the waiver.",
                waiver.id,
                root.display()
            ));
        }
        if registered.contains(waiver.id) {
            problems.push(format!(
                "waiver for {} is obsolete: it IS registered now. Remove the waiver.",
                waiver.id
            ));
        }
    }

    let waived: BTreeSet<&str> = REGISTRATION_WAIVERS.iter().map(|w| w.id).collect();
    for (id, files) in &declared {
        if registered.contains(id) || waived.contains(id.as_str()) {
            continue;
        }
        let where_ = files.iter().cloned().collect::<Vec<_>>().join(", ");
        problems.push(format!(
            "{id} is declared in {where_} but is registered nowhere: \
             InvariantHarness::with_defaults() will never run it, and every \
             harness test will still pass. Register it from its domain \
             module's register_invariants() hook; or, if it is a \
             StatefulInvariant belonging to StatefulInvariantRegistry, add it \
             to REGISTRATION_WAIVERS as WaiverKind::OtherRegistry naming where \
             it registers instead."
        ));
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invariants::harness::InvariantHarness;

    /// A scratch directory, without taking a `tempfile` dev-dependency for
    /// three tests. Named per-test so concurrent test threads cannot collide.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "wrfcoin-registration-audit-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Self(dir)
        }

        fn write(&self, file: &str, contents: &str) -> &Self {
            std::fs::write(self.0.join(file), contents).expect("write fixture");
            self
        }

        /// Declare every shipped waiver's identifier, so a scratch tree does
        /// not trip the stale-waiver check while testing something else.
        fn with_shipped_waivers(&self) -> &Self {
            let decls: String = REGISTRATION_WAIVERS
                .iter()
                .map(|w| format!("// {}\n", w.id))
                .collect();
            self.write("waived_fixtures.rs", &decls)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Assemble an identifier at runtime.
    ///
    /// Never write one as a literal in this file: it is itself inside the
    /// scanned tree, so a literal here becomes a real declaration that the
    /// shipped audit will then demand be registered.
    fn fake_id(domain: &str, n: u32) -> String {
        format!("INV-{domain}-{n}")
    }

    /// The identifier used as the "written but not registered" fixture.
    ///
    /// This was INV-SUPPLY-1, chosen because #2250's supply invariant was
    /// genuinely unregistered at the time. That coupled the fixture to a real
    /// invariant's registration status, and when #2250 landed and INV-SUPPLY-1
    /// was waived, the fixture stopped being unregistered and these tests
    /// failed for a reason that had nothing to do with what they assert.
    ///
    /// A fixture must not depend on the live state of the thing it models. The
    /// replacement uses a FICTIONAL domain that belongs to no module and never
    /// will, so nobody can invalidate it by registering it.
    ///
    /// (Its identifier is assembled, not spelled — writing it literally here
    /// would make this file declare it, and the shipped audit would then demand
    /// it be registered. That is the trap described in the module docs above,
    /// and it caught this very edit.)
    fn unregistered_fixture_id() -> String {
        fake_id("FIXTURE", 1)
    }

    // -- the shipped state -------------------------------------------------

    /// The real audit over the real source tree.
    #[test]
    fn shipped_registration_is_complete() {
        let harness = InvariantHarness::with_defaults();
        let ids = harness.ids();
        audit_registration(&default_scan_root(), &ids)
            .expect("every declared invariant must be registered or waived");
    }

    /// The scan must actually find things — a vacuous pass looks identical to
    /// a working audit, which is the bug class this module is about.
    #[test]
    fn the_scan_finds_the_shipped_identifiers() {
        let declared = scan_declared_ids(&default_scan_root()).expect("scan");
        for expected in [
            "INV-BAL-1",
            "INV-BAL-2",
            "INV-BAL-3",
            "INV-POP-1",
            "INV-RWD-1",
            "INV-RWD-2",
            "INV-SLASH-1",
        ] {
            assert!(
                declared.contains_key(expected),
                "{expected} not found by the scan; declared: {:?}",
                declared.keys().collect::<Vec<_>>()
            );
        }
    }

    // -- the regression the reviewer asked for -----------------------------

    /// **The #2250 scenario, verbatim in shape.**
    ///
    /// A domain module grows a supply-conservation predicate with a real
    /// identifier, a ledger type and a `check_supply` function — and no
    /// harness registration. Before this audit existed, every harness test
    /// stayed green and the predicate ran nowhere. It must now fail, and the
    /// failure must name the identifier and the file.
    #[test]
    fn an_unregistered_domain_predicate_cannot_silently_ship() {
        let id = unregistered_fixture_id();
        let scratch = Scratch::new("unregistered");
        scratch.with_shipped_waivers();
        scratch.write(
            "balance.rs",
            &format!(
                r#"
/// {id} — global supply conservation (#2097).
pub struct SupplyLedger {{
    pub issued: u128,
    pub held: u128,
}}

pub fn check_supply(ledger: &SupplyLedger) -> Result<(), String> {{
    if ledger.issued != ledger.held {{
        return Err(format!("{id} violated: issued != held"));
    }}
    Ok(())
}}
"#
            ),
        );

        // The harness is entirely healthy — this is the point. Its own tests
        // pass, its ids() are all registered, and it still cannot see the
        // omission without the audit.
        let harness = InvariantHarness::with_defaults();
        assert_eq!(
            harness.check_all(&crate::blockchain::state::State::new()),
            Ok(()),
            "the harness must be healthy; the omission is invisible to it"
        );
        assert!(
            !harness.ids().iter().any(|i| i.contains(&id)),
            "{id} must not be registered — that is the scenario"
        );

        let err = audit_registration(scratch.path(), &harness.ids())
            .expect_err("an unregistered domain predicate must fail the audit");
        assert!(err.contains(&id), "failure must name the identifier: {err}");
        assert!(err.contains("balance.rs"), "failure must name the file: {err}");
        assert!(
            err.contains("registered nowhere"),
            "failure must say what is wrong: {err}"
        );
    }

    /// The same predicate, once registered, passes — so the audit is
    /// discriminating rather than simply always red.
    #[test]
    fn registering_the_omitted_predicate_satisfies_the_audit() {
        let id = unregistered_fixture_id();
        let scratch = Scratch::new("registered");
        scratch.with_shipped_waivers();
        scratch.write("balance.rs", &format!("/// {id} — global supply conservation\n"));

        let titled = format!("{id}: Global Supply Conservation");
        let mut ids = InvariantHarness::with_defaults().ids();
        ids.push(Box::leak(titled.into_boxed_str()));

        audit_registration(scratch.path(), &ids)
            .expect("a registered predicate must satisfy the audit");
    }

    // -- the waiver escape hatch, and its limits ---------------------------

    #[test]
    fn a_waived_identifier_is_accepted() {
        let scratch = Scratch::new("waived");
        // Declare every shipped waiver's identifier and nothing else.
        scratch.with_shipped_waivers();
        audit_registration(scratch.path(), &["INV-BAL-2: Balance Accounting Identity"])
            .expect("a waived identifier must be accepted");
    }

    #[test]
    fn a_stale_waiver_is_rejected() {
        let scratch = Scratch::new("stale");
        // Declares something else entirely, so every shipped waiver is stale.
        scratch.write("other.rs", "// INV-BAL-2 only\n");
        let err = audit_registration(scratch.path(), &["INV-BAL-2: x"])
            .expect_err("a waiver for an undeclared identifier must be rejected");
        assert!(err.contains("stale"), "got: {err}");
        assert!(err.contains("INV-EXAMPLE-1"), "got: {err}");
    }

    #[test]
    fn an_empty_scan_root_fails_rather_than_passing_vacuously() {
        let scratch = Scratch::new("empty");
        let err = scan_declared_ids(scratch.path())
            .expect_err("an empty tree must not pass vacuously");
        assert!(err.contains("vacuously"), "got: {err}");
    }

    // -- the tokenizer, in the over-counting direction ---------------------

    #[test]
    fn identifiers_are_found_in_comments_and_test_modules_too() {
        // Over-counting on purpose: a mention anywhere is a declaration.
        // Assembled, not literal — see the module docs.
        let id = fake_id("FOO", 3);
        assert!(ids_in_text(&format!("// see {id} for details")).contains(&id));
        assert!(ids_in_text(&format!("/// {id} does a thing")).contains(&id));
        assert!(
            ids_in_text(&format!(
                "#[cfg(test)] mod t {{ const X: &str = \"{id}: T\"; }}"
            ))
            .contains(&id)
        );
    }

    #[test]
    fn the_tokenizer_rejects_near_misses() {
        for text in [
            "INV-",
            "INV-BAL",
            "INV-BAL-",
            "INV-bal-1", // domain must be uppercase
            "XINV-BAL-1",
            "myINV-BAL-1",
        ] {
            assert!(
                ids_in_text(text).is_empty(),
                "{text:?} must not tokenize as an invariant id, got {:?}",
                ids_in_text(text)
            );
        }
        let multi_digit = fake_id("BAL", 10);
        assert_eq!(
            ids_in_text(&format!("{multi_digit}: Title"))
                .into_iter()
                .collect::<Vec<_>>(),
            vec![multi_digit],
            "multi-digit numbers must tokenize"
        );
    }

    #[test]
    fn every_shipped_waiver_states_a_reason() {
        for waiver in REGISTRATION_WAIVERS {
            assert!(
                !waiver.reason.trim().is_empty(),
                "waiver {} must state a reason",
                waiver.id
            );
        }
    }

    /// A `TestFixture` waiver is cheap; an `OtherRegistry` waiver means a real
    /// invariant's registration is not covered by this audit, so it must say
    /// where that invariant actually registers. Otherwise the categories
    /// collapse and the list stops being readable at a glance.
    #[test]
    fn every_other_registry_waiver_names_its_registry() {
        for waiver in REGISTRATION_WAIVERS
            .iter()
            .filter(|w| w.kind == WaiverKind::OtherRegistry)
        {
            assert!(
                waiver.reason.contains("Registry"),
                "waiver {} is WaiverKind::OtherRegistry but its reason does not \
                 name the registry it lives in: {}",
                waiver.id,
                waiver.reason
            );
        }
    }
}
