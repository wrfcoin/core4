//! Adversarial crypto vector harness (#2126).
//!
//! # Three sources, never conflated
//!
//! Review of #2236 made the point that a harness labelling everything
//! `[wycheproof:*]` overstates its provenance. Each source is now named:
//!
//! | Label | Source | Path |
//! |---|---|---|
//! | `[vectors:ed25519-local-controls]` | **Project-authored**, generated with `python-cryptography` | `fixtures/wycheproof/local/` |
//! | `[vectors:ed25519-wycheproof-upstream]` | **Published Project Wycheproof** corpus (#2264, vendored by #2282) | `fixtures/wycheproof/upstream/` |
//! | `[vectors:hash-nist]` | **NIST** FIPS 180-4 / 202 published messages | `fixtures/nist/` |
//!
//! The local controls are not a substitute waiting to be deleted: their
//! `malleable_S_plus_L_*` and `identity_public_key` cases target this project's
//! `verify_strict` behaviour (#2043) and were built deliberately. They are a
//! complement to the upstream corpus, not a competitor.
//!
//! The upstream corpus is **optional at compile and run time**. If
//! `fixtures/wycheproof/upstream/ed25519_test.json` is absent, that is recorded
//! in the coverage ledger as an unrun class rather than silently passing. So
//! this harness is correct whether or not #2282 has landed.
//!
//! # What this tests, and what it deliberately does not
//!
//! Wycheproof exists to catch implementation errors that ordinary unit tests
//! miss — malleable signatures, non-canonical encodings, identity-point keys.
//! The question it answers is not "is ed25519 correct" (upstream's job) but
//! **"does *this codebase* use it correctly."** So every vector is driven
//! through the project's own wrappers:
//!
//! - ed25519 → `crypto::signature::PublicKey::verify_signature`
//! - sha256  → `crypto::hash::sha256_hash`
//! - sha3    → `crypto::hash::sha3_hash`
//!
//! Never through `ed25519_dalek` / `sha2` / `sha3` directly. Testing the
//! upstream crate would prove the crate works while saying nothing about
//! whether this codebase invokes it safely.
//!
//! # Vendored, not fetched
//!
//! Vectors live in `tests/fixtures/`. A test that fetches vectors at
//! run time is a flaky test — it fails when the network does. They are checked
//! in and read with `std::fs`.
//!
//! # Why ed25519 and the hashes, but not k256
//!
//! The story lists ed25519, k256 ECDSA, sha2 and sha3. Three of those have a
//! project wrapper in `signature.rs` / `hash.rs`. **k256 does not** — there is
//! no k256 wrapper anywhere in `shared-protocol` (it is a dependency of other
//! crates, used directly). The story is explicit that the harness must test the
//! project's wrappers and *not* the upstream crate, so adding a k256 dependency
//! here purely to test upstream would violate that. Recorded as
//! [`K256_NO_PROJECT_WRAPPER`] rather than papered over. See the PR body.
//!
//! # The `acceptable` flag
//!
//! Wycheproof marks borderline-but-tolerated vectors `acceptable`. Per the
//! story these route through an explicit project decision recorded in a
//! constant ([`ACCEPTABLE_POLICY`]) rather than an ad-hoc skip. WRFCoin's
//! ed25519 wrapper uses `verify_strict` (#2043), which rejects the malleable
//! and non-canonical cases Wycheproof tends to mark `acceptable` — so for this
//! codebase `acceptable` collapses to `invalid`.

use serde::Deserialize;
use wrfcoin_shared_protocol::crypto::hash::{sha3_hash, sha256_hash};
use wrfcoin_shared_protocol::crypto::signature::{PublicKey, Signature};

/// k256 ECDSA has no wrapper in `shared-protocol`, so there is nothing here to
/// test without importing the upstream crate directly — which the story
/// forbids. Tracked rather than silently omitted.
pub const K256_NO_PROJECT_WRAPPER: &str = "core4#2126: no k256 wrapper in shared-protocol; \
    add a project wrapper (or fold k256 vectors in wherever the wrapper lands) before covering it";

/// Algorithm classes this harness does **not** execute, with the reason.
///
/// Review of PR #2236 made the point that matters: a harness printing a pass
/// count while silently skipping a whole class **overstates what it verified**.
/// A constant nobody reads is not accounting. These are emitted in the summary
/// on every run and reconciled by
/// [`coverage_ledger_is_complete_and_reconciles`], so a class cannot be dropped
/// without the ledger noticing.
/// Path of the published Project Wycheproof ed25519 corpus, vendored by #2282.
///
/// **Optional.** When absent, the harness records it as an unrun class rather
/// than passing quietly, so this file is correct whether or not #2282 has
/// landed. When present, its vectors execute through the same project wrapper
/// as everything else.
pub const UPSTREAM_ED25519: &str = "wycheproof/upstream/ed25519_test.json";

/// Project-authored adversarial controls. Not the published corpus, and named
/// so nobody can mistake them for it.
pub const LOCAL_ED25519: &str = "wycheproof/local/ed25519_project_controls.json";

/// NIST FIPS 180-4 / FIPS 202 published messages. Not Wycheproof at all, and no
/// longer filed under a `wycheproof/` path.
pub const NIST_HASH: &str = "nist/sha2_sha3_fips.json";

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new("tests/fixtures").join(name)
}

/// Whether the vendored upstream corpus is present in this checkout.
fn upstream_available() -> bool {
    fixture_path(UPSTREAM_ED25519).is_file()
}

/// Algorithm classes this harness does **not** execute, with the reason.
///
/// Review of PR #2236 made the point that matters: a harness printing a pass
/// count while silently skipping a whole class **overstates what it verified**.
/// A constant nobody reads is not accounting. These are emitted in the summary
/// on every run and reconciled by
/// [`coverage_ledger_is_complete_and_reconciles`], so a class cannot be dropped
/// without the ledger noticing.
///
/// Computed rather than `const`, because whether the published corpus is a gap
/// depends on whether it is actually vendored in this checkout. A hard-coded
/// row would go stale the moment #2282 lands — which is the same
/// labels-versus-reality problem this ledger exists to prevent.
pub fn unsupported_classes() -> Vec<(String, String)> {
    let mut rows = vec![(
        "k256/ECDSA".to_string(),
        "no k256 wrapper exists in shared-protocol; the story forbids testing the \
         upstream crate directly, so there is no project surface to drive (core4#2126). \
         The vendored secp256k1 corpora from #2282 stay unrun for the same reason"
            .to_string(),
    )];

    if !upstream_available() {
        rows.push((
            "ed25519 (published Wycheproof corpus)".to_string(),
            format!(
                "{UPSTREAM_ED25519} is not present in this checkout, so only the \
                 project-authored local controls ran. Vendored by core4#2282; \
                 tracked as core4#2264"
            ),
        ));
    }

    if !any_corpus_has_acceptable() {
        rows.push((
            "acceptable-flagged vectors".to_string(),
            "no available corpus contains an `acceptable` case, so ACCEPTABLE_POLICY \
             is defined but not exercised by any vector"
                .to_string(),
        ));
    }

    rows
}

/// True if any corpus actually present carries an `acceptable` result flag.
fn any_corpus_has_acceptable() -> bool {
    let mut files = vec![LOCAL_ED25519, NIST_HASH];
    if upstream_available() {
        files.push(UPSTREAM_ED25519);
    }
    files
        .iter()
        .any(|f| fixture(f).contains("\"acceptable\""))
}

/// Project decision for Wycheproof `acceptable`-flagged vectors: treat as
/// `invalid`. WRFCoin verifies ed25519 with `verify_strict` (#2043), which
/// already rejects the malleable / non-canonical encodings that flag covers.
pub const ACCEPTABLE_POLICY: ExpectedResult = ExpectedResult::Invalid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedResult {
    Valid,
    Invalid,
}

impl ExpectedResult {
    fn parse(raw: &str) -> Self {
        match raw {
            "valid" => Self::Valid,
            "invalid" => Self::Invalid,
            "acceptable" => ACCEPTABLE_POLICY,
            other => panic!("unknown result flag {other:?} in a vendored vector"),
        }
    }
}

fn fixture(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing vendored vector file {}: {e}", path.display()))
}

/// Running tally so a genuine failure is reported as a security issue and
/// skipped vectors are surfaced, per the story's summary-artifact requirement.
#[derive(Default)]
struct Summary {
    passed: usize,
    skipped: Vec<String>,
    failures: Vec<String>,
}

impl Summary {
    fn record(&mut self, tc: &str, ok: bool, detail: impl FnOnce() -> String) {
        if ok {
            self.passed += 1;
        } else {
            self.failures.push(format!("{tc}: {}", detail()));
        }
    }

    fn finish(self, algorithm: &str) {
        // The summary artifact: printed with `--nocapture`, and asserted on.
        // `total` is stated alongside `passed` so the two can be reconciled at a
        // glance — a pass count on its own does not say what was not run.
        let total = self.passed + self.skipped.len() + self.failures.len();
        eprintln!(
            "[vectors:{algorithm}] total={total} passed={} skipped={} failed={}",
            self.passed,
            self.skipped.len(),
            self.failures.len()
        );
        for s in &self.skipped {
            eprintln!("[vectors:{algorithm}] SKIP {s}");
        }
        assert!(
            self.failures.is_empty(),
            "{algorithm} vector failures (each is a SECURITY finding — file it, do not \
             delete the vector):\n{}",
            self.failures.join("\n")
        );
        assert!(self.passed > 0, "no {algorithm} vectors ran");
    }

    /// Emit the classes this harness does not cover at all.
    ///
    /// Printed by every algorithm's summary so a reader of the CI log sees the
    /// gaps next to the pass counts, not only in a source constant.
    fn report_unsupported() {
        let rows = unsupported_classes();
        eprintln!(
            "[vectors:coverage] {} unsupported class(es) — NOT verified by this harness:",
            rows.len()
        );
        for (class, why) in &rows {
            eprintln!("[vectors:coverage]   SKIP {class}: {why}");
        }
        eprintln!(
            "[vectors:coverage] upstream Wycheproof corpus present: {}",
            upstream_available()
        );
    }
}

// ---------------------------------------------------------------------------
// ed25519
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Ed25519File {
    tests: Vec<Ed25519Vector>,
}

#[derive(Deserialize)]
struct Ed25519Vector {
    #[serde(rename = "tcId")]
    tc_id: String,
    public_key_hex: String,
    msg_hex: String,
    sig_hex: String,
    result: String,
    #[allow(dead_code)]
    comment: String,
}

#[test]
fn ed25519_wrapper_matches_vectors() {
    let file: Ed25519File =
        serde_json::from_str(&fixture(LOCAL_ED25519)).expect("local ed25519 controls must parse");
    let mut summary = Summary::default();

    for v in &file.tests {
        let expected = ExpectedResult::parse(&v.result);
        let msg = hex::decode(&v.msg_hex).expect("msg_hex");
        let pk_bytes = hex::decode(&v.public_key_hex).expect("public_key_hex");
        let sig_bytes = hex::decode(&v.sig_hex).expect("sig_hex");

        // Malformed public keys are themselves a rejection case: the wrapper
        // must decline, never panic. A vector whose key does not parse can only
        // be `invalid`.
        let Ok(pk) = PublicKey::from_bytes(&pk_bytes) else {
            summary.record(&v.tc_id, expected == ExpectedResult::Invalid, || {
                "public key failed to parse but vector expected valid".into()
            });
            continue;
        };

        // Drive BOTH wrapper entry points — the byte API and the typed API —
        // since a bug can live in either.
        let via_bytes = pk.verify_signature(&msg, &sig_bytes);
        let via_typed = Signature::from_bytes(&sig_bytes)
            .map(|sig| pk.verify(&msg, &sig))
            .unwrap_or(false);

        let actual = if via_bytes {
            ExpectedResult::Valid
        } else {
            ExpectedResult::Invalid
        };

        summary.record(
            &v.tc_id,
            actual == expected && via_bytes == via_typed,
            || {
                format!(
                    "expected {expected:?}, got via_bytes={via_bytes} via_typed={via_typed} ({})",
                    v.comment
                )
            },
        );
    }

    summary.finish("ed25519-local-controls");
}

#[test]
fn ed25519_strict_rejects_malleable_s_plus_l() {
    // The single most important adversarial case, called out by name so a
    // regression is greppable. verify_strict (#2043) must reject S' = S + L.
    let file: Ed25519File =
        serde_json::from_str(&fixture(LOCAL_ED25519)).expect("local ed25519 controls must parse");
    let malleable: Vec<_> = file
        .tests
        .iter()
        .filter(|v| v.tc_id.starts_with("malleable_S_plus_L"))
        .collect();
    assert!(
        !malleable.is_empty(),
        "the malleability vectors are missing from the corpus"
    );

    for v in malleable {
        let pk = PublicKey::from_bytes(&hex::decode(&v.public_key_hex).unwrap()).unwrap();
        let msg = hex::decode(&v.msg_hex).unwrap();
        let sig = hex::decode(&v.sig_hex).unwrap();
        assert!(
            !pk.verify_signature(&msg, &sig),
            "{}: a non-canonical S = S+L signature was accepted — malleability regression",
            v.tc_id
        );
    }
}

#[test]
fn ed25519_identity_public_key_does_not_verify() {
    let file: Ed25519File =
        serde_json::from_str(&fixture(LOCAL_ED25519)).expect("local ed25519 controls must parse");
    let identity: Vec<_> = file
        .tests
        .iter()
        .filter(|v| v.tc_id.contains("identity"))
        .collect();
    assert!(!identity.is_empty(), "identity-key vector missing");

    for v in identity {
        let msg = hex::decode(&v.msg_hex).unwrap();
        let sig = hex::decode(&v.sig_hex).unwrap();
        // The identity point is a small-order key. Whether it parses or not, it
        // must not verify an unrelated signature.
        if let Ok(pk) = PublicKey::from_bytes(&hex::decode(&v.public_key_hex).unwrap()) {
            assert!(
                !pk.verify_signature(&msg, &sig),
                "identity public key verified a signature — small-order key attack surface"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// hashes (sha256, sha3-256)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct HashFile {
    tests: Vec<HashVector>,
}

#[derive(Deserialize)]
struct HashVector {
    #[serde(rename = "tcId")]
    tc_id: String,
    algo: String,
    msg_hex: String,
    expected_hex: String,
    result: String,
}

#[test]
fn hash_wrappers_match_nist_vectors() {
    let file: HashFile = serde_json::from_str(&fixture(NIST_HASH)).expect("NIST hash vectors must parse");
    let mut summary = Summary::default();

    for v in &file.tests {
        let expected = ExpectedResult::parse(&v.result);
        let msg = hex::decode(&v.msg_hex).expect("msg_hex");

        let digest_hex = match v.algo.as_str() {
            "sha256" => hex::encode(sha256_hash(&msg).as_bytes()),
            "sha3_256" => hex::encode(sha3_hash(&msg).as_bytes()),
            other => {
                summary
                    .skipped
                    .push(format!("{}: unknown algo {other}", v.tc_id));
                continue;
            },
        };

        let matches = digest_hex == v.expected_hex;
        // A `valid` vector must match; an `invalid` vector (deliberately wrong
        // expected) must NOT match — which proves the comparison is live.
        let ok = match expected {
            ExpectedResult::Valid => matches,
            ExpectedResult::Invalid => !matches,
        };
        summary.record(&v.tc_id, ok, || {
            format!(
                "algo={} expected_hex={} got={digest_hex} (result flag {})",
                v.algo, v.expected_hex, v.result
            )
        });
    }

    summary.finish("hash-nist");
}

#[test]
fn coverage_ledger_is_complete_and_reconciles() {
    // Replaces the previous `k256_scope_is_recorded`, which only asserted that a
    // prose constant contained "2126". That was not coverage accounting: it
    // executed no vectors and emitted no skip count, so the harness could report
    // a clean pass while an entire algorithm class went unrun.
    Summary::report_unsupported();

    // Every unsupported class carries a real reason.
    for (class, why) in &unsupported_classes() {
        assert!(
            why.len() > 30,
            "unsupported class {class} has no meaningful reason: {why:?}"
        );
    }

    // k256 must be named explicitly — it is the class the story asked for and
    // this harness cannot provide.
    assert!(
        unsupported_classes()
            .iter()
            .any(|(class, _)| class.contains("k256")),
        "k256 must be accounted for in the coverage ledger, not only in a constant"
    );
    assert!(K256_NO_PROJECT_WRAPPER.contains("2126"));

    // ACCEPTABLE_POLICY is declared. Whether any vector exercises it depends on
    // which corpora are present, so the ledger must agree with what is on disk
    // rather than with a hard-coded expectation.
    assert_eq!(ACCEPTABLE_POLICY, ExpectedResult::Invalid);
    let has_acceptable = any_corpus_has_acceptable();
    let ledger_says_unexercised = unsupported_classes()
        .iter()
        .any(|(class, _)| class.contains("acceptable"));
    assert_eq!(
        has_acceptable, !ledger_says_unexercised,
        "the `acceptable` ledger row must track the corpora actually present: \
         corpus_has_acceptable={has_acceptable} ledger_says_unexercised={ledger_says_unexercised}"
    );

    // Same for the published corpus: it is a gap only while it is absent.
    let upstream_row = unsupported_classes()
        .iter()
        .any(|(class, _)| class.contains("published Wycheproof corpus"));
    assert_eq!(
        upstream_available(), !upstream_row,
        "the published-corpus ledger row must track whether {UPSTREAM_ED25519} is \
         actually vendored — this is the labels-versus-reality check that #2236 \
         review asked for"
    );
}

// ---------------------------------------------------------------------------
// Published Project Wycheproof corpus (optional — vendored by #2282)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct UpstreamFile {
    /// The corpus's own declared size. The harness reconciles what it actually
    /// executed against this (#2264 requirement: "failing if they disagree") —
    /// a re-pin whose schema drift silently drops groups must fail loudly, not
    /// shrink the pass count while staying green.
    #[serde(rename = "numberOfTests")]
    number_of_tests: usize,
    #[serde(rename = "testGroups")]
    test_groups: Vec<UpstreamGroup>,
}

#[derive(Deserialize)]
struct UpstreamGroup {
    #[serde(rename = "publicKey")]
    public_key: UpstreamKey,
    tests: Vec<UpstreamVector>,
}

#[derive(Deserialize)]
struct UpstreamKey {
    pk: String,
}

#[derive(Deserialize)]
struct UpstreamVector {
    #[serde(rename = "tcId")]
    tc_id: u32,
    comment: String,
    msg: String,
    sig: String,
    result: String,
    /// Upstream's classification tags (e.g. `SignatureMalleability`,
    /// `InvalidEncoding`). Carried into failure output so a red vector names
    /// the attack class it represents without a manual corpus lookup.
    #[serde(default)]
    flags: Vec<String>,
}

/// Drive the **published** Project Wycheproof ed25519 corpus through the same
/// project wrapper as everything else.
///
/// Skips with a loud ledger line when #2282 has not landed, rather than
/// failing — the harness must be correct either way. It cannot pass silently:
/// `coverage_ledger_is_complete_and_reconciles` asserts the ledger names the
/// corpus as a gap in exactly that case.
/// Execute the full upstream corpus through the project wrappers and reconcile
/// the run against the corpus's own declared `numberOfTests` (#2264).
///
/// Shared by the execution test AND the accounting test, so removing, ignoring,
/// or early-returning one of them still leaves the other actually driving the
/// vectors — a vendored fixture can never be silently orphaned again (review
/// of PR #2369). Returns the summary for the caller to `finish()` or drop.
fn run_upstream_ed25519_corpus() -> Summary {
    let file: UpstreamFile = serde_json::from_str(&fixture(UPSTREAM_ED25519))
        .expect("published wycheproof ed25519 corpus must parse");
    let mut summary = Summary::default();

    for group in &file.test_groups {
        let pk_bytes = hex::decode(&group.public_key.pk).expect("group pk hex");
        for v in &group.tests {
            let expected = ExpectedResult::parse(&v.result);
            let msg = hex::decode(&v.msg).expect("msg hex");
            let sig_bytes = hex::decode(&v.sig).expect("sig hex");
            let tc = format!("tcId={}", v.tc_id);

            // A key the wrapper refuses to parse can only be an `invalid` case.
            let Ok(pk) = PublicKey::from_bytes(&pk_bytes) else {
                summary.record(&tc, expected == ExpectedResult::Invalid, || {
                    "public key failed to parse but vector expected valid".into()
                });
                continue;
            };

            let via_bytes = pk.verify_signature(&msg, &sig_bytes);
            let via_typed = Signature::from_bytes(&sig_bytes)
                .map(|sig| pk.verify(&msg, &sig))
                .unwrap_or(false);

            let actual = if via_bytes {
                ExpectedResult::Valid
            } else {
                ExpectedResult::Invalid
            };

            summary.record(&tc, actual == expected && via_bytes == via_typed, || {
                format!(
                    "expected {expected:?}, got via_bytes={via_bytes} via_typed={via_typed} \
                     (flags={:?}; {})",
                    v.flags, v.comment
                )
            });
        }
    }

    // #2264: reconcile against the corpus's own declared size, in BOTH
    // directions — the file's `numberOfTests` versus the groups it actually
    // carries, and versus what this harness actually executed. `passed > 0`
    // alone would stay green if a schema drift on re-pin silently dropped
    // groups; the vendor-integrity gate (`scripts/check_wycheproof_vendor.py`)
    // pins the bytes, and this pins the *interpretation* of the bytes.
    let group_sum: usize = file.test_groups.iter().map(|g| g.tests.len()).sum();
    assert_eq!(
        group_sum, file.number_of_tests,
        "corpus declares numberOfTests={} but its groups carry {group_sum} — the \
         vendored file is internally inconsistent (re-pin gone wrong?)",
        file.number_of_tests
    );
    let executed = summary.passed + summary.skipped.len() + summary.failures.len();
    assert_eq!(
        executed, file.number_of_tests,
        "harness executed {executed} of the corpus's declared {} vectors — some were \
         silently not driven, which overstates coverage (#2264)",
        file.number_of_tests
    );

    summary
}

#[test]
fn ed25519_wrapper_matches_published_wycheproof_corpus() {
    if !upstream_available() {
        eprintln!(
            "[vectors:ed25519-wycheproof-upstream] SKIPPED — {UPSTREAM_ED25519} not \
             vendored in this checkout (core4#2282). Recorded in the coverage ledger."
        );
        return;
    }
    run_upstream_ed25519_corpus().finish("ed25519-wycheproof-upstream");
}

#[test]
fn every_vendored_vector_is_executed_and_accounted_for() {
    // Reconciles executed vectors against the corpus size, so a vector that is
    // vendored but silently never run cannot hide.
    let ed: Ed25519File =
        serde_json::from_str(&fixture(LOCAL_ED25519)).expect("local ed25519 controls must parse");
    let hash: HashFile = serde_json::from_str(&fixture(NIST_HASH)).expect("NIST hash vectors must parse");

    // Each ed25519 vector is executed by `ed25519_wrapper_matches_vectors`; each
    // hash vector by `hash_wrappers_match_nist_vectors`. Assert the corpus is
    // non-trivial and that the known algorithm set is exactly what the harness
    // knows how to drive.
    assert!(!ed.tests.is_empty() && !hash.tests.is_empty());

    // The upstream corpus joins the same accounting when vendored (#2264):
    // this test EXECUTES it via the shared runner rather than only parsing it
    // (review of PR #2369) — so deleting, `#[ignore]`-ing, or early-returning
    // `ed25519_wrapper_matches_published_wycheproof_corpus` still leaves the
    // vectors actually driven and reconciled here, and the fixture cannot be
    // silently orphaned again. The runner asserts declared==carried==executed;
    // this test additionally requires the run to be non-trivial and clean.
    if upstream_available() {
        let summary = run_upstream_ed25519_corpus();
        assert!(
            summary.passed > 0 && summary.failures.is_empty(),
            "upstream corpus run must be non-trivial and clean: passed={} failures={:?}",
            summary.passed,
            summary.failures
        );
    }

    for v in &hash.tests {
        assert!(
            matches!(v.algo.as_str(), "sha256" | "sha3_256"),
            "hash vector {} names algo {}, which the harness cannot execute — it \
             would be counted as skipped and overstate coverage",
            v.tc_id,
            v.algo
        );
    }
}
