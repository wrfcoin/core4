//! Build provenance embedded at compile time (#2721).
//!
//! Before this crate, nothing a built binary could tell you identified the
//! commit it came from: `wrfcoin --version` answered `wrfcoin 0.1.0` from a
//! hardcoded literal identical for every commit in the repository's history,
//! and the two ceremony examples had no `--version` at all. The genesis cut
//! card's A1 abort states that a node built from the wrong commit is invisible
//! to the chain and that build discipline is the only defence; this crate is
//! the verification step behind that defence.
//!
//! See `build.rs` for the rebuild policy and for why the dirty flag is
//! `unknown` rather than `clean` whenever it cannot be established.

use std::sync::OnceLock;

/// The four-way decision about what a binary may claim, shared with `build.rs`
/// so every branch — `clean` included — is unit-tested on each CI run.
pub mod claim;

/// Full commit SHA the binary was built from, or `unknown`.
pub const COMMIT: &str = env!("WRFCOIN_BUILD_COMMIT");

/// First nine characters of [`COMMIT`], or `unknown`.
pub const COMMIT_SHORT: &str = env!("WRFCOIN_BUILD_COMMIT_SHORT");

/// `clean`, `dirty`, `unverified` or `unknown` — never optimistic.
///
/// `clean` is emitted ONLY by an attested build (see [`claim`]), which recomputes
/// provenance for every artifact it produces. A clean tree in an ordinary
/// developer build reports `unverified`, because that value can be replayed by
/// cargo onto a binary built from a tree it never saw.
pub const STATE: &str = env!("WRFCOIN_BUILD_STATE");

/// UTC build timestamp, `SOURCE_DATE_EPOCH` when that was set.
pub const TIMESTAMP: &str = env!("WRFCOIN_BUILD_TIMESTAMP");

/// `SOURCE_DATE_EPOCH`, `build-time`, or `unknown`.
pub const TIMESTAMP_SOURCE: &str = env!("WRFCOIN_BUILD_TIMESTAMP_SOURCE");

/// True only for an attested clean tree.
///
/// Neither `unknown` nor `unverified` is clean. An operator asking "can I trust
/// this binary's SHA" must get `false` whenever the answer was not positively
/// established for THIS artifact — which, before #2882, it frequently was not.
pub const fn is_clean() -> bool {
    matches!(STATE.as_bytes(), b"clean")
}

/// Version and provenance WITHOUT a leading program name:
/// `<semver> (<sha> <state>, built <ts>)`.
///
/// This is the form clap wants. clap renders `--version` as
/// `{bin_name} {version}` and prepends the name itself; passing a string that
/// already carried it printed `wrfcoin wrfcoin 0.1.0 (...)`.
///
/// The state word is always present, so a reader never has to know whether its
/// absence meant "clean" or "the field was dropped".
pub fn version_suffix(semver: &str) -> String {
    format!("{semver} ({COMMIT_SHORT} {STATE}, built {TIMESTAMP})")
}

/// Multi-line detail without a leading program name, carrying the full SHA.
///
/// The short SHA is for humans; an operator verifying a ceremony binary
/// against a recorded commit needs all 40 characters.
pub fn version_detail_body(semver: &str) -> String {
    format!(
        "{}\ncommit:    {COMMIT}\ntree:      {STATE}\nbuilt:     {TIMESTAMP} ({TIMESTAMP_SOURCE})",
        version_suffix(semver)
    )
}

/// One-line version string including the program name, for callers that print
/// it themselves rather than handing it to clap.
pub fn version_line(name: &str, semver: &str) -> String {
    format!("{name} {}", version_suffix(semver))
}

/// [`version_detail_body`] with the program name, for hand-rolled `--version`.
pub fn version_detail(name: &str, semver: &str) -> String {
    format!("{name} {}", version_detail_body(semver))
}

/// [`version_suffix`] as `&'static str`, computed once.
///
/// clap's `Str` has no `From<String>`, so the derive needs a borrow that lives
/// for the program. Interning here — rather than leaking at the call site —
/// keeps that detail out of the CLI and makes the allocation happen once no
/// matter how many times clap rebuilds its `Command`.
pub fn version_suffix_static(semver: &str) -> &'static str {
    static CELL: OnceLock<String> = OnceLock::new();
    CELL.get_or_init(|| version_suffix(semver)).as_str()
}

/// [`version_detail_body`] as `&'static str`. See [`version_suffix_static`].
pub fn version_detail_body_static(semver: &str) -> &'static str {
    static CELL: OnceLock<String> = OnceLock::new();
    CELL.get_or_init(|| version_detail_body(semver)).as_str()
}

/// Handle `--version` / `-V` for the two ceremony examples, which parse argv by
/// hand and have no clap. Returns true when the caller should exit 0.
///
/// This must be consulted BEFORE argument-count validation. At `df1c6fabd`
/// `oracle_ceremony_sign --version` exited 2 with a usage line and
/// `oracle_ceremony_verify --version` PANICKED (exit 101) trying to open a file
/// literally named `--version`, because both counted arguments first.
pub fn print_version_if_requested(name: &str, semver: &str, args: &[String]) -> bool {
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{}", version_detail(name, semver));
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of #2721: a binary must be able to name its commit.
    /// RED before this crate existed — there was no such value anywhere.
    #[test]
    fn commit_is_a_real_sha_when_built_in_a_repository() {
        assert_ne!(COMMIT, "", "commit must never be empty");
        if COMMIT != "unknown" {
            assert_eq!(COMMIT.len(), 40, "a git SHA-1 is 40 hex characters: {COMMIT}");
            assert!(
                COMMIT.chars().all(|c| c.is_ascii_hexdigit()),
                "commit must be hex: {COMMIT}"
            );
            assert_eq!(COMMIT_SHORT, &COMMIT[..9]);
        }
    }

    /// Fail-closed: `unknown` must not read as clean. This is the assertion
    /// that stops a future refactor from defaulting the flag to something
    /// reassuring.
    #[test]
    fn unknown_state_is_not_treated_as_clean() {
        assert!(
            matches!(STATE, "clean" | "dirty" | "unverified" | "unknown"),
            "STATE={STATE}"
        );
        if STATE != "clean" {
            assert!(!is_clean(), "only a positively clean tree may report clean");
        }
    }

    /// A version string that omits the state would let a dirty binary look
    /// like a clean one.
    #[test]
    fn version_line_carries_commit_and_state() {
        let line = version_line("wrfcoin", "0.1.0");
        assert!(line.starts_with("wrfcoin 0.1.0 ("), "{line}");
        assert!(line.contains(COMMIT_SHORT), "{line}");
        assert!(line.contains(STATE), "{line}");
    }

    /// clap prepends the binary name itself. A version string carrying its own
    /// name printed `wrfcoin wrfcoin 0.1.0 (...)` — caught by running the built
    /// binary, not by any compile check, which is why it is pinned here.
    #[test]
    fn clap_facing_forms_carry_no_program_name() {
        let suffix = version_suffix("0.1.0");
        assert!(suffix.starts_with("0.1.0 ("), "{suffix}");
        assert!(!suffix.contains("wrfcoin "), "{suffix}");

        let body = version_detail_body("0.1.0");
        assert_eq!(body.lines().next().unwrap(), suffix, "{body}");
        assert!(body.contains(COMMIT), "{body}");
    }

    /// The named form is exactly the clap-facing form with a name in front.
    /// If these drift, one surface starts telling a different story.
    #[test]
    fn named_forms_are_the_unnamed_forms_prefixed() {
        assert_eq!(version_line("p", "0.1.0"), format!("p {}", version_suffix("0.1.0")));
        assert_eq!(
            version_detail("p", "0.1.0"),
            format!("p {}", version_detail_body("0.1.0"))
        );
    }

    /// The detail block must carry all 40 characters; the short SHA is not
    /// enough to verify a ceremony binary against a recorded commit.
    #[test]
    fn version_detail_carries_the_full_sha() {
        let detail = version_detail("oracle_ceremony_sign", "0.1.0");
        assert!(detail.contains(COMMIT), "{detail}");
        assert!(detail.contains(TIMESTAMP_SOURCE), "{detail}");
        assert_eq!(detail.lines().count(), 4, "{detail}");
    }

    /// Both spellings, and only when actually asked.
    #[test]
    fn version_flag_is_recognised_in_both_spellings() {
        let long = vec!["prog".to_string(), "--version".to_string()];
        let short = vec!["prog".to_string(), "-V".to_string()];
        let neither = vec!["prog".to_string(), "bundle.json".to_string()];
        assert!(print_version_if_requested("p", "0.1.0", &long));
        assert!(print_version_if_requested("p", "0.1.0", &short));
        assert!(!print_version_if_requested("p", "0.1.0", &neither));
    }

    /// #2721's own reproduction: the flag must be found even when it arrives
    /// after the positional arguments, because argv order is the operator's
    /// choice, not ours.
    #[test]
    fn version_flag_is_found_after_positional_arguments() {
        let trailing = vec![
            "oracle_ceremony_verify".to_string(),
            "bundle.json".to_string(),
            "--version".to_string(),
        ];
        assert!(print_version_if_requested("p", "0.1.0", &trailing));
    }

    /// The timestamp is embedded to be read by an operator, so it has to be a


    /// The whole of #2882, as a table. Every branch runs on every CI run —
    /// including `clean`, which under this design is otherwise reached only
    /// during a release ceremony. A path exercised only at ceremony is a path
    /// that rots unobserved, which is the same failure mode as a gate that
    /// never runs.
    #[test]
    fn classify_covers_every_branch_including_the_attested_clean_one() {
        use claim::{Worktree, CLEAN, DIRTY, UNKNOWN, UNVERIFIED};

        assert_eq!(claim::classify(Worktree::Clean, true), CLEAN);
        assert_eq!(claim::classify(Worktree::Clean, false), UNVERIFIED);
        assert_eq!(claim::classify(Worktree::Dirty, true), DIRTY);
        assert_eq!(claim::classify(Worktree::Dirty, false), DIRTY);
        assert_eq!(claim::classify(Worktree::Unavailable, true), UNKNOWN);
        assert_eq!(claim::classify(Worktree::Unavailable, false), UNKNOWN);
    }

    /// The load-bearing asymmetry. `clean` is reachable from EXACTLY ONE input
    /// combination; every other combination must refuse it. This is the
    /// assertion that a future refactor has to break in order to reintroduce
    /// #2882, so it is stated as an exhaustive sweep rather than as examples.
    #[test]
    fn clean_is_reachable_only_from_an_attested_clean_tree() {
        use claim::{Worktree, CLEAN};

        let mut clean_count = 0;
        for worktree in [Worktree::Clean, Worktree::Dirty, Worktree::Unavailable] {
            for attested in [true, false] {
                if claim::classify(worktree, attested) == CLEAN {
                    clean_count += 1;
                    assert_eq!(worktree, Worktree::Clean, "clean from a non-clean tree");
                    assert!(attested, "clean without attestation is the #2882 defect");
                }
            }
        }
        assert_eq!(clean_count, 1, "exactly one input combination may yield `clean`");
    }

    /// A cached `dirty` or `unverified` replayed onto a later binary is
    /// harmless because it under-claims. That is the property the whole design
    /// rests on, so it is pinned: no unattested build may produce a value that
    /// `is_clean()`-style logic would accept.
    #[test]
    fn no_unattested_input_can_produce_an_affirmative_claim() {
        use claim::{is_attested_clean, Worktree};

        for worktree in [Worktree::Clean, Worktree::Dirty, Worktree::Unavailable] {
            let state = claim::classify(worktree, false);
            assert!(
                !is_attested_clean(state),
                "unattested {worktree:?} produced an affirmative claim: {state}"
            );
        }
    }

    /// `unverified` and `unknown` must stay distinct: one says "re-run the
    /// build with attestation", the other says "there is no provenance here at
    /// all". Collapsing them loses the distinction the review asked to keep.
    #[test]
    fn unverified_is_distinguishable_from_provenance_unavailable() {
        use claim::{Worktree, UNKNOWN, UNVERIFIED};

        assert_ne!(UNVERIFIED, UNKNOWN);
        assert_eq!(claim::classify(Worktree::Clean, false), UNVERIFIED);
        assert_eq!(claim::classify(Worktree::Unavailable, false), UNKNOWN);
    }

    /// Attestation requires a live token AND the wrapper's file agreeing with it.
    ///
    /// Round 4 checked only that the token was 64-hex. A reviewer replayed the
    /// digest printed by a REJECTED wrapper run into a direct cargo invocation
    /// and reproduced the original TOCTOU — the token proved shape, not that a
    /// wrapper run was underway, so it bound nothing (#2882 rd5).
    #[test]
    fn attestation_requires_a_live_token_file_that_agrees() {
        let digest = "a".repeat(64);
        let other = "b".repeat(64);

        assert!(claim::attestation_requested(Some(&digest), Some(&digest)));
        assert!(claim::attestation_requested(Some(&digest), Some(&format!("{digest}\n"))));

        // THE REPLAY: a well-formed token with no live file must not attest.
        assert!(
            !claim::attestation_requested(Some(&digest), None),
            "a replayed digest with no wrapper file is the round 4 defect"
        );
        // A file that disagrees is a stale token from some other run.
        assert!(!claim::attestation_requested(Some(&digest), Some(&other)));
        assert!(!claim::attestation_requested(Some(&digest), Some("")));

        // Shape still matters.
        for bad in ["", "   ", "0", "1", "yes", "true"] {
            assert!(!claim::attestation_requested(Some(bad), Some(bad)), "{bad:?}");
        }
        assert!(!claim::attestation_requested(None, Some(&digest)));
        assert!(!claim::attestation_requested(Some(&"a".repeat(63)), Some(&"a".repeat(63))));
        assert!(!claim::attestation_requested(Some(&"A".repeat(64)), Some(&"A".repeat(64))));
        assert!(!claim::attestation_requested(Some(&"g".repeat(64)), Some(&"g".repeat(64))));
    }

}
